//! Binding tabs to the pane controls, enumeration results, change notifications and list
//! view notifications.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::*;
use crate::fs::enumerate::{EnumRequest, enumerate};
use crate::fs::watch::Change;
use crate::model::command::Command;
use crate::model::listing::{Entry, Filter};
use crate::shell::names;
use crate::ui::listview;

pub fn create_pane_ui(main: HWND, side: PaneSide, font: HFONT) -> Result<PaneUi> {
    let i = side.index();
    unsafe {
        let tabbar = ui::tabbar::create(main, side)?;
        let (address, edit) = ui::addressbar::create(main, side, ID_EDIT[i], font)?;
        let list = listview::create(main, ID_LIST[i])?;
        let (filter_bar, filter_edit) = ui::filterbar::create(main, side, ID_FILTER_EDIT[i], font)?;
        let mk = |class: PCWSTR, text: PCWSTR, style: u32, id: i32| {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                text,
                WS_CHILD | WS_CLIPSIBLINGS | WINDOW_STYLE(style),
                0,
                0,
                0,
                0,
                Some(main),
                Some(HMENU(id as usize as *mut _)),
                None,
                None,
            )
        };
        let msg = mk(
            w!("STATIC"),
            w!(""),
            windows::Win32::System::SystemServices::SS_CENTER.0,
            0,
        )?;
        let retry = mk(
            w!("BUTTON"),
            w!("再試行"),
            (BS_PUSHBUTTON | WS_TABSTOP.0 as i32) as u32,
            ID_RETRY[i],
        )?;
        let parent_btn = mk(
            w!("BUTTON"),
            w!("親フォルダへ"),
            (BS_PUSHBUTTON | WS_TABSTOP.0 as i32) as u32,
            ID_PARENT[i],
        )?;
        Ok(PaneUi {
            tabbar,
            address,
            edit,
            list,
            msg,
            retry,
            parent_btn,
            filter_bar,
            filter_edit,
            shown: None,
            rects: None,
            addr_error: false,
        })
    }
}

/// Explorer's "This PC".
pub const THIS_PC: &str = "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}";

impl App {
    pub fn filter(&self) -> Filter {
        Filter {
            show_hidden: self.config.show_hidden,
            show_dotfiles: self.config.show_dotfiles,
        }
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        let (s, i) = self.ws.find_tab(id)?;
        Some(&self.ws.pane(s).tabs[i])
    }

    pub fn side_of_list(&self, h: HWND) -> Option<PaneSide> {
        PaneSide::BOTH.into_iter().find(|s| self.panes[s.index()].list == h)
    }

    /// Tab that should be displayed in each pane right now.
    fn wanted_tab(&self, side: PaneSide) -> Option<TabId> {
        match self.ws.layout {
            Layout::Single if side != self.ws.active_pane => None,
            _ => Some(self.ws.pane(side).active().id),
        }
    }

    /// Bring every pane in line with the workspace state. Cheap when nothing changed.
    pub fn refresh_panes(&mut self) {
        // Drop runtime state of tabs that no longer exist.
        let alive: Vec<TabId> = self.ws.panes.iter().flat_map(|p| p.tabs.iter().map(|t| t.id)).collect();
        self.rt.retain(|id, rt| {
            let keep = alive.contains(id);
            if !keep {
                rt.cancel.store(true, Ordering::Relaxed);
            }
            keep
        });
        for side in PaneSide::BOTH {
            let want = self.wanted_tab(side);
            if self.panes[side.index()].shown != want {
                self.unbind(side);
                if let Some(id) = want {
                    self.bind(side, id);
                }
            }
        }
        self.trim_listings();
        self.invalidate_chrome();
        self.update_status();
        self.update_title();
    }

    /// Save the control state of the tab currently shown in `side` back into the model.
    pub fn capture_view(&mut self, side: PaneSide) {
        let p = &self.panes[side.index()];
        let Some(id) = p.shown else { return };
        let list = p.list;
        let dpi = self.dpi;
        let top = listview::top_index(list);
        if let Some(rt) = self.rt.get_mut(&id) {
            rt.top_index = top;
        }
        if let Some(tab) = self.ws.tab_mut(id)
            && tab.view.mode == ViewMode::Details
        {
            listview::read_columns(list, &mut tab.view, dpi);
        }
    }

    fn unbind(&mut self, side: PaneSide) {
        self.capture_view(side);
        let p = &mut self.panes[side.index()];
        if let Some(id) = p.shown.take() {
            if let Some(w) = &self.watcher {
                w.unwatch(id);
            }
            // Hidden tabs may change unnoticed; refresh when shown again.
            if let Some(l) = self.rt.get_mut(&id).and_then(|r| r.listing.as_mut()) {
                l.stale = true;
            }
        }
        listview::set_count(p.list, 0);
        self.command_line[side.index()] = false;
        self.sync_filter_bar(side);
    }

    fn bind(&mut self, side: PaneSide, id: TabId) {
        let Some(tab) = self.tab(id).cloned() else { return };
        let list = self.panes[side.index()].list;
        self.panes[side.index()].shown = Some(id);
        listview::set_view_mode(list, tab.view.mode, &self.lists);
        listview::set_columns(list, &tab.view, self.dpi);
        self.ensure_rt(id);
        let needs_enum = match self.rt[&id].listing.as_ref() {
            None => true,
            Some(l) => l.stale || !l.complete,
        };
        if let Some(rt) = self.rt.get(&id)
            && let Some(l) = &rt.listing
        {
            self.suppress_selection = true;
            listview::set_count(list, l.len());
            listview::apply_selection(list, l);
            listview::scroll_to_top_index(list, rt.top_index, l.len());
            self.suppress_selection = false;
        }
        if needs_enum {
            let keep = self.rt[&id].listing.as_ref().is_some_and(|l| l.complete);
            self.start_enum(id, keep);
        }
        self.watch(side);
        self.place_placeholder(side);
        self.command_line[side.index()] = false;
        self.sync_filter_bar(side);
    }

    fn watch(&self, side: PaneSide) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        if let (Some(w), Some(tab)) = (&self.watcher, self.tab(id)) {
            if tab.location.is_file_system() {
                w.watch(id, tab.location.as_str());
            } else {
                w.unwatch(id);
            }
        }
    }

    /// Keep listings only for shown tabs and the two most recently hidden ones.
    fn trim_listings(&mut self) {
        let shown: Vec<TabId> = self.panes.iter().filter_map(|p| p.shown).collect();
        let mut hidden: Vec<(usize, TabId)> = Vec::new();
        for (id, rt) in &self.rt {
            if !shown.contains(id) && rt.listing.is_some() {
                hidden.push((rt.top_index, *id));
            }
        }
        if hidden.len() > 2 {
            // Order by generation: older enumerations are dropped first.
            let mut by_gen: Vec<(u64, TabId)> = hidden.iter().map(|(_, id)| (self.rt[id].generation, *id)).collect();
            by_gen.sort();
            for (_, id) in &by_gen[..by_gen.len() - 2] {
                if let Some(rt) = self.rt.get_mut(id) {
                    rt.listing = None;
                    rt.pending = None;
                    rt.cancel.store(true, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn ensure_rt(&mut self, id: TabId) {
        if self.rt.contains_key(&id) {
            return;
        }
        let Some(tab) = self.tab(id) else { return };
        let loc = tab.location.clone();
        let rt = TabRt {
            title: names::display_name(&loc),
            folder_pidl: if loc.is_file_system() {
                None
            } else {
                Pidl::parse(loc.as_str()).ok()
            },
            ..Default::default()
        };
        self.rt.insert(id, rt);
    }

    pub fn shown_side(&self, id: TabId) -> Option<PaneSide> {
        PaneSide::BOTH
            .into_iter()
            .find(|s| self.panes[s.index()].shown == Some(id))
    }

    /// Start (re-)enumerating a tab. With `keep_old`, the current listing stays visible
    /// until the new one is complete.
    pub fn start_enum(&mut self, id: TabId, keep_old: bool) {
        let Some(tab) = self.tab(id) else { return };
        let location = tab.location.clone();
        let sort = tab.view.sort;
        let query = tab.narrow.clone();
        let generation = self.next_generation;
        self.next_generation += 1;
        let filter = self.filter();
        let cancel = Arc::new(AtomicBool::new(false));
        self.ensure_rt(id);
        let rt = self.rt.get_mut(&id).unwrap();
        rt.cancel.store(true, Ordering::Relaxed);
        rt.cancel = cancel.clone();
        rt.generation = generation;
        let fresh = || Listing::new(generation, filter).with_query(query.as_deref());
        if keep_old && rt.listing.is_some() {
            rt.pending = Some(fresh());
        } else {
            rt.pending = None;
            rt.listing = Some(fresh());
            if let Some(side) = self.shown_side(id) {
                listview::set_count(self.panes[side.index()].list, 0);
            }
        }
        if let Some(t) = self.ws.tab_mut(id) {
            t.status = TabStatus::Loading;
        }
        let notifier = self.notifier.clone();
        self.enum_pool.spawn(move || {
            let req = EnumRequest {
                location,
                sort,
                cancelled: cancel,
            };
            enumerate(&req, &mut |event| {
                notifier.send(WorkerMsg::Enum {
                    tab: id,
                    generation,
                    event,
                })
            });
        });
        self.update_status();
    }

    pub fn on_enum(&mut self, id: TabId, generation: u64, event: EnumEvent) {
        let Some(tab) = self.tab(id) else { return };
        let cmp = crate::fs::sort::comparator(tab.view.sort);
        let query = tab.narrow.clone();
        let shown = self.shown_side(id);
        let filter = self.filter();
        let Some(rt) = self.rt.get_mut(&id) else { return };
        if rt.generation != generation {
            return;
        }
        let into_pending = rt.pending.is_some();
        match event {
            EnumEvent::Chunk(chunk) => {
                let target = if into_pending {
                    rt.pending.as_mut()
                } else {
                    rt.listing.as_mut()
                };
                if let Some(l) = target {
                    l.append_chunk(chunk, &cmp);
                }
                if !into_pending {
                    self.after_listing_change(id, false);
                }
            }
            EnumEvent::Done => {
                if let Some(mut fresh) = rt.pending.take() {
                    fresh.complete = true;
                    let old = rt.listing.take();
                    let mut focus_name = None;
                    if let Some(old) = &old {
                        fresh.select_names(&old.selected_names());
                        if let Some(side) = shown {
                            focus_name = listview::focused(self.panes[side.index()].list)
                                .and_then(|p| old.get(p))
                                .map(|e| e.name.clone());
                        }
                    }
                    rt.listing = Some(fresh);
                    if let (Some(side), Some(name)) = (shown, focus_name) {
                        let l = rt.listing.as_ref().unwrap();
                        if let Some(pos) = l.find(&name).and_then(|i| l.position_of(i)) {
                            let list = self.panes[side.index()].list;
                            self.after_listing_change(id, true);
                            listview::set_focused(list, pos);
                        }
                    }
                } else if let Some(l) = rt.listing.as_mut() {
                    l.complete = true;
                }
                if let Some(t) = self.ws.tab_mut(id) {
                    t.status = TabStatus::Ready;
                }
                self.after_listing_change(id, true);
                if let Some(side) = shown {
                    self.place_placeholder(side);
                }
                self.invalidate_chrome();
            }
            EnumEvent::Failed(e) => {
                rt.pending = None;
                let mut empty = Listing::new(generation, filter).with_query(query.as_deref());
                empty.complete = true;
                rt.listing = Some(empty);
                if let Some(t) = self.ws.tab_mut(id) {
                    t.status = match e {
                        crate::fs::FsError::NotFound => TabStatus::NotFound,
                        other => TabStatus::Error(other.to_string()),
                    };
                }
                self.after_listing_change(id, true);
                if let Some(side) = shown {
                    self.place_placeholder(side);
                }
                self.invalidate_chrome();
            }
        }
    }

    /// Push listing changes to the control: item count, selection and pending focus.
    pub fn after_listing_change(&mut self, id: TabId, full_redraw: bool) {
        let Some(side) = self.shown_side(id) else {
            return;
        };
        let list = self.panes[side.index()].list;
        let Some(rt) = self.rt.get_mut(&id) else { return };
        let Some(l) = rt.listing.as_mut() else { return };
        let mut focus = None;
        if let Some(name) = rt.select_on_load.clone()
            && let Some(pos) = l.select_only_name(&name)
        {
            rt.select_on_load = None;
            focus = Some(pos);
        }
        // Copy what the control needs and release the listing before calling into it: the
        // control's change notifications re-enter the App.
        let len = l.len();
        let selected = (l.selection_count() > 0 || full_redraw).then(|| l.selected_positions());
        self.suppress_selection = true;
        listview::set_count(list, len);
        if let Some(positions) = &selected {
            listview::select_positions(list, positions);
        }
        if let Some(pos) = focus {
            listview::set_focused(list, pos);
        }
        self.suppress_selection = false;
        if full_redraw {
            unsafe {
                let _ = InvalidateRect(Some(list), None, false);
            }
        }
        if self.ws.active_tab().id == id {
            self.update_status();
        }
        self.invalidate_filter_bar(side);
        if self.pending_rename.as_ref().is_some_and(|(t, _)| *t == id) {
            self.try_pending_rename();
        }
    }

    pub fn on_watch(&mut self, ev: WatchEvent) {
        match ev {
            WatchEvent::Changes { key, changes } => {
                let Some(tab) = self.tab(key) else { return };
                let cmp = crate::fs::sort::comparator(tab.view.sort);
                let Some(l) = self.rt.get_mut(&key).and_then(|r| r.listing.as_mut()) else {
                    return;
                };
                for c in changes {
                    match c {
                        Change::Upsert(e) => {
                            l.insert(e, &cmp);
                        }
                        Change::Removed(name) => {
                            l.remove(&name);
                        }
                    }
                }
                self.after_listing_change(key, true);
            }
            WatchEvent::Overflow { key } => {
                if self.shown_side(key).is_some() {
                    self.start_enum(key, true);
                }
            }
            WatchEvent::Failed { key } => {
                // The folder itself may be gone: re-enumerate to find out.
                if self.shown_side(key).is_some() {
                    self.start_enum(key, true);
                }
            }
        }
    }

    pub fn on_icon(&mut self, id: TabId, generation: u64, name: &[u16], icon: i32) {
        let Some(rt) = self.rt.get_mut(&id) else { return };
        if rt.generation != generation {
            return;
        }
        let Some(l) = rt.listing.as_mut() else { return };
        let Some(eid) = l.find(name) else { return };
        let Some(pos) = l.position_of(eid) else { return };
        if let Some(e) = l.get_mut(pos) {
            e.icon = icon;
        }
        if let Some(side) = self.shown_side(id) {
            listview::redraw_item(self.panes[side.index()].list, pos);
        }
    }

    /// Navigate a tab. `select` is selected once it appears (e.g. the folder we came from).
    pub fn navigate(&mut self, id: TabId, loc: Location, push_history: bool, select: Option<Box<[u16]>>) {
        let Some(tab) = self.ws.tab_mut(id) else { return };
        if push_history && tab.location != loc {
            let cur = tab.location.clone();
            tab.history.push(cur);
        }
        tab.location = loc.clone();
        // Narrowing belongs to the folder it was typed in.
        tab.narrow = None;
        self.ensure_rt(id);
        let rt = self.rt.get_mut(&id).unwrap();
        rt.title = names::display_name(&loc);
        rt.folder_pidl = if loc.is_file_system() {
            None
        } else {
            Pidl::parse(loc.as_str()).ok()
        };
        rt.select_on_load = select;
        rt.top_index = 0;
        self.start_enum(id, false);
        if let Some(side) = self.shown_side(id) {
            self.panes[side.index()].addr_error = false;
            self.watch(side);
            self.place_placeholder(side);
            self.sync_filter_bar(side);
        }
        self.mark_dirty();
        self.invalidate_chrome();
        self.update_title();
    }

    /// Location of the parent folder, "This PC" above drive roots.
    pub fn parent_of(&self, loc: &Location) -> Option<Location> {
        if loc.is_file_system() {
            if let Some(p) = loc.parent() {
                return Some(p);
            }
            if loc.as_str().starts_with("\\\\") {
                return None;
            }
            return Some(Location::new(THIS_PC));
        }
        let pidl = Pidl::parse(loc.as_str()).ok()?;
        unsafe {
            if (*pidl.0).mkid.cb == 0 || !ILRemoveLastID(Some(pidl.0)).as_bool() {
                return None;
            }
        }
        let name = unsafe { SHGetNameFromIDList(pidl.0, SIGDN_DESKTOPABSOLUTEPARSING).ok()? };
        let s = unsafe { name.to_string().ok() };
        unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _)) };
        s.map(Location::new)
    }

    pub fn place_placeholder(&self, side: PaneSide) {
        let p = &self.panes[side.index()];
        let status = p.shown.and_then(|id| self.tab(id)).map(|t| t.status.clone());
        let text = match status {
            Some(TabStatus::NotFound) => Some("フォルダが見つかりません".to_string()),
            Some(TabStatus::Error(e)) => Some(format!("フォルダを開けません: {e}")),
            _ => None,
        };
        let (Some(text), Some(rects)) = (text, p.rects) else {
            for h in [p.msg, p.retry, p.parent_btn] {
                ui::place(h, Rect::default(), false);
            }
            return;
        };
        let s = |v: i32| layout::scale(v, self.dpi);
        let r = rects.list;
        let cx = (r.left + r.right) / 2;
        let top = r.top + s(60);
        unsafe {
            let _ = SetWindowTextW(p.msg, &HSTRING::from(text));
        }
        let place_top = |h: HWND, rr: Rect| unsafe {
            let _ = SetWindowPos(
                h,
                Some(HWND_TOP),
                rr.left,
                rr.top,
                rr.width(),
                rr.height(),
                SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
        };
        place_top(p.msg, Rect::new(r.left + s(8), top, r.right - s(8), top + s(24)));
        place_top(p.retry, Rect::new(cx - s(130), top + s(36), cx - s(10), top + s(66)));
        place_top(
            p.parent_btn,
            Rect::new(cx + s(10), top + s(36), cx + s(130), top + s(66)),
        );
    }

    // ---- list view notifications ----

    pub fn on_notify(&mut self, hdr: &NMHDR, lp: LPARAM) -> LRESULT {
        if hdr.hwndFrom == self.nav.hwnd {
            return self.on_tree_notify(hdr, lp);
        }
        if let Some(side) = self.side_of_list(hdr.hwndFrom) {
            return self.on_list_notify(side, hdr, lp);
        }
        for side in PaneSide::BOTH {
            let list = self.panes[side.index()].list;
            if hdr.hwndFrom == listview::header(list) {
                return self.on_header_notify(side, hdr);
            }
        }
        LRESULT(0)
    }

    fn on_header_notify(&mut self, side: PaneSide, hdr: &NMHDR) -> LRESULT {
        match hdr.code {
            HDN_ENDTRACKW | HDN_ENDDRAG | HDN_DIVIDERDBLCLICKW => {
                self.mark_dirty();
            }
            NM_RCLICK => {
                self.set_active_pane(side);
                self.column_menu(side);
                return LRESULT(1);
            }
            _ => {}
        }
        LRESULT(0)
    }

    fn shown_listing(&self, side: PaneSide) -> Option<(&Tab, &Listing)> {
        let id = self.panes[side.index()].shown?;
        let tab = self.tab(id)?;
        let l = self.rt.get(&id)?.listing.as_ref()?;
        Some((tab, l))
    }

    fn on_list_notify(&mut self, side: PaneSide, hdr: &NMHDR, lp: LPARAM) -> LRESULT {
        unsafe {
            match hdr.code {
                LVN_GETDISPINFOW => {
                    let di = &mut *(lp.0 as *mut NMLVDISPINFOW);
                    self.get_dispinfo(side, &mut di.item);
                }
                NM_CUSTOMDRAW => {
                    let cd = &mut *(lp.0 as *mut NMLVCUSTOMDRAW);
                    return self.custom_draw(side, cd);
                }
                LVN_ODFINDITEMW => {
                    let fi = &*(lp.0 as *const NMLVFINDITEMW);
                    return LRESULT(self.find_item(side, fi));
                }
                LVN_ITEMCHANGED => {
                    let nm = &*(lp.0 as *const NMLISTVIEW);
                    if !self.suppress_selection
                        && nm.uChanged.0 & LVIF_STATE.0 != 0
                        && (nm.uOldState ^ nm.uNewState) & LVIS_SELECTED.0 != 0
                    {
                        self.list_selection_changed(side, nm.iItem, nm.iItem, nm.uNewState & LVIS_SELECTED.0 != 0);
                    }
                }
                LVN_ODSTATECHANGED => {
                    let nm = &*(lp.0 as *const NMLVODSTATECHANGE);
                    if !self.suppress_selection && (nm.uOldState.0 ^ nm.uNewState.0) & LVIS_SELECTED.0 != 0 {
                        self.list_selection_changed(side, nm.iFrom, nm.iTo, nm.uNewState.0 & LVIS_SELECTED.0 != 0);
                    }
                }
                LVN_COLUMNCLICK => {
                    let nm = &*(lp.0 as *const NMLISTVIEW);
                    let col = self
                        .shown_listing(side)
                        .and_then(|(t, _)| t.view.columns.get(nm.iSubItem as usize))
                        .map(|c| c.id);
                    if let Some(col) = col {
                        self.set_active_pane(side);
                        self.execute(Command::SortBy(col));
                    }
                }
                NM_DBLCLK => {
                    let nm = &*(lp.0 as *const NMITEMACTIVATE);
                    if nm.iItem >= 0 {
                        self.execute(Command::Open);
                    }
                }
                NM_SETFOCUS => self.set_active_pane(side),
                NM_RCLICK => {
                    self.set_active_pane(side);
                    let nm = &*(lp.0 as *const NMITEMACTIVATE);
                    self.context_menu(side, nm.iItem >= 0);
                    return LRESULT(1);
                }
                LVN_BEGINLABELEDITW => {
                    // Label editing only starts through the Rename command.
                    return LRESULT(if self.editing_label { 0 } else { 1 });
                }
                LVN_ENDLABELEDITW => {
                    let di = &*(lp.0 as *const NMLVDISPINFOW);
                    self.editing_label = false;
                    if !di.item.pszText.is_null() {
                        let text = di.item.pszText.to_string().unwrap_or_default();
                        self.finish_rename(side, di.item.iItem as usize, text);
                    }
                    return LRESULT(0);
                }
                LVN_BEGINDRAG | LVN_BEGINRDRAG => {
                    self.begin_drag(side);
                }
                _ => {}
            }
        }
        LRESULT(0)
    }

    fn list_selection_changed(&mut self, side: PaneSide, from: i32, to: i32, selected: bool) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(l) = self.rt.get_mut(&id).and_then(|r| r.listing.as_mut()) else {
            return;
        };
        if from < 0 {
            if selected { l.select_all() } else { l.clear_selection() }
        } else {
            for p in from..=to {
                l.set_selected(p as usize, selected);
            }
        }
        if side == self.ws.active_pane {
            self.update_status();
        }
    }

    fn find_item(&self, side: PaneSide, fi: &NMLVFINDITEMW) -> isize {
        let Some((_, l)) = self.shown_listing(side) else {
            return -1;
        };
        if fi.lvfi.flags.0 & (LVFI_STRING.0 | LVFI_PARTIAL.0) == 0 || fi.lvfi.psz.is_null() {
            return -1;
        }
        let needle: Vec<u16> = unsafe { fi.lvfi.psz.as_wide() }.iter().map(|&c| lower(c)).collect();
        let n = l.len();
        if n == 0 {
            return -1;
        }
        let start = (fi.iStart.max(0) as usize).min(n);
        for k in 0..n {
            let pos = (start + k) % n;
            let name = &l.get(pos).unwrap().name;
            if name.len() >= needle.len() && name.iter().zip(&needle).all(|(&a, &b)| lower(a) == b) {
                return pos as isize;
            }
        }
        -1
    }

    fn get_dispinfo(&mut self, side: PaneSide, item: &mut LVITEMW) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(tab) = self.tab(id) else { return };
        let col = tab
            .view
            .columns
            .get(item.iSubItem as usize)
            .map(|c| c.id)
            .unwrap_or(ColumnId::Name);
        let icon_view = matches!(tab.view.mode, ViewMode::IconsLarge | ViewMode::IconsMedium);
        let dir = tab.location.0.clone();
        let show_ext = self.config.show_extensions;
        let tab_gen = self.rt.get(&id).map(|r| r.generation).unwrap_or(0);
        let icons = &mut self.icons;
        let Some(rt) = self.rt.get_mut(&id) else { return };
        let folder_pidl = rt.folder_pidl.as_ref().map(|p| p.0);
        let Some(l) = rt.listing.as_mut() else { return };
        let pos = item.iItem as usize;
        let Some(e) = l.get_mut(pos) else { return };
        if item.mask.0 & LVIF_IMAGE.0 != 0 {
            if e.icon == -1 {
                e.icon = match (&e.shell_id, folder_pidl) {
                    (Some(child), Some(parent)) => unsafe {
                        let abs = ILCombine(Some(parent), Some(child.as_ptr() as *const _));
                        let i = crate::shell::icons::pidl_icon(abs);
                        windows::Win32::System::Com::CoTaskMemFree(Some(abs as _));
                        i
                    },
                    _ => {
                        let generic = icons.generic_icon(e);
                        if crate::shell::icons::has_custom_icon(e) {
                            let path = crate::fs::join(&dir, &e.name_string());
                            let name = e.name.clone();
                            let n = self.notifier.clone();
                            self.icon_pool.spawn(move || {
                                let icon = crate::shell::icons::path_icon(&path);
                                if icon >= 0 {
                                    n.send(WorkerMsg::Icon {
                                        tab: id,
                                        generation: tab_gen,
                                        name,
                                        icon,
                                    });
                                }
                            });
                        }
                        generic
                    }
                };
            }
            item.iImage = if e.icon == crate::model::listing::THUMB_ICON && !icon_view {
                icons.generic_icon(e)
            } else {
                e.icon
            };
        }
        if item.mask.0 & LVIF_TEXT.0 != 0 {
            let text: Vec<u16> = match col {
                ColumnId::Name => {
                    let ext = e.extension().len();
                    if !show_ext && ext > 0 {
                        e.name[..e.name.len() - ext - 1].to_vec()
                    } else {
                        e.name.to_vec()
                    }
                }
                ColumnId::Modified => ui::format::datetime(e.modified).encode_utf16().collect(),
                ColumnId::Created => ui::format::datetime(e.created).encode_utf16().collect(),
                ColumnId::Size => {
                    if e.is_dir() || e.shell_id.is_some() && e.size == 0 {
                        Vec::new()
                    } else {
                        ui::format::size_kb(e.size).encode_utf16().collect()
                    }
                }
                ColumnId::Type => icons.type_name(e).encode_utf16().collect(),
                ColumnId::Attributes => ui::format::attributes(e.attrs).encode_utf16().collect(),
                ColumnId::Extension => e.extension().to_vec(),
            };
            unsafe { ui::copy_to_buf(text, item.pszText.0, item.cchTextMax) };
        }
    }

    /// Entries selected in the active pane, with their full locations.
    pub fn selection(&self, side: PaneSide) -> Vec<(Entry, Location)> {
        let Some((tab, l)) = self.shown_listing(side) else {
            return Vec::new();
        };
        let id = tab.id;
        let parent = self.rt.get(&id).and_then(|r| r.folder_pidl.as_ref());
        l.selected_entries()
            .into_iter()
            .filter_map(|e| {
                let loc = match (&e.shell_id, parent) {
                    (Some(child), Some(p)) => names::child_location(p, child)?,
                    _ => Location::new(crate::fs::join(tab.location.as_str(), &e.name_string())),
                };
                Some((e.clone(), loc))
            })
            .collect()
    }

    pub fn open_selection(&mut self, new_tab: bool) {
        let side = self.ws.active_pane;
        let items = self.selection(side);
        if items.is_empty() {
            return;
        }
        let tab_id = self.ws.active_tab().id;
        let single_dir = items.len() == 1 && items[0].0.is_dir() && !new_tab;
        for (e, loc) in items {
            if e.is_dir() {
                if single_dir {
                    self.navigate(tab_id, loc, true, None);
                } else {
                    self.ws.open_tab(side, loc, None, false);
                }
            } else {
                self.shell_open(&loc);
            }
        }
        self.refresh_panes();
    }

    pub fn shell_open(&self, loc: &Location) {
        unsafe {
            let file = HSTRING::from(loc.as_str());
            let pidl = if loc.is_file_system() {
                None
            } else {
                Pidl::parse(loc.as_str()).ok()
            };
            let mut sei = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: if pidl.is_some() {
                    SEE_MASK_IDLIST
                } else {
                    SEE_MASK_DEFAULT
                },
                hwnd: self.hwnd,
                lpFile: if pidl.is_some() {
                    PCWSTR::null()
                } else {
                    PCWSTR(file.as_ptr())
                },
                lpIDList: pidl.as_ref().map(|p| p.0 as *mut _).unwrap_or(std::ptr::null_mut()),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            let _ = ShellExecuteExW(&mut sei);
        }
    }
}

fn lower(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) {
        c + 32
    } else {
        c
    }
}
