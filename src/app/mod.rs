//! Application controller: owns all state on the UI thread and executes commands.

mod commands;
mod menu;
mod middle;
mod narrow;
mod panes;
mod shellops;
mod tree;
pub mod worker;
mod workspaces;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::DragAcceptFiles;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::cli::RunArgs;
use crate::fs::enumerate::{EnumEvent, Pidl};
use crate::fs::watch::{WatchEvent, Watcher};
use crate::instance;
use crate::model::layout::{self, LayoutRects, LayoutState, Rect};
use crate::model::listing::Listing;
use crate::model::workspace::*;
use crate::shell::icons::{IconCache, ImageLists};
use crate::store::autosave::{AUTOSAVE_DELAY, Debouncer};
use crate::store::config::Config;
use crate::store::{self, paths};
use crate::ui::accel::KeyMap;
use crate::ui::navtree::{NavChild, NavTree};
use crate::ui::{self, dpi, theme};
use worker::{Inbox, Notifier, Pool, WM_APP_WORKER};

pub const WM_APP_IDLE_INIT: u32 = WM_APP + 2;
pub const TIMER_AUTOSAVE: usize = 1;
pub const TIMER_STATUS_CLEAR: usize = 2;

pub const ID_LIST: [i32; 2] = [100, 101];
pub const ID_EDIT: [i32; 2] = [130, 131];
pub const ID_RETRY: [i32; 2] = [150, 151];
pub const ID_PARENT: [i32; 2] = [160, 161];
pub const ID_FILTER_EDIT: [i32; 2] = [170, 171];
pub const ID_TREE: i32 = 110;
pub const ID_STATUS: i32 = 120;

pub enum WorkerMsg {
    Enum {
        tab: TabId,
        generation: u64,
        event: EnumEvent,
    },
    Watch(WatchEvent),
    NavChildren {
        node: usize,
        generation: u64,
        children: Vec<NavChild>,
    },
    Icon {
        tab: TabId,
        generation: u64,
        name: Box<[u16]>,
        icon: i32,
    },
    Thumb {
        tab: TabId,
        generation: u64,
        name: Box<[u16]>,
        key: String,
        bmp: Option<isize>,
    },
    FileOp {
        result: crate::shell::fileop::OpResult,
        after: shellops::AfterOp,
    },
}

/// Runtime state of a tab that is not persisted.
#[derive(Default)]
pub struct TabRt {
    pub generation: u64,
    pub cancel: Arc<AtomicBool>,
    pub listing: Option<Listing>,
    /// Refresh in progress: replaces `listing` when complete, keeping selection by name.
    pub pending: Option<Listing>,
    pub title: String,
    pub top_index: usize,
    pub select_on_load: Option<Box<[u16]>>,
    /// PIDL of a virtual (shell) folder, used to resolve child items.
    pub folder_pidl: Option<Pidl>,
}

pub struct PaneUi {
    pub tabbar: HWND,
    pub address: HWND,
    pub edit: HWND,
    pub list: HWND,
    pub msg: HWND,
    pub retry: HWND,
    pub parent_btn: HWND,
    /// Narrowing input under the list and its edit control.
    pub filter_bar: HWND,
    pub filter_edit: HWND,
    pub shown: Option<TabId>,
    pub rects: Option<layout::PaneRects>,
    pub addr_error: bool,
}

pub struct SplitDrag {
    pub nav: bool,
}

pub struct TabDrag {
    pub from: (PaneSide, usize),
    pub start: (i32, i32),
    pub active: bool,
    pub target: Option<(PaneSide, usize)>,
}

pub struct App {
    pub hwnd: HWND,
    pub base: PathBuf,
    pub config: Config,
    pub ws: Workspace,
    pub rt: HashMap<TabId, TabRt>,
    pub panes: [PaneUi; 2],
    pub nav: NavTree,
    pub status: HWND,
    pub dpi: u32,
    pub font: HFONT,
    pub keymap: KeyMap,
    pub lists: ImageLists,
    pub icons: IconCache,
    pub enum_pool: Pool,
    pub icon_pool: Pool,
    pub watcher: Option<Watcher>,
    pub notifier: Notifier<WorkerMsg>,
    pub inbox: Inbox<WorkerMsg>,
    pub autosave: Debouncer,
    pub config_dirty: bool,
    pub home: Location,
    pub next_generation: u64,
    pub suppress_selection: bool,
    /// Set while the filter bar text is replaced programmatically (ignore EN_CHANGE).
    pub narrow_syncing: bool,
    /// ":" command line open under the list, indexed by `PaneSide::index()`.
    pub command_line: [bool; 2],
    pub split_drag: Option<SplitDrag>,
    pub tab_drag: Option<TabDrag>,
    pub menu: menu::MenuState,
    pub status_message: Option<String>,
    pub editing_label: bool,
    pub fileop_pool: Pool,
    pub undo_stack: Vec<crate::shell::undo::UndoOp>,
    pub drop_targets: Vec<windows::Win32::System::Ole::IDropTarget>,
    pub clip: Option<windows::Win32::System::Com::IDataObject>,
    pub pending_rename: Option<(TabId, Box<[u16]>)>,
    pub drag_hover: Option<((PaneSide, usize), Instant)>,
    pub thumbs: crate::shell::thumbs::ThumbCache,
}

pub fn run(base: PathBuf, key: u32, args: RunArgs) -> std::result::Result<(), String> {
    let trace = std::env::var_os("TABEX_TRACE").is_some();
    let t0 = Instant::now();
    unsafe {
        OleInitialize(None).map_err(|e| e.message())?;
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES | ICC_STANDARD_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);
    }
    let result = unsafe { run_inner(base, key, args, t0, trace) };
    unsafe { CoUninitialize() };
    result.map_err(|e| e.message())
}

pub fn trace(t0: Instant, enabled: bool, stage: &str) {
    if !enabled {
        return;
    }
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("tabex");
    let _ = std::fs::create_dir_all(&dir);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("trace.log"))
    {
        let _ = writeln!(f, "{:>8.2} ms  {stage}", t0.elapsed().as_secs_f64() * 1000.0);
    }
}

unsafe fn register_classes(instance: HINSTANCE) -> Result<()> {
    unsafe {
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let icon = LoadIconW(Some(instance), PCWSTR(1 as _)).unwrap_or_default();
        let main = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            hInstance: instance,
            lpszClassName: instance::MAIN_CLASS,
            lpfnWndProc: Some(main_proc),
            hCursor: cursor,
            hIcon: icon,
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        };
        RegisterClassExW(&main);
        ui::tabbar::register(instance, cursor)?;
        ui::addressbar::register(instance, cursor)?;
        ui::filterbar::register(instance, cursor)?;
    }
    Ok(())
}

unsafe fn run_inner(base: PathBuf, key: u32, args: RunArgs, t0: Instant, tracing: bool) -> Result<()> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        register_classes(instance)?;

        let cfg_path = paths::config_path(&base);
        let (config, cfg_err) = match store::config::load(&cfg_path) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(e)),
        };
        let home = crate::shell::names::home();
        let downloads = crate::shell::names::downloads();
        let wanted = args.workspace.clone().or_else(|| config.last_workspace.clone());
        let opened = store::open_or_create(&base, wanted.as_deref(), (&home, &downloads));
        trace(t0, tracing, "config and workspace loaded");

        let placement = config.window;
        let (x, y, w, h) = match placement {
            Some(p) => (p.x, p.y, p.width.max(400), p.height.max(300)),
            None => (CW_USEDEFAULT, CW_USEDEFAULT, 1400, 900),
        };
        let hwnd = CreateWindowExW(
            WS_EX_ACCEPTFILES,
            instance::MAIN_CLASS,
            w!("tabex"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance),
            None,
        )?;
        DragAcceptFiles(hwnd, false);
        instance::tag_window(hwnd, key);
        theme::apply_title_bar(hwnd);

        let mut app = Box::new(App::new(hwnd, base, config, opened.workspace, home)?);
        let app_ptr: *mut App = &mut *app;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        let app = &mut *app_ptr;
        trace(t0, tracing, "window and controls created");

        app.register_drop_targets();
        app.install_middle_click();
        app.apply_run_args(&args, false);
        app.refresh_panes();
        app.layout();
        let show = if placement.is_some_and(|p| p.maximized) {
            SW_SHOWMAXIMIZED
        } else {
            SW_SHOWNORMAL
        };
        let _ = ShowWindow(hwnd, show);
        let _ = UpdateWindow(hwnd);
        app.focus_active_list();
        trace(t0, tracing, "window shown");

        if let Some(e) = cfg_err {
            app.error_box(&format!(
                "設定ファイルを読み込めませんでした。既定の設定で起動します。\n\n{e}"
            ));
        }
        if let Some(e) = opened.error {
            app.error_box(&format!(
                "ワークスペースを読み込めませんでした。新しいワークスペース \"{}\" で起動します。\n読み込めなかったファイルは変更していません。\n\n{e}",
                app.ws.name
            ));
        }
        let _ = PostMessageW(Some(hwnd), WM_APP_IDLE_INIT, WPARAM(0), LPARAM(0));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if let Some(app) = App::from_hwnd(hwnd)
                && app.pre_translate(&msg)
            {
                continue;
            }
            if ui::workspace_dialog::is_dialog_message(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

impl App {
    fn new(hwnd: HWND, base: PathBuf, config: Config, ws: Workspace, home: Location) -> Result<App> {
        let dpi = dpi::window_dpi(hwnd);
        let font = dpi::ui_font(dpi);
        let lists = crate::shell::icons::image_lists();
        let mut panes = Vec::with_capacity(2);
        for side in PaneSide::BOTH {
            panes.push(panes::create_pane_ui(hwnd, side, font)?);
        }
        let panes: [PaneUi; 2] = panes.try_into().ok().unwrap();
        let nav = NavTree::create(hwnd, ID_TREE, lists.small)?;
        let status = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                STATUSCLASSNAMEW,
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE((CCS_NOPARENTALIGN | CCS_NORESIZE) as u32),
                0,
                0,
                0,
                0,
                Some(hwnd),
                Some(HMENU(ID_STATUS as usize as *mut _)),
                None,
                None,
            )?
        };
        let (notifier, inbox) = worker::channel_for::<WorkerMsg>(hwnd);
        let wn = notifier.clone();
        let watcher = Watcher::new(move |ev| wn.send(WorkerMsg::Watch(ev))).ok();
        let (keymap, key_warnings) = KeyMap::new(&config.keys);
        let mut app = App {
            hwnd,
            base,
            config,
            ws,
            rt: HashMap::new(),
            panes,
            nav,
            status,
            dpi,
            font,
            keymap,
            lists,
            icons: IconCache::default(),
            enum_pool: Pool::new("enum", 2),
            icon_pool: Pool::new("icon", 2),
            watcher,
            notifier,
            inbox,
            autosave: Debouncer::new(AUTOSAVE_DELAY),
            config_dirty: false,
            home,
            next_generation: 1,
            suppress_selection: false,
            narrow_syncing: false,
            command_line: [false; 2],
            split_drag: None,
            tab_drag: None,
            menu: menu::MenuState::default(),
            status_message: None,
            editing_label: false,
            fileop_pool: Pool::new("fileop", 1),
            undo_stack: Vec::new(),
            drop_targets: Vec::new(),
            clip: None,
            pending_rename: None,
            drag_hover: None,
            thumbs: Default::default(),
        };
        app.apply_font();
        unsafe {
            let _ = SetMenu(hwnd, Some(app.menu.build()));
        }
        if !key_warnings.is_empty() {
            app.show_status(&key_warnings.join(" / "));
        }
        Ok(app)
    }

    pub fn from_hwnd(hwnd: HWND) -> Option<&'static mut App> {
        unsafe {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if p.is_null() { None } else { Some(&mut *p) }
        }
    }

    pub fn apply_font(&self) {
        let set = |h: HWND| unsafe {
            SendMessageW(h, WM_SETFONT, Some(WPARAM(self.font.0 as usize)), Some(LPARAM(1)));
        };
        for p in &self.panes {
            for h in [p.list, p.edit, p.filter_edit, p.msg, p.retry, p.parent_btn] {
                set(h);
            }
        }
        set(self.nav.hwnd);
        set(self.status);
    }

    pub fn layout_state(&self) -> LayoutState {
        LayoutState {
            layout: self.ws.layout,
            split_ratio: self.ws.split_ratio,
            active_pane: self.ws.active_pane,
            nav_visible: self.config.nav_tree_visible,
            nav_width: self.config.nav_width,
            filter_bar: PaneSide::BOTH.map(|side| self.filter_bar_visible(side)),
        }
    }

    pub fn compute_layout(&self) -> LayoutRects {
        layout::compute(ui::client_rect(self.hwnd), self.dpi, &self.layout_state())
    }

    pub fn layout(&mut self) {
        let r = self.compute_layout();
        match r.nav {
            Some(nav) => ui::place(self.nav.hwnd, nav, true),
            None => ui::place(self.nav.hwnd, Rect::default(), false),
        }
        for side in PaneSide::BOTH {
            let i = side.index();
            let pr = r.panes[i];
            self.panes[i].rects = pr;
            let p = &self.panes[i];
            match pr {
                Some(pr) => {
                    ui::place(p.tabbar, pr.tabbar, true);
                    ui::place(p.address, pr.address, true);
                    ui::place(p.list, pr.list, true);
                    ui::place(p.filter_bar, pr.filter.unwrap_or_default(), pr.filter.is_some());
                }
                None => {
                    for h in [p.tabbar, p.address, p.list, p.filter_bar] {
                        ui::place(h, Rect::default(), false);
                    }
                }
            }
            self.place_placeholder(side);
        }
        ui::place(self.status, r.status, true);
        let w = r.status.width();
        let parts = [w / 5, w * 2 / 5, w * 3 / 5, -1];
        unsafe {
            SendMessageW(
                self.status,
                SB_SETPARTS,
                Some(WPARAM(parts.len())),
                Some(LPARAM(parts.as_ptr() as isize)),
            );
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    pub fn mark_dirty(&mut self) {
        self.autosave.mark(Instant::now());
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_AUTOSAVE, AUTOSAVE_DELAY.as_millis() as u32, None);
        }
    }

    pub fn show_status(&mut self, text: &str) {
        self.status_message = Some(text.to_string());
        self.update_status();
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_STATUS_CLEAR, 8000, None);
        }
    }

    pub fn error_box(&self, text: &str) {
        unsafe {
            MessageBoxW(
                Some(self.hwnd),
                &HSTRING::from(text),
                w!("tabex"),
                MB_ICONWARNING | MB_OK,
            );
        }
    }

    pub fn update_status(&self) {
        let set = |i: usize, s: &str| unsafe {
            let w = ui::wz(s);
            SendMessageW(
                self.status,
                SB_SETTEXTW,
                Some(WPARAM(i)),
                Some(LPARAM(w.as_ptr() as isize)),
            );
        };
        let tab = self.ws.active_tab();
        let rt = self.rt.get(&tab.id);
        let listing = rt.and_then(|r| r.listing.as_ref());
        let (count, sel, size) = match listing {
            Some(l) => (l.len(), l.selection_count(), l.selected_size()),
            None => (0, 0, 0),
        };
        let loading = matches!(tab.status, TabStatus::Loading);
        set(
            0,
            &format!(
                "{} 個の項目{}",
                ui::format::group_thousands(count as u64),
                if loading { " (読み込み中)" } else { "" }
            ),
        );
        set(
            1,
            &if sel > 0 {
                format!("{} 個の項目を選択", ui::format::group_thousands(sel as u64))
            } else {
                String::new()
            },
        );
        set(
            2,
            &if size > 0 {
                ui::format::size_human(size)
            } else {
                String::new()
            },
        );
        set(3, self.status_message.as_deref().unwrap_or(""));
    }

    pub fn update_title(&self) {
        let tab = self.ws.active_tab();
        let title = self.rt.get(&tab.id).map(|r| r.title.clone()).unwrap_or_default();
        let s = format!("{title} - {} - tabex", self.ws.name);
        unsafe {
            let _ = SetWindowTextW(self.hwnd, &HSTRING::from(s));
        }
    }

    pub fn focus_active_list(&self) {
        unsafe {
            let _ = SetFocus(Some(self.panes[self.ws.active_pane.index()].list));
        }
    }

    pub fn set_active_pane(&mut self, side: PaneSide) {
        if self.ws.active_pane != side {
            self.ws.active_pane = side;
            self.mark_dirty();
            if self.ws.layout == Layout::Single {
                self.refresh_panes();
                self.layout();
            }
            self.invalidate_chrome();
            self.update_status();
            self.update_title();
        }
    }

    pub fn invalidate_chrome(&self) {
        for p in &self.panes {
            unsafe {
                let _ = InvalidateRect(Some(p.tabbar), None, false);
                let _ = InvalidateRect(Some(p.address), None, false);
            }
        }
    }

    pub fn pane_of_hwnd(&self, h: HWND) -> Option<PaneSide> {
        PaneSide::BOTH.into_iter().find(|s| {
            let p = &self.panes[s.index()];
            [p.list, p.tabbar, p.address, p.edit, p.retry, p.parent_btn, p.filter_bar].contains(&h)
                || unsafe {
                    IsChild(p.list, h).as_bool()
                        || IsChild(p.address, h).as_bool()
                        || IsChild(p.filter_bar, h).as_bool()
                }
        })
    }

    /// Keyboard shortcuts and mouse X buttons, handled before normal dispatch.
    fn pre_translate(&mut self, msg: &MSG) -> bool {
        match msg.message {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if ui::workspace_dialog::owns(msg.hwnd) {
                    return false;
                }
                let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) } < 0;
                let spec = ui::accel::KeySpec {
                    ctrl: down(VK_CONTROL),
                    shift: down(VK_SHIFT),
                    alt: down(VK_MENU),
                    vk: msg.wParam.0 as u16,
                };
                let focus = unsafe { GetFocus() };
                let in_edit = ui::class_name(focus).eq_ignore_ascii_case("Edit");
                if in_edit && ui::accel::is_edit_key(&spec) {
                    return false;
                }
                let Some(cmd) = self.keymap.lookup(&spec).cloned() else {
                    // Alt+letter opens the matching menu even when a child control has focus.
                    if spec.alt && !spec.ctrl && (0x41..=0x5A).contains(&spec.vk) && !in_edit {
                        unsafe {
                            let _ = PostMessageW(
                                Some(self.hwnd),
                                WM_SYSCOMMAND,
                                WPARAM(SC_KEYMENU as usize),
                                LPARAM((spec.vk + 0x20) as isize),
                            );
                        }
                        return true;
                    }
                    return false;
                };
                let in_list = self.panes.iter().any(|p| p.list == focus);
                if (ui::accel::is_list_command(&cmd) || ui::accel::is_list_only_key(&spec)) && !in_list {
                    return false;
                }
                // Tab in the tree or address bar keeps normal focus behaviour.
                if cmd == crate::model::command::Command::SwitchPane && !in_list {
                    return false;
                }
                if let Some(side) = self.pane_of_hwnd(focus) {
                    self.set_active_pane(side);
                }
                self.execute(cmd);
                true
            }
            WM_CHAR => {
                // Character bindings (":") act only in the file list, so typing elsewhere is untouched.
                let focus = unsafe { GetFocus() };
                let Some(side) = self.side_of_list(focus) else {
                    return false;
                };
                let Some(cmd) = char::from_u32(msg.wParam.0 as u32).and_then(|c| self.keymap.lookup_char(c).cloned())
                else {
                    return false;
                };
                self.set_active_pane(side);
                self.execute(cmd);
                true
            }
            WM_XBUTTONUP => {
                if let Some(side) = self.pane_of_hwnd(msg.hwnd) {
                    self.set_active_pane(side);
                }
                let back = ui::hiword(msg.wParam.0) == XBUTTON1;
                self.execute(if back {
                    crate::model::command::Command::Back
                } else {
                    crate::model::command::Command::Forward
                });
                true
            }
            _ => false,
        }
    }

    fn on_worker(&mut self) {
        for m in self.inbox.drain() {
            match m {
                WorkerMsg::Enum { tab, generation, event } => self.on_enum(tab, generation, event),
                WorkerMsg::Watch(ev) => self.on_watch(ev),
                WorkerMsg::NavChildren {
                    node,
                    generation,
                    children,
                } => {
                    let filter = self.filter();
                    self.nav.add_children(node, generation, children, filter);
                }
                WorkerMsg::Icon {
                    tab,
                    generation,
                    name,
                    icon,
                } => self.on_icon(tab, generation, &name, icon),
                WorkerMsg::Thumb {
                    tab,
                    generation,
                    name,
                    key,
                    bmp,
                } => self.on_thumb(tab, generation, &name, key, bmp),
                WorkerMsg::FileOp { result, after } => self.on_fileop_done(result, after),
            }
        }
    }

    fn on_timer(&mut self, id: usize) {
        match id {
            TIMER_AUTOSAVE => {
                if self.autosave.due(Instant::now()) {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TIMER_AUTOSAVE);
                    }
                    self.save_now();
                }
            }
            TIMER_STATUS_CLEAR => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_STATUS_CLEAR);
                }
                self.status_message = None;
                self.update_status();
            }
            _ => {}
        }
    }

    fn on_dpi_changed(&mut self, dpi: u32, suggested: &RECT) {
        for side in PaneSide::BOTH {
            self.capture_view(side);
        }
        self.dpi = dpi;
        let old = self.font;
        self.font = dpi::ui_font(dpi);
        self.apply_font();
        unsafe {
            let _ = DeleteObject(old.into());
            let _ = SetWindowPos(
                self.hwnd,
                None,
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        for side in PaneSide::BOTH {
            let p = &self.panes[side.index()];
            if let Some(id) = p.shown
                && let Some((s, i)) = self.ws.find_tab(id)
            {
                let view = self.ws.pane(s).tabs[i].view.clone();
                ui::listview::set_columns(p.list, &view, dpi);
            }
        }
        self.layout();
    }

    fn on_close(&mut self) {
        for side in PaneSide::BOTH {
            self.capture_view(side);
        }
        self.save_now();
        unsafe {
            let mut wp = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(self.hwnd, &mut wp).is_ok() {
                let r = wp.rcNormalPosition;
                self.config.window = Some(store::config::WindowPlacement {
                    x: r.left,
                    y: r.top,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                    maximized: wp.showCmd == SW_SHOWMAXIMIZED.0 as u32,
                });
            }
        }
        self.save_config();
        crate::shell::clipboard::flush_if_owner(self.clip.as_ref());
        for h in self
            .panes
            .iter()
            .flat_map(|p| [p.list, p.tabbar])
            .chain(std::iter::once(self.nav.hwnd))
        {
            unsafe {
                let _ = windows::Win32::System::Ole::RevokeDragDrop(h);
            }
        }
        self.drop_targets.clear();
        for rt in self.rt.values() {
            rt.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.enum_pool.clear_pending();
        self.icon_pool.clear_pending();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn splitter_hit(&self, x: i32, y: i32) -> Option<bool> {
        let r = self.compute_layout();
        if r.pane_splitter.is_some_and(|s| s.contains(x, y)) {
            return Some(false);
        }
        if r.nav_splitter.is_some_and(|s| s.contains(x, y)) {
            return Some(true);
        }
        None
    }

    fn on_split_drag(&mut self, x: i32) {
        let Some(d) = &self.split_drag else { return };
        let client = ui::client_rect(self.hwnd);
        if d.nav {
            let w96 = ((x - client.left) as i64 * 96 / self.dpi as i64) as i32;
            self.config.nav_width = w96.max(layout::MIN_NAV_WIDTH);
            self.config_dirty = true;
        } else {
            self.ws.split_ratio = layout::ratio_from_drag(client, self.dpi, &self.layout_state(), x);
            self.mark_dirty();
        }
        self.layout();
    }
}

extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(app) = App::from_hwnd(hwnd) else {
            return DefWindowProcW(hwnd, msg, wp, lp);
        };
        if crate::shell::context_menu::is_active()
            && let Some(r) = crate::shell::context_menu::handle_menu_msg(msg, wp, lp)
        {
            return r;
        }
        match msg {
            WM_CONTEXTMENU => {
                let from = HWND(wp.0 as *mut _);
                if let Some(side) = app.side_of_list(from) {
                    if lp.0 == -1 {
                        app.set_active_pane(side);
                        app.keyboard_context_menu(side);
                    }
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_SIZE => {
                app.layout();
                LRESULT(0)
            }
            WM_APP_WORKER => {
                app.on_worker();
                LRESULT(0)
            }
            WM_APP_IDLE_INIT => {
                app.build_nav_tree();
                LRESULT(0)
            }
            WM_TIMER => {
                app.on_timer(wp.0);
                LRESULT(0)
            }
            WM_NOTIFY => app.on_notify(&*(lp.0 as *const NMHDR), lp),
            WM_COMMAND => {
                app.on_command(ui::loword(wp.0) as i32, ui::hiword(wp.0), HWND(lp.0 as *mut _));
                LRESULT(0)
            }
            WM_INITMENUPOPUP => {
                app.on_init_menu_popup(HMENU(wp.0 as *mut _));
                LRESULT(0)
            }
            WM_COPYDATA => {
                if let Some(args) = instance::decode(lp) {
                    app.apply_run_args(&args, true);
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                }
                LRESULT(1)
            }
            WM_SETCURSOR => {
                if HWND(wp.0 as *mut _) == hwnd && ui::loword(lp.0 as usize) as u32 == HTCLIENT {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                    if app.splitter_hit(pt.x, pt.y).is_some() {
                        SetCursor(LoadCursorW(None, IDC_SIZEWE).ok());
                        return LRESULT(1);
                    }
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = ui::point_from_lparam(lp);
                if let Some(nav) = app.splitter_hit(x, y) {
                    app.split_drag = Some(SplitDrag { nav });
                    SetCapture(hwnd);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if app.split_drag.is_some() {
                    app.on_split_drag(ui::point_from_lparam(lp).0);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                if app.split_drag.take().is_some() {
                    let _ = ReleaseCapture();
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                app.on_dpi_changed(ui::hiword(wp.0) as u32, &*(lp.0 as *const RECT));
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                theme::apply_title_bar(hwnd);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ACTIVATE => {
                if ui::loword(wp.0) as u32 != WA_INACTIVE {
                    app.focus_active_list();
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                app.on_close();
                LRESULT(0)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wp.0 != 0 {
                    for side in PaneSide::BOTH {
                        app.capture_view(side);
                    }
                    app.save_now();
                    app.save_config();
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !p.is_null() {
                    drop(Box::from_raw(p));
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
