//! Settings dialog: display, home directories, key bindings and other settings on four
//! pages, plus the small dialog that edits one home directory.

use std::cell::RefCell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::accel::{KeyMap, KeySpec, KeyTable, format_key};
use crate::app::App;
use crate::model::command::{CATALOG, Command};
use crate::model::homes::is_absolute;
use crate::model::layout::scale;
use crate::store::config::{HomeSpec, UserSettings};

const SETTINGS_CLASS: PCWSTR = w!("TabexSettings");
const ENTRY_CLASS: PCWSTR = w!("TabexHomeEntry");

const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;
const ID_APPLY: i32 = 3;
const ID_TAB: i32 = 10;
const ID_SHOW: [i32; 4] = [20, 21, 22, 23];
const ID_HOMES: i32 = 30;
const ID_HOME_ADD: i32 = 31;
const ID_HOME_EDIT: i32 = 32;
const ID_HOME_DEL: i32 = 33;
const ID_HOME_UP: i32 = 34;
const ID_HOME_DOWN: i32 = 35;
const ID_CMDS: i32 = 40;
const ID_KEYS: i32 = 41;
const ID_CAPTURE: i32 = 42;
const ID_ASSIGN: i32 = 43;
const ID_KEY_DEL: i32 = 44;
const ID_KEY_RESET: i32 = 45;
const ID_KEY_RESET_ALL: i32 = 46;
const ID_SHELL: i32 = 50;
const ID_MIGEMO: i32 = 51;

const PAGES: [&str; 4] = ["表示", "ホームディレクトリ", "キー割り当て", "その他"];
const SHOW_LABELS: [&str; 4] = [
    "隠しファイル",
    "ドットファイル",
    "ファイル名拡張子",
    "ナビゲーション ウィンドウ",
];

struct State {
    hwnd: HWND,
    main: HWND,
    font: HFONT,
    bold: HFONT,
    tab: HWND,
    pages: [Vec<HWND>; 4],
    show: [HWND; 4],
    homes: HWND,
    home_buttons: [HWND; 4], // edit, delete, up, down
    cmds: HWND,
    keys: HWND,
    capture: HWND,
    key_buttons: [HWND; 3], // assign, delete, reset
    shell: HWND,
    migemo: HWND,
    apply: HWND,
    /// Settings being edited; `keys` is rebuilt from `table` when needed.
    work: UserSettings,
    /// Last saved settings: what "Apply" compares with.
    baseline: UserSettings,
    table: KeyTable,
    captured: Option<KeySpec>,
    applied: bool,
    /// Page shown now; read after the window is gone.
    page: usize,
    done: bool,
    /// Set while controls are filled programmatically (ignore their notifications).
    syncing: bool,
}

impl State {
    fn current(&self) -> UserSettings {
        let mut u = self.work.clone();
        u.keys = self.table.to_overrides(&self.baseline.keys);
        u
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Run `f` on the dialog state. Controls notify the dialog synchronously while `f` fills
/// them; such nested calls find the state borrowed and do nothing (`f` refreshes itself).
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

fn send(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wp)), Some(LPARAM(lp))) }
}

fn label_of(name: &str) -> &'static str {
    CATALOG.iter().find(|c| c.name == name).map_or("", |c| c.label)
}

// ---- list view helpers ----

fn lv_columns(lv: HWND, cols: &[(&str, i32)], dpi: u32) {
    for (i, (title, width)) in cols.iter().enumerate() {
        let mut t: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            cx: scale(*width, dpi),
            pszText: PWSTR(t.as_mut_ptr()),
            ..Default::default()
        };
        send(lv, LVM_INSERTCOLUMNW, i, &col as *const _ as isize);
    }
    let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER;
    send(lv, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as usize, ex as isize);
    unsafe {
        let _ = SetWindowTheme(lv, w!("Explorer"), PCWSTR::null());
    }
}

fn lv_fill(lv: HWND, rows: &[Vec<String>]) {
    send(lv, WM_SETREDRAW, 0, 0);
    send(lv, LVM_DELETEALLITEMS, 0, 0);
    for (i, row) in rows.iter().enumerate() {
        for (j, text) in row.iter().enumerate() {
            let mut t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: i as i32,
                iSubItem: j as i32,
                pszText: PWSTR(t.as_mut_ptr()),
                ..Default::default()
            };
            let msg = if j == 0 { LVM_INSERTITEMW } else { LVM_SETITEMTEXTW };
            send(lv, msg, i, &item as *const _ as isize);
        }
    }
    send(lv, WM_SETREDRAW, 1, 0);
    unsafe {
        let _ = InvalidateRect(Some(lv), None, true);
    }
}

fn lv_selected(lv: HWND) -> Option<usize> {
    let r = send(lv, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize).0;
    (r >= 0).then_some(r as usize)
}

fn lv_select(lv: HWND, index: usize) {
    let item = LVITEMW {
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        ..Default::default()
    };
    send(lv, LVM_SETITEMSTATE, index, &item as *const _ as isize);
    send(lv, LVM_ENSUREVISIBLE, index, 0);
}

fn enable(hwnd: HWND, on: bool) {
    unsafe {
        let _ = EnableWindow(hwnd, on);
    }
}

// ---- refreshing controls from the state ----

fn refresh_homes(s: &mut State, select: Option<usize>) {
    let rows: Vec<Vec<String>> = s
        .work
        .homes
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let name = h.name.clone().unwrap_or_default();
            let name = if i == 0 {
                format!("{name}（プライマリ）")
            } else {
                name
            };
            vec![name, h.path.clone().unwrap_or_default()]
        })
        .collect();
    s.syncing = true;
    lv_fill(s.homes, &rows);
    if let Some(i) = select.filter(|i| *i < rows.len()) {
        lv_select(s.homes, i);
    }
    s.syncing = false;
    update_home_buttons(s);
}

fn update_home_buttons(s: &State) {
    let sel = lv_selected(s.homes);
    let n = s.work.homes.len();
    enable(s.home_buttons[0], sel.is_some());
    enable(s.home_buttons[1], sel.is_some());
    enable(s.home_buttons[2], sel.is_some_and(|i| i > 0));
    enable(s.home_buttons[3], sel.is_some_and(|i| i + 1 < n));
}

fn key_text(s: &State, name: &str) -> String {
    let mut keys: Vec<String> = s.table.keys_of(name).iter().map(format_key).collect();
    if let Some(cmd) = Command::from_name(name) {
        // Character bindings such as ":" are shown but edited in config.toml.
        let (map, _) = KeyMap::new(&s.current().keys);
        keys.extend(map.chars_of(&cmd).into_iter().map(|c| c.to_string()));
    }
    keys.join(", ")
}

fn refresh_commands(s: &mut State) {
    let rows: Vec<Vec<String>> = CATALOG
        .iter()
        .map(|c| vec![c.label.to_string(), key_text(s, c.name)])
        .collect();
    let sel = lv_selected(s.cmds);
    s.syncing = true;
    lv_fill(s.cmds, &rows);
    if let Some(i) = sel {
        lv_select(s.cmds, i);
    }
    s.syncing = false;
    refresh_keys(s);
}

fn selected_command(s: &State) -> Option<&'static str> {
    lv_selected(s.cmds).and_then(|i| CATALOG.get(i)).map(|c| c.name)
}

fn refresh_keys(s: &mut State) {
    let keys: Vec<Vec<String>> = selected_command(s)
        .map(|n| s.table.keys_of(n).iter().map(|k| vec![format_key(k)]).collect())
        .unwrap_or_default();
    s.syncing = true;
    lv_fill(s.keys, &keys);
    s.syncing = false;
    update_key_buttons(s);
}

fn update_key_buttons(s: &State) {
    let cmd = selected_command(s);
    enable(s.key_buttons[0], cmd.is_some() && s.captured.is_some());
    enable(s.key_buttons[1], cmd.is_some() && lv_selected(s.keys).is_some());
    enable(s.key_buttons[2], cmd.is_some_and(|n| !s.table.is_default(n)));
}

fn update_apply(s: &State) {
    enable(s.apply, s.current() != s.baseline);
}

fn show_page(s: &mut State, page: usize) {
    s.page = page;
    for (i, ctrls) in s.pages.iter().enumerate() {
        for &c in ctrls {
            unsafe {
                let _ = ShowWindow(c, if i == page { SW_SHOW } else { SW_HIDE });
            }
        }
    }
}

// ---- actions ----

fn apply_now() {
    let Some((main, u)) = with(|s| (s.main, s.current())) else {
        return;
    };
    if let Some(app) = App::from_hwnd(main) {
        app.apply_user_settings(u.clone());
        app.save_config();
    }
    with(|s| {
        s.table = KeyTable::from_overrides(&u.keys);
        s.baseline = u.clone();
        s.work = u;
        s.applied = true;
        refresh_commands(s);
        update_apply(s);
    });
}

fn edit_home(index: Option<usize>) {
    let Some((hwnd, font, initial)) = with(|s| {
        let initial = index.and_then(|i| s.work.homes.get(i).cloned()).unwrap_or_default();
        (s.hwnd, s.font, initial)
    }) else {
        return;
    };
    // Modal: no state borrow may be held here.
    let Some(spec) = home_entry(hwnd, font, &initial) else {
        return;
    };
    with(|s| {
        let at = match index {
            Some(i) => {
                s.work.homes[i] = spec;
                i
            }
            None => {
                s.work.homes.push(spec);
                s.work.homes.len() - 1
            }
        };
        refresh_homes(s, Some(at));
        update_apply(s);
    });
}

fn home_command(id: i32) {
    match id {
        ID_HOME_ADD => edit_home(None),
        ID_HOME_EDIT => {
            if let Some(i) = with(|s| lv_selected(s.homes)).flatten() {
                edit_home(Some(i));
            }
        }
        _ => {
            with(|s| {
                let Some(i) = lv_selected(s.homes) else { return };
                let select = match id {
                    ID_HOME_DEL => {
                        s.work.homes.remove(i);
                        i.min(s.work.homes.len().saturating_sub(1))
                    }
                    ID_HOME_UP if i > 0 => {
                        s.work.homes.swap(i, i - 1);
                        i - 1
                    }
                    ID_HOME_DOWN if i + 1 < s.work.homes.len() => {
                        s.work.homes.swap(i, i + 1);
                        i + 1
                    }
                    _ => return,
                };
                refresh_homes(s, Some(select));
                update_apply(s);
            });
        }
    }
}

fn confirm(owner: HWND, text: &str) -> bool {
    unsafe {
        MessageBoxW(
            Some(owner),
            &HSTRING::from(text),
            w!("設定"),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

fn key_command(id: i32) {
    match id {
        ID_ASSIGN => {
            let Some((hwnd, name, key, owner)) = with(|s| {
                let name = selected_command(s)?;
                let key = s.captured?;
                Some((s.hwnd, name, key, s.table.owner_of(&key)))
            })
            .flatten() else {
                return;
            };
            if let Some(other) = owner.filter(|o| *o != name) {
                let text = format!(
                    "{} は「{}」に割り当てられています。こちらに割り当て直しますか？",
                    format_key(&key),
                    label_of(other)
                );
                if !confirm(hwnd, &text) {
                    return;
                }
            }
            with(|s| {
                s.table.assign(key, name);
                refresh_commands(s);
                update_apply(s);
            });
        }
        ID_KEY_DEL => {
            with(|s| {
                let (Some(name), Some(i)) = (selected_command(s), lv_selected(s.keys)) else {
                    return;
                };
                if let Some(k) = s.table.keys_of(name).get(i).copied() {
                    s.table.remove(&k);
                }
                refresh_commands(s);
                update_apply(s);
            });
        }
        ID_KEY_RESET => {
            with(|s| {
                if let Some(name) = selected_command(s) {
                    s.table.reset(name);
                }
                refresh_commands(s);
                update_apply(s);
            });
        }
        ID_KEY_RESET_ALL => {
            let Some(hwnd) = with(|s| s.hwnd) else { return };
            if confirm(hwnd, "すべてのキー割り当てを既定に戻しますか？") {
                with(|s| {
                    s.table.reset_all();
                    refresh_commands(s);
                    update_apply(s);
                });
            }
        }
        _ => {}
    }
}

fn read_controls(s: &mut State) {
    let checked = |h: HWND| send(h, BM_GETCHECK, 0, 0).0 == BST_CHECKED.0 as isize;
    s.work.show_hidden = checked(s.show[0]);
    s.work.show_dotfiles = checked(s.show[1]);
    s.work.show_extensions = checked(s.show[2]);
    s.work.nav_tree_visible = checked(s.show[3]);
    s.work.migemo = checked(s.migemo);
    let shell = super::window_text(s.shell).trim().to_string();
    s.work.shell = (!shell.is_empty()).then_some(shell);
}

// ---- key capture field ----

extern "system" fn capture_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = VIRTUAL_KEY(wp.0 as u16);
                let modifier = matches!(
                    vk,
                    VK_SHIFT
                        | VK_CONTROL
                        | VK_MENU
                        | VK_LSHIFT
                        | VK_RSHIFT
                        | VK_LCONTROL
                        | VK_RCONTROL
                        | VK_LMENU
                        | VK_RMENU
                        | VK_LWIN
                        | VK_RWIN
                );
                if !modifier {
                    let down = |k: VIRTUAL_KEY| GetKeyState(k.0 as i32) < 0;
                    let spec = KeySpec {
                        ctrl: down(VK_CONTROL),
                        shift: down(VK_SHIFT),
                        // The context bit of WM_SYSKEYDOWN is set while Alt is held.
                        alt: down(VK_MENU) || (msg == WM_SYSKEYDOWN && lp.0 & (1 << 29) != 0),
                        vk: vk.0,
                    };
                    let text = format_key(&spec);
                    if text.contains("0x") {
                        // Keys config.toml cannot name.
                        let _ = MessageBeep(MB_OK);
                    } else {
                        let _ = SetWindowTextW(hwnd, &HSTRING::from(text));
                        with(|s| {
                            s.captured = Some(spec);
                            update_key_buttons(s);
                        });
                    }
                }
                LRESULT(0)
            }
            // Swallow the rest so Alt never starts menu mode and nothing is typed.
            WM_KEYUP | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR | WM_DEADCHAR | WM_SYSDEADCHAR => LRESULT(0),
            _ => DefSubclassProc(hwnd, msg, wp, lp),
        }
    }
}

/// Keys go straight to the capture field, except plain Tab / Esc / Enter for the dialog.
fn goes_to_capture(msg: &MSG, capture: HWND) -> bool {
    if unsafe { GetFocus() } != capture {
        return false;
    }
    match msg.message {
        WM_KEYDOWN => {
            let vk = VIRTUAL_KEY(msg.wParam.0 as u16);
            let plain = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } >= 0;
            let mods_free = plain(VK_CONTROL) && plain(VK_SHIFT) && plain(VK_MENU);
            !(mods_free && matches!(vk, VK_TAB | VK_ESCAPE | VK_RETURN))
        }
        WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP | WM_SYSCHAR => true,
        _ => false,
    }
}

// ---- dialog ----

/// Modal; returns after the dialog is closed.
pub fn show(app: &mut App) {
    let main = app.hwnd;
    let font = app.font;
    let page = app.settings_page.min(PAGES.len() - 1);
    let baseline = app.config.user_settings();
    app.settings_dialog_open = true;
    let hwnd = unsafe { create(main, font, page, baseline) };
    if let Some(hwnd) = hwnd {
        unsafe { run_modal(main, hwnd) };
    }
    let (applied, page) = STATE
        .with(|s| s.borrow_mut().take())
        .map(|s| {
            unsafe {
                let _ = DeleteObject(s.bold.into());
            }
            let page = s.page;
            (s.applied, page)
        })
        .unwrap_or((false, page));
    let Some(app) = App::from_hwnd(main) else { return };
    app.settings_page = page;
    app.settings_dialog_open = false;
    if app.config_reload_pending {
        app.config_reload_pending = false;
        // An applied dialog has already saved its content over outside edits.
        if !applied {
            app.reload_config();
        }
    }
}

unsafe fn run_modal(main: HWND, hwnd: HWND) {
    unsafe {
        let _ = EnableWindow(main, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut msg = MSG::default();
        loop {
            if with(|s| s.done).unwrap_or(true) {
                break;
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            let capture = with(|s| s.capture).unwrap_or_default();
            if goes_to_capture(&msg, capture) {
                DispatchMessageW(&msg);
                continue;
            }
            if IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = EnableWindow(main, true);
        let _ = DestroyWindow(hwnd);
        let _ = SetForegroundWindow(main);
    }
}

unsafe fn create(main: HWND, font: HFONT, page: usize, baseline: UserSettings) -> Option<HWND> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        let wc = WNDCLASSW {
            hInstance: instance,
            lpszClassName: SETTINGS_CLASS,
            lpfnWndProc: Some(settings_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let dpi = super::dpi::window_dpi(main);
        let s = |v| scale(v, dpi);
        let mut wr = RECT {
            left: 0,
            top: 0,
            right: s(560),
            bottom: s(480),
        };
        let style = WS_POPUP | WS_CAPTION | WS_SYSMENU;
        let ex = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
        let _ = AdjustWindowRectExForDpi(&mut wr, style, false, ex, dpi);
        let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
        let mut mr = RECT::default();
        let _ = GetWindowRect(main, &mut mr);
        let hwnd = CreateWindowExW(
            ex,
            SETTINGS_CLASS,
            w!("設定"),
            style,
            (mr.left + mr.right - w) / 2,
            (mr.top + mr.bottom - h) / 2,
            w,
            h,
            Some(main),
            None,
            Some(instance),
            None,
        )
        .ok()?;
        let mk = |class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: i32| {
            let c = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                s(r.0),
                s(r.1),
                s(r.2),
                s(r.3),
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(instance),
                None,
            )
            .unwrap_or_default();
            SendMessageW(c, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            c
        };
        let none = WINDOW_EX_STYLE(0);
        let tabstop = WS_TABSTOP.0;
        let button = |text: &str, r, id| mk(w!("BUTTON"), text, tabstop | BS_PUSHBUTTON as u32, none, r, id);
        let label = |text: &str, r| mk(w!("STATIC"), text, 0, none, r, 0);
        let list = |r, id| {
            mk(
                WC_LISTVIEWW,
                "",
                tabstop | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOSORTHEADER,
                WS_EX_CLIENTEDGE,
                r,
                id,
            )
        };

        // Page headers only; the pages live on the dialog background below them.
        let tab = mk(WC_TABCONTROLW, "", tabstop, none, (12, 12, 536, 30), ID_TAB);
        for (i, p) in PAGES.iter().enumerate() {
            let mut t: Vec<u16> = p.encode_utf16().chain(std::iter::once(0)).collect();
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: PWSTR(t.as_mut_ptr()),
                ..Default::default()
            };
            send(tab, TCM_INSERTITEMW, i, &item as *const _ as isize);
        }

        // Display.
        let check = |text: &str, y, id| {
            mk(
                w!("BUTTON"),
                text,
                tabstop | BS_AUTOCHECKBOX as u32,
                none,
                (24, y, 400, 24),
                id,
            )
        };
        let show: [HWND; 4] = std::array::from_fn(|i| check(SHOW_LABELS[i], 60 + 32 * i as i32, ID_SHOW[i]));
        let set_check = |h: HWND, on: bool| send(h, BM_SETCHECK, if on { BST_CHECKED.0 } else { 0 } as usize, 0);
        set_check(show[0], baseline.show_hidden);
        set_check(show[1], baseline.show_dotfiles);
        set_check(show[2], baseline.show_extensions);
        set_check(show[3], baseline.nav_tree_visible);
        let page0 = show.to_vec();

        // Home directories.
        let homes = list((24, 60, 400, 280), ID_HOMES);
        lv_columns(homes, &[("名前", 150), ("パス", 230)], dpi);
        let add = button("追加...", (436, 60, 112, 28), ID_HOME_ADD);
        let edit = button("編集...", (436, 96, 112, 28), ID_HOME_EDIT);
        let del = button("削除", (436, 132, 112, 28), ID_HOME_DEL);
        let up = button("上へ", (436, 180, 112, 28), ID_HOME_UP);
        let down = button("下へ", (436, 216, 112, 28), ID_HOME_DOWN);
        let note1 = label(
            "先頭がプライマリ（ホームボタンの左クリックと Alt+Home の移動先）です。\r\n一覧が空のときは、ユーザーフォルダがホームディレクトリになります。",
            (24, 350, 524, 44),
        );
        let page1 = vec![homes, add, edit, del, up, down, note1];

        // Keys.
        let cmds = list((24, 60, 524, 200), ID_CMDS);
        lv_columns(cmds, &[("コマンド", 250), ("キー", 240)], dpi);
        let keys = list((24, 270, 230, 96), ID_KEYS);
        lv_columns(keys, &[("キー", 200)], dpi);
        let cap_label = label("キーの入力:", (266, 270, 120, 20));
        let capture = mk(
            w!("EDIT"),
            "",
            tabstop | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
            WS_EX_CLIENTEDGE,
            (266, 292, 180, 26),
            ID_CAPTURE,
        );
        let _ = SetWindowSubclass(capture, Some(capture_proc), 1, 0);
        let assign = button("割り当て", (454, 291, 94, 28), ID_ASSIGN);
        let key_del = button("削除", (266, 330, 86, 28), ID_KEY_DEL);
        let reset = button("既定に戻す", (360, 330, 120, 28), ID_KEY_RESET);
        let reset_all = button("すべて既定に戻す", (24, 374, 160, 28), ID_KEY_RESET_ALL);
        let note2 = label(
            "「:」のように文字で割り当てるキーは、config.toml で変更します。",
            (192, 380, 356, 40),
        );
        let page2 = vec![cmds, keys, cap_label, capture, assign, key_del, reset, reset_all, note2];

        // Other.
        let shell_label = label("シェル（:sh で起動するコマンド）:", (24, 60, 524, 20));
        let shell = mk(
            w!("EDIT"),
            baseline.shell.as_deref().unwrap_or(""),
            tabstop | ES_AUTOHSCROLL as u32,
            WS_EX_CLIENTEDGE,
            (24, 84, 524, 26),
            ID_SHELL,
        );
        let note3 = label(
            "空欄のときは Windows Terminal → PowerShell 7 → Windows PowerShell → コマンドプロンプトの順に探します。",
            (24, 116, 524, 40),
        );
        let migemo = check("migemo を使う", 172, ID_MIGEMO);
        set_check(migemo, baseline.migemo);
        let note4 = label(
            "tabex.exe と同じフォルダに rustmigemo.dll と辞書があるときに使われます。",
            (24, 200, 524, 40),
        );
        let page3 = vec![shell_label, shell, note3, migemo, note4];

        let ok = mk(
            w!("BUTTON"),
            "OK",
            tabstop | BS_DEFPUSHBUTTON as u32,
            none,
            (268, 440, 86, 28),
            ID_OK,
        );
        let cancel = button("キャンセル", (362, 440, 86, 28), ID_CANCEL);
        let apply = button("適用", (456, 440, 92, 28), ID_APPLY);
        let _ = (ok, cancel);

        // Bold variant of the font for changed key bindings.
        let mut lf = LOGFONTW::default();
        GetObjectW(
            font.into(),
            std::mem::size_of::<LOGFONTW>() as i32,
            Some(&mut lf as *mut _ as *mut _),
        );
        lf.lfWeight = FW_BOLD.0 as i32;
        let bold = CreateFontIndirectW(&lf);

        let state = State {
            hwnd,
            main,
            font,
            bold,
            tab,
            pages: [page0, page1, page2, page3],
            show,
            homes,
            home_buttons: [edit, del, up, down],
            cmds,
            keys,
            capture,
            key_buttons: [assign, key_del, reset],
            shell,
            migemo,
            apply,
            table: KeyTable::from_overrides(&baseline.keys),
            work: baseline.clone(),
            baseline,
            captured: None,
            applied: false,
            page,
            done: false,
            syncing: false,
        };
        STATE.with(|st| *st.borrow_mut() = Some(state));
        with(|s| {
            refresh_homes(s, None);
            refresh_commands(s);
            update_apply(s);
            send(s.tab, TCM_SETCURSEL, page, 0);
            show_page(s, page);
        });
        Some(hwnd)
    }
}

fn on_notify(hdr: &NMHDR, lp: LPARAM) -> LRESULT {
    let id = hdr.idFrom as i32;
    match (id, hdr.code) {
        (ID_TAB, TCN_SELCHANGE) => {
            with(|s| show_page(s, send(s.tab, TCM_GETCURSEL, 0, 0).0.max(0) as usize));
        }
        (ID_HOMES, LVN_ITEMCHANGED) => {
            with(|s| {
                if !s.syncing {
                    update_home_buttons(s);
                }
            });
        }
        (ID_HOMES, NM_DBLCLK) => home_command(ID_HOME_EDIT),
        (ID_HOMES, LVN_KEYDOWN) => {
            let nm = unsafe { &*(lp.0 as *const NMLVKEYDOWN) };
            if nm.wVKey == VK_DELETE.0 {
                home_command(ID_HOME_DEL);
            }
        }
        (ID_CMDS, LVN_ITEMCHANGED) => {
            let nm = unsafe { &*(lp.0 as *const NMLISTVIEW) };
            if nm.uChanged.0 & LVIF_STATE.0 != 0 && (nm.uOldState ^ nm.uNewState) & LVIS_SELECTED.0 != 0 {
                with(|s| {
                    if !s.syncing {
                        refresh_keys(s);
                    }
                });
            }
        }
        (ID_KEYS, LVN_ITEMCHANGED) => {
            with(|s| {
                if !s.syncing {
                    update_key_buttons(s);
                }
            });
        }
        (ID_CMDS, NM_CUSTOMDRAW) => {
            let cd = unsafe { &mut *(lp.0 as *mut NMLVCUSTOMDRAW) };
            let stage = cd.nmcd.dwDrawStage;
            if stage == CDDS_PREPAINT {
                return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
            }
            if stage == CDDS_ITEMPREPAINT {
                return LRESULT(CDRF_NOTIFYSUBITEMDRAW as isize);
            }
            if stage == NMCUSTOMDRAW_DRAW_STAGE(CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0) {
                let changed = with(|s| {
                    let name = CATALOG.get(cd.nmcd.dwItemSpec).map(|c| c.name);
                    let changed = cd.iSubItem == 1 && name.is_some_and(|n| !s.table.is_default(n));
                    (changed, s.bold, s.font)
                });
                if let Some((changed, bold, font)) = changed {
                    unsafe {
                        SelectObject(cd.nmcd.hdc, if changed { bold } else { font }.into());
                    }
                    return LRESULT(CDRF_NEWFONT as isize);
                }
            }
        }
        _ => {}
    }
    LRESULT(0)
}

fn close(ok: bool) {
    if ok && with(|s| s.current() != s.baseline).unwrap_or(false) {
        apply_now();
    }
    with(|s| s.done = true);
}

extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = super::loword(wp.0) as i32;
                let code = super::hiword(wp.0) as u32;
                match id {
                    ID_OK => close(true),
                    ID_CANCEL => close(false),
                    ID_APPLY => apply_now(),
                    ID_HOME_ADD | ID_HOME_EDIT | ID_HOME_DEL | ID_HOME_UP | ID_HOME_DOWN => home_command(id),
                    ID_ASSIGN | ID_KEY_DEL | ID_KEY_RESET | ID_KEY_RESET_ALL => key_command(id),
                    _ if (ID_SHOW.contains(&id) || id == ID_MIGEMO) && code == BN_CLICKED
                        || id == ID_SHELL && code == EN_CHANGE =>
                    {
                        with(|s| {
                            if !s.syncing {
                                read_controls(s);
                                update_apply(s);
                            }
                        });
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_NOTIFY => on_notify(&*(lp.0 as *const NMHDR), lp),
            WM_CLOSE => {
                close(false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

// ---- home entry dialog ----

struct EntryState {
    name: HWND,
    path: HWND,
    error: HWND,
    result: Option<Option<HomeSpec>>,
}

thread_local! {
    static ENTRY: RefCell<Option<EntryState>> = const { RefCell::new(None) };
}

/// Folder picker; the chosen file system path.
fn browse_folder(owner: HWND) -> Option<String> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().ok()?;
        dlg.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        dlg.Show(Some(owner)).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn finish_entry(ok: bool) {
    let Some((name, path, error)) = ENTRY.with(|e| e.borrow().as_ref().map(|e| (e.name, e.path, e.error))) else {
        return;
    };
    let result = if ok {
        let path_text = super::window_text(path).trim().to_string();
        let expanded = crate::shell::names::expand_env(&path_text);
        if path_text.is_empty() || !is_absolute(expanded.trim()) {
            unsafe {
                let _ = SetWindowTextW(error, w!("パスには絶対パスを指定してください"));
                let _ = MessageBeep(MB_ICONWARNING);
                let _ = SetFocus(Some(path));
            }
            return;
        }
        let name_text = super::window_text(name).trim().to_string();
        Some(HomeSpec {
            name: (!name_text.is_empty()).then_some(name_text),
            path: Some(path_text),
        })
    } else {
        None
    };
    ENTRY.with(|e| {
        if let Some(e) = e.borrow_mut().as_mut() {
            e.result = Some(result);
        }
    });
}

extern "system" fn entry_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                match super::loword(wp.0) as i32 {
                    ID_OK => finish_entry(true),
                    ID_CANCEL => finish_entry(false),
                    ID_BROWSE => {
                        let path = ENTRY.with(|e| e.borrow().as_ref().map(|e| e.path));
                        if let (Some(path), Some(dir)) = (path, browse_folder(hwnd)) {
                            let _ = SetWindowTextW(path, &HSTRING::from(dir));
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                let is_error = ENTRY.with(|e| e.borrow().as_ref().is_some_and(|e| e.error.0 == lp.0 as *mut _));
                let hdc = HDC(wp.0 as *mut _);
                if is_error {
                    SetTextColor(hdc, COLORREF(0x0000_20C0));
                }
                SetBkMode(hdc, TRANSPARENT);
                LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
            }
            WM_CLOSE => {
                finish_entry(false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

const ID_ENTRY_NAME: i32 = 100;
const ID_ENTRY_PATH: i32 = 101;
const ID_BROWSE: i32 = 102;

/// Small modal dialog for one home directory; None when cancelled.
fn home_entry(owner: HWND, font: HFONT, initial: &HomeSpec) -> Option<HomeSpec> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        let wc = WNDCLASSW {
            hInstance: instance,
            lpszClassName: ENTRY_CLASS,
            lpfnWndProc: Some(entry_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let dpi = super::dpi::window_dpi(owner);
        let s = |v| scale(v, dpi);
        let mut rc = RECT::default();
        let _ = GetWindowRect(owner, &mut rc);
        let (w, h) = (s(480), s(250));
        let title = if initial.path.is_some() {
            "ホームディレクトリの編集"
        } else {
            "ホームディレクトリの追加"
        };
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
            ENTRY_CLASS,
            &HSTRING::from(title),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            (rc.left + rc.right - w) / 2,
            (rc.top + rc.bottom - h) / 2,
            w,
            h,
            Some(owner),
            None,
            Some(instance),
            None,
        )
        .ok()?;
        let mk = |class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: i32| {
            let c = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                s(r.0),
                s(r.1),
                s(r.2),
                s(r.3),
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(instance),
                None,
            )
            .unwrap_or_default();
            SendMessageW(c, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            c
        };
        let none = WINDOW_EX_STYLE(0);
        let tabstop = WS_TABSTOP.0;
        let edit_style = tabstop | ES_AUTOHSCROLL as u32;
        mk(w!("STATIC"), "名前（省略可）:", 0, none, (12, 12, 440, 20), 0);
        let name = mk(
            w!("EDIT"),
            initial.name.as_deref().unwrap_or(""),
            edit_style,
            WS_EX_CLIENTEDGE,
            (12, 34, 440, 26),
            ID_ENTRY_NAME,
        );
        mk(w!("STATIC"), "パス:", 0, none, (12, 70, 440, 20), 0);
        let path = mk(
            w!("EDIT"),
            initial.path.as_deref().unwrap_or(""),
            edit_style,
            WS_EX_CLIENTEDGE,
            (12, 92, 344, 26),
            ID_ENTRY_PATH,
        );
        mk(
            w!("BUTTON"),
            "参照...",
            tabstop | BS_PUSHBUTTON as u32,
            none,
            (364, 91, 88, 28),
            ID_BROWSE,
        );
        let error = mk(w!("STATIC"), "", 0, none, (12, 126, 440, 20), 0);
        mk(
            w!("BUTTON"),
            "OK",
            tabstop | BS_DEFPUSHBUTTON as u32,
            none,
            (272, 164, 86, 28),
            ID_OK,
        );
        mk(
            w!("BUTTON"),
            "キャンセル",
            tabstop | BS_PUSHBUTTON as u32,
            none,
            (366, 164, 86, 28),
            ID_CANCEL,
        );
        ENTRY.with(|e| {
            *e.borrow_mut() = Some(EntryState {
                name,
                path,
                error,
                result: None,
            })
        });
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(Some(if initial.path.is_some() { path } else { name }));
        let mut msg = MSG::default();
        loop {
            if ENTRY.with(|e| e.borrow().as_ref().is_none_or(|e| e.result.is_some())) {
                break;
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            if IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(hwnd);
        let _ = SetForegroundWindow(owner);
        ENTRY.with(|e| e.borrow_mut().take()).and_then(|e| e.result.flatten())
    }
}
