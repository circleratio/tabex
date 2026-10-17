//! File operations, clipboard, drag and drop, context menus and thumbnails.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::*;
use crate::model::command::Command;
use crate::shell::clipboard;
use crate::shell::context_menu::{self, Chosen};
use crate::shell::dragdrop::{DropHost, DropTarget};
use crate::shell::fileop::{self, Items, Op, OpResult};
use crate::shell::thumbs::{self, Lookup};
use crate::shell::undo::MAX_UNDO;
use crate::ui::listview;

/// What to do on the UI thread when a file operation finishes.
pub enum AfterOp {
    Nothing,
    /// Select this entry in the tab once it appears.
    Select(TabId, Box<[u16]>),
    /// Select and start renaming a newly created folder.
    RenameNew(TabId, Box<[u16]>),
    /// A cut + paste finished: notify the source data object.
    CutPasteDone(AgileReference<IDataObject>),
    Undo,
}

const INVALID_NAME_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
const TAB_HOVER_DELAY: Duration = Duration::from_millis(500);

impl App {
    /// The active tab's folder, if it is a file system folder that accepts file operations.
    fn active_fs_dir(&self) -> Option<(TabId, Location)> {
        let tab = self.ws.active_tab();
        tab.location.is_file_system().then(|| (tab.id, tab.location.clone()))
    }

    /// Names (file system) or raw PIDLs (virtual folders) of the selection in `side`.
    fn selected_parts(&self, side: PaneSide) -> (Location, Vec<String>, Vec<Box<[u8]>>) {
        let tab = self.ws.pane(side).active();
        let mut names = Vec::new();
        let mut raw = Vec::new();
        if let Some(l) = self.rt.get(&tab.id).and_then(|r| r.listing.as_ref()) {
            for e in l.selected_entries() {
                match &e.shell_id {
                    Some(id) => raw.push(id.clone()),
                    None => names.push(e.name_string()),
                }
            }
        }
        (tab.location.clone(), names, raw)
    }

    fn selected_paths(&self, side: PaneSide) -> Vec<String> {
        self.selection(side).into_iter().map(|(_, loc)| loc.0).collect()
    }

    pub fn run_fileop(&mut self, op: Op, after: AfterOp) {
        #[derive(Clone, Copy)]
        struct H(HWND);
        unsafe impl Send for H {}
        let owner = H(self.hwnd);
        let n = self.notifier.clone();
        self.fileop_pool.spawn(move || {
            let owner = owner;
            let result = fileop::run(owner.0, op);
            n.send(WorkerMsg::FileOp { result, after });
        });
    }

    pub fn on_fileop_done(&mut self, result: OpResult, after: AfterOp) {
        if let Some(e) = &result.error {
            self.show_status(&format!("操作を完了できませんでした: {e}"));
        }
        match after {
            AfterOp::Undo => {
                if result.error.is_none() {
                    self.show_status("元に戻しました");
                }
            }
            other => {
                if let Some(u) = result.undo {
                    self.undo_stack.push(u);
                    if self.undo_stack.len() > MAX_UNDO {
                        self.undo_stack.remove(0);
                    }
                }
                match other {
                    AfterOp::Select(tab, name) => {
                        if let Some(rt) = self.rt.get_mut(&tab) {
                            rt.select_on_load = Some(name);
                        }
                        self.after_listing_change(tab, false);
                    }
                    AfterOp::RenameNew(tab, name) => {
                        self.pending_rename = Some((tab, name));
                        self.try_pending_rename();
                    }
                    AfterOp::CutPasteDone(obj) => {
                        if result.error.is_none()
                            && !result.aborted
                            && let Ok(o) = obj.resolve()
                        {
                            clipboard::finish_cut_paste(&o);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Virtual folders are not watched: refresh what is on screen.
        for side in PaneSide::BOTH {
            if let Some(id) = self.panes[side.index()].shown
                && self.tab(id).is_some_and(|t| !t.location.is_file_system())
            {
                self.start_enum(id, true);
            }
        }
    }

    /// Start renaming the folder created by "new folder" once it shows up in the list.
    pub fn try_pending_rename(&mut self) {
        let Some((tab, name)) = self.pending_rename.clone() else {
            return;
        };
        let Some(side) = self.shown_side(tab) else { return };
        let list = self.panes[side.index()].list;
        let Some(l) = self.rt.get_mut(&tab).and_then(|r| r.listing.as_mut()) else {
            return;
        };
        let Some(pos) = l.select_only_name(&name) else { return };
        self.pending_rename = None;
        self.suppress_selection = true;
        listview::apply_selection(list, l);
        listview::set_focused(list, pos);
        self.suppress_selection = false;
        unsafe {
            let _ = SetFocus(Some(list));
        }
        self.start_label_edit(list, pos);
    }

    fn start_label_edit(&mut self, list: HWND, pos: usize) {
        self.editing_label = true;
        let edit = listview::edit_label(list, pos);
        if edit.0.is_null() {
            self.editing_label = false;
            return;
        }
        // Select the name without its extension, like Explorer.
        let text = ui::window_text(edit);
        let is_dir = self.shown_listing_entry(list, pos).is_some_and(|e| e.is_dir());
        let end = match text.rfind('.') {
            Some(i) if i > 0 && !is_dir => text[..i].encode_utf16().count(),
            _ => text.encode_utf16().count(),
        };
        unsafe {
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(end as isize)));
        }
    }

    fn shown_listing_entry(&self, list: HWND, pos: usize) -> Option<&crate::model::listing::Entry> {
        let side = self.side_of_list(list)?;
        let id = self.panes[side.index()].shown?;
        self.rt.get(&id)?.listing.as_ref()?.get(pos)
    }

    pub fn begin_rename(&mut self) {
        let side = self.ws.active_pane;
        let list = self.panes[side.index()].list;
        if self.active_fs_dir().is_none() {
            return;
        }
        let pos = listview::focused(list).filter(|p| {
            self.rt
                .get(&self.ws.active_tab().id)
                .and_then(|r| r.listing.as_ref())
                .is_some_and(|l| l.is_selected(*p))
        });
        let pos = pos.or_else(|| {
            self.rt
                .get(&self.ws.active_tab().id)
                .and_then(|r| r.listing.as_ref())
                .and_then(|l| l.selected_positions().first().copied())
        });
        if let Some(pos) = pos {
            self.start_label_edit(list, pos);
        }
    }

    pub fn finish_rename(&mut self, side: PaneSide, pos: usize, text: String) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(tab) = self.tab(id) else { return };
        let dir = tab.location.clone();
        let Some(old) = self
            .rt
            .get(&id)
            .and_then(|r| r.listing.as_ref())
            .and_then(|l| l.get(pos))
            .map(|e| e.name_string())
        else {
            return;
        };
        let new = text.trim().to_string();
        if new.is_empty() || new == old {
            return;
        }
        if let Some(c) = new.chars().find(|c| INVALID_NAME_CHARS.contains(c)) {
            self.show_status(&format!("ファイル名に次の文字は使えません: {c}"));
            return;
        }
        // With hidden extensions the edit shows the bare name; keep the extension.
        let new = if !self.config.show_extensions {
            match old.rfind('.') {
                Some(i) if i > 0 && !new.contains('.') => format!("{new}{}", &old[i..]),
                _ => new,
            }
        } else {
            new
        };
        let path = crate::fs::join(dir.as_str(), &old);
        let name: Box<[u16]> = new.encode_utf16().collect::<Vec<u16>>().into();
        self.run_fileop(Op::Rename { path, new_name: new }, AfterOp::Select(id, name));
    }

    pub fn new_folder(&mut self) {
        let Some((id, dir)) = self.active_fs_dir() else { return };
        let base = "新しいフォルダー";
        let mut name = base.to_string();
        let mut n = 2;
        while std::path::Path::new(&crate::fs::join(dir.as_str(), &name)).exists() {
            name = format!("{base} ({n})");
            n += 1;
        }
        let wide: Box<[u16]> = name.encode_utf16().collect::<Vec<u16>>().into();
        self.run_fileop(Op::NewFolder { dest: dir.0, name }, AfterOp::RenameNew(id, wide));
    }

    pub fn delete_selection(&mut self, permanent: bool) {
        let paths = self.selected_paths(self.ws.active_pane);
        if paths.is_empty() {
            return;
        }
        self.run_fileop(Op::Delete { paths, permanent }, AfterOp::Nothing);
    }

    fn data_object(&self, side: PaneSide) -> Option<IDataObject> {
        let (loc, names, raw) = self.selected_parts(side);
        if names.is_empty() && raw.is_empty() {
            return None;
        }
        let sel = clipboard::selection(loc.as_str(), &names, &raw).ok()?;
        clipboard::data_object(&sel, self.hwnd).ok()
    }

    pub fn clipboard_copy(&mut self, cut: bool) {
        let Some(obj) = self.data_object(self.ws.active_pane) else {
            return;
        };
        match clipboard::set_clipboard(&obj, cut) {
            Ok(()) => {
                self.clip = Some(obj);
                let n = self
                    .rt
                    .get(&self.ws.active_tab().id)
                    .and_then(|r| r.listing.as_ref())
                    .map(|l| l.selection_count())
                    .unwrap_or(0);
                self.show_status(&format!(
                    "{n} 個の項目を{}",
                    if cut {
                        "切り取りました"
                    } else {
                        "コピーしました"
                    }
                ));
            }
            Err(e) => self.show_status(&format!("クリップボードを使えません: {}", e.message())),
        }
    }

    pub fn clipboard_paste(&mut self) {
        let Some((_, dir)) = self.active_fs_dir() else { return };
        let Some((obj, is_move)) = clipboard::clipboard() else {
            return;
        };
        let Ok(agile) = AgileReference::new(&obj) else { return };
        if is_move {
            let Ok(again) = AgileReference::new(&obj) else { return };
            self.run_fileop(
                Op::Move {
                    items: Items::Data(agile),
                    dest: dir.0,
                },
                AfterOp::CutPasteDone(again),
            );
        } else {
            self.run_fileop(
                Op::Copy {
                    items: Items::Data(agile),
                    dest: dir.0,
                },
                AfterOp::Nothing,
            );
        }
    }

    pub fn undo(&mut self) {
        let Some(u) = self.undo_stack.pop() else {
            self.show_status("元に戻す操作はありません");
            return;
        };
        self.show_status(&format!("元に戻しています: {}", u.describe()));
        self.run_fileop(fileop::inverse(&u), AfterOp::Undo);
    }

    pub fn properties(&mut self) {
        let side = self.ws.active_pane;
        if let Some(obj) = self.data_object(side) {
            unsafe {
                let _ = SHMultiFileProperties(&obj, 0);
            }
            return;
        }
        let loc = self.ws.active_tab().location.clone();
        unsafe {
            let _ = SHObjectProperties(
                Some(self.hwnd),
                SHOP_FILEPATH,
                &HSTRING::from(loc.as_str()),
                PCWSTR::null(),
            );
        }
    }

    pub fn context_menu(&mut self, side: PaneSide, on_item: bool) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        self.context_menu_at(side, on_item, pt.x, pt.y);
    }

    /// Shift+F10 / menu key: open at the focused item.
    pub fn keyboard_context_menu(&mut self, side: PaneSide) {
        let list = self.panes[side.index()].list;
        let has_sel = self
            .rt
            .get(&self.ws.pane(side).active().id)
            .and_then(|r| r.listing.as_ref())
            .is_some_and(|l| l.selection_count() > 0);
        let mut pt = POINT::default();
        if let Some(pos) = listview::focused(list) {
            let mut rc = RECT {
                left: LVIR_LABEL as i32,
                ..Default::default()
            };
            unsafe {
                SendMessageW(
                    list,
                    LVM_GETITEMRECT,
                    Some(WPARAM(pos)),
                    Some(LPARAM(&mut rc as *mut _ as isize)),
                );
            }
            pt = POINT {
                x: rc.left + 8,
                y: rc.bottom,
            };
        }
        unsafe {
            let _ = ClientToScreen(list, &mut pt);
        }
        self.context_menu_at(side, has_sel, pt.x, pt.y);
    }

    fn context_menu_at(&mut self, side: PaneSide, on_item: bool, x: i32, y: i32) {
        let (loc, names, raw) = if on_item {
            self.selected_parts(side)
        } else {
            (self.ws.pane(side).active().location.clone(), Vec::new(), Vec::new())
        };
        let Ok(sel) = clipboard::selection(loc.as_str(), &names, &raw) else {
            return;
        };
        let dir = loc.is_file_system().then(|| loc.0.clone());
        match context_menu::show(self.hwnd, &sel, dir.as_deref(), x, y) {
            Ok(Chosen::Verb(v)) => {
                let cmd = match v.as_str() {
                    "rename" => Command::Rename,
                    "delete" => Command::Delete {
                        permanent: unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) } < 0,
                    },
                    "copy" => Command::Copy,
                    "cut" => Command::Cut,
                    "paste" => Command::Paste,
                    _ => Command::NewFolder,
                };
                self.execute(cmd);
            }
            Ok(Chosen::None) => {}
            Err(e) => self.show_status(&format!("メニューを表示できません: {}", e.message())),
        }
    }

    pub fn begin_drag(&mut self, side: PaneSide) {
        let Some(obj) = self.data_object(side) else { return };
        unsafe {
            use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
            let _ = SHDoDragDrop(
                Some(self.hwnd),
                &obj,
                None,
                DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0 | DROPEFFECT_LINK.0),
            );
        }
    }

    pub fn register_drop_targets(&mut self) {
        let mut targets = Vec::new();
        let mut hwnds: Vec<HWND> = self.panes.iter().flat_map(|p| [p.list, p.tabbar]).collect();
        hwnds.push(self.nav.hwnd);
        for h in hwnds {
            if let Ok(t) = DropTarget::register(h, Box::new(AppDrop { main: self.hwnd })) {
                targets.push(t);
            }
        }
        self.drop_targets = targets;
    }

    /// Thumbnail handling for icon views. Returns the bitmap if cached.
    pub fn thumbnail_for(
        &mut self,
        id: TabId,
        generation: u64,
        dir: &str,
        e: &crate::model::listing::Entry,
        px: i32,
    ) -> Option<HBITMAP> {
        if e.is_dir() || e.shell_id.is_some() {
            return None;
        }
        let path = crate::fs::join(dir, &e.name_string());
        let key = format!("{path}|{}|{px}", e.modified);
        match self.thumbs.lookup(&key) {
            Lookup::Hit(b) => Some(b),
            Lookup::Skip => None,
            Lookup::Request => {
                let n = self.notifier.clone();
                let name = e.name.clone();
                self.icon_pool.spawn(move || {
                    let bmp = thumbs::fetch(&path, px).map(|b| b.0 as isize);
                    n.send(WorkerMsg::Thumb {
                        tab: id,
                        generation,
                        name,
                        key,
                        bmp,
                    });
                });
                None
            }
        }
    }

    pub fn on_thumb(&mut self, id: TabId, generation: u64, name: &[u16], key: String, bmp: Option<isize>) {
        self.thumbs.insert(key, bmp.map(|b| HBITMAP(b as *mut _)));
        if bmp.is_some() {
            self.on_icon(id, generation, name, crate::model::listing::THUMB_ICON);
        }
    }

    /// NM_CUSTOMDRAW for the file list: paints thumbnails in icon views.
    pub fn custom_draw(&mut self, side: PaneSide, cd: &mut NMLVCUSTOMDRAW) -> LRESULT {
        let stage = cd.nmcd.dwDrawStage;
        let Some(id) = self.panes[side.index()].shown else {
            return LRESULT(CDRF_DODEFAULT as isize);
        };
        let Some(tab) = self.tab(id) else {
            return LRESULT(CDRF_DODEFAULT as isize);
        };
        if !matches!(tab.view.mode, ViewMode::IconsLarge | ViewMode::IconsMedium) {
            return LRESULT(CDRF_DODEFAULT as isize);
        }
        if stage == CDDS_PREPAINT {
            return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
        }
        if stage == CDDS_ITEMPREPAINT {
            return LRESULT(CDRF_NOTIFYPOSTPAINT as isize);
        }
        if stage == CDDS_ITEMPOSTPAINT {
            let pos = cd.nmcd.dwItemSpec;
            let px = thumb_px(tab.view.mode);
            let dir = tab.location.0.clone();
            let entry = self
                .rt
                .get(&id)
                .and_then(|r| r.listing.as_ref())
                .and_then(|l| l.get(pos))
                .cloned();
            if let Some(e) = entry {
                let generation = self.rt.get(&id).map(|r| r.generation).unwrap_or(0);
                if let Some(b) = self.thumbnail_for(id, generation, &dir, &e, px) {
                    let list = self.panes[side.index()].list;
                    let mut rc = RECT {
                        left: LVIR_ICON as i32,
                        ..Default::default()
                    };
                    unsafe {
                        SendMessageW(
                            list,
                            LVM_GETITEMRECT,
                            Some(WPARAM(pos)),
                            Some(LPARAM(&mut rc as *mut _ as isize)),
                        );
                    }
                    thumbs::draw(cd.nmcd.hdc, b, rc);
                }
            }
        }
        LRESULT(CDRF_DODEFAULT as isize)
    }

    /// Track hovering over tabs during a drag; switch tab after a short delay.
    fn drag_hover(&mut self, sx: i32, sy: i32) {
        let hit = ui::tabbar::tab_at_screen(self, sx, sy);
        match (hit, self.drag_hover) {
            (Some(t), Some((prev, since))) if t == prev => {
                if since.elapsed() >= TAB_HOVER_DELAY && self.ws.pane(t.0).active_tab != t.1 {
                    self.execute(Command::ActivateTab(t.0, t.1));
                }
            }
            (Some(t), _) => self.drag_hover = Some((t, Instant::now())),
            (None, _) => self.drag_hover = None,
        }
    }
}

pub fn thumb_px(mode: ViewMode) -> i32 {
    match mode {
        ViewMode::IconsLarge => 256,
        _ => 48,
    }
}

struct AppDrop {
    main: HWND,
}

impl DropHost for AppDrop {
    fn drop_folder(&self, target: HWND, x: i32, y: i32) -> Option<String> {
        let app = App::from_hwnd(self.main)?;
        if let Some(side) = app.side_of_list(target) {
            let id = app.panes[side.index()].shown?;
            let tab = app.tab(id)?;
            if !tab.location.is_file_system() {
                return None;
            }
            let mut pt = POINT { x, y };
            unsafe {
                let _ = ScreenToClient(target, &mut pt);
            }
            let mut hit = LVHITTESTINFO {
                pt,
                ..Default::default()
            };
            let idx = unsafe { SendMessageW(target, LVM_HITTEST, None, Some(LPARAM(&mut hit as *mut _ as isize))) }.0;
            if idx >= 0
                && let Some(e) = app
                    .rt
                    .get(&id)
                    .and_then(|r| r.listing.as_ref())
                    .and_then(|l| l.get(idx as usize))
                && e.is_dir()
            {
                return Some(crate::fs::join(tab.location.as_str(), &e.name_string()));
            }
            return Some(tab.location.0.clone());
        }
        if let Some(side) = PaneSide::BOTH
            .into_iter()
            .find(|s| app.panes[s.index()].tabbar == target)
        {
            let tab = match ui::tabbar::tab_at_screen(app, x, y) {
                Some((s, i)) => &app.ws.pane(s).tabs[i],
                None => app.ws.pane(side).active(),
            };
            return tab.location.is_file_system().then(|| tab.location.0.clone());
        }
        if target == app.nav.hwnd {
            let mut pt = POINT { x, y };
            unsafe {
                let _ = ScreenToClient(target, &mut pt);
            }
            let loc = app.nav.location_at(pt.x, pt.y)?;
            return loc.is_file_system().then_some(loc.0);
        }
        None
    }

    fn hover(&self, target: HWND, x: i32, y: i32, _since: Instant) {
        if let Some(app) = App::from_hwnd(self.main)
            && app.panes.iter().any(|p| p.tabbar == target)
        {
            app.drag_hover(x, y);
        }
    }

    fn drop_items(&self, data: &IDataObject, dest: &str, effect: u32) {
        let Some(app) = App::from_hwnd(self.main) else { return };
        let Ok(agile) = AgileReference::new(data) else { return };
        let items = Items::Data(agile);
        let op = if effect & clipboard::DROPEFFECT_MOVE_ != 0 {
            Op::Move {
                items,
                dest: dest.to_string(),
            }
        } else {
            Op::Copy {
                items,
                dest: dest.to_string(),
            }
        };
        app.run_fileop(op, AfterOp::Nothing);
    }
}
