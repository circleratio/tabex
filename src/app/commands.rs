//! Command execution.

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::*;
use crate::model::command::Command;
use crate::ui::listview;

pub fn debug_log(s: &str) {
    use std::io::Write;
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("tabex");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("trace.log"))
    {
        let _ = writeln!(f, "{s}");
    }
}

impl App {
    pub fn execute(&mut self, cmd: Command) {
        use Command::*;
        if std::env::var_os("TABEX_TRACE").is_some() {
            debug_log(&format!("execute {cmd:?}"));
        }
        let side = self.ws.active_pane;
        let home = self.home.clone();
        match cmd {
            NewTab => {
                let loc = self.ws.active_tab().location.clone();
                self.ws.open_tab(side, loc, None, true);
                self.after_tabs_changed();
            }
            CloseTab => {
                let idx = self.ws.pane(side).active_tab;
                self.close_tab(side, idx);
            }
            CloseTabAt(s, i) => self.close_tab(s, i),
            CloseOtherTabs(s, i) => {
                self.ws.close_others(s, i, &home);
                self.after_tabs_changed();
            }
            ReopenClosedTab => {
                if self.ws.reopen_closed().is_some() {
                    self.after_tabs_changed();
                }
            }
            NextTab => {
                self.ws.cycle_tab(1);
                self.after_tabs_changed();
            }
            PrevTab => {
                self.ws.cycle_tab(-1);
                self.after_tabs_changed();
            }
            SelectTab(n) => {
                let len = self.ws.pane(side).tabs.len();
                // Ctrl+9 always selects the last tab, like browsers.
                let idx = if n == 9 { len - 1 } else { n as usize - 1 };
                if idx < len {
                    self.ws.activate(side, idx);
                    self.after_tabs_changed();
                }
            }
            ActivateTab(s, i) => {
                self.ws.activate(s, i);
                self.after_tabs_changed();
                self.focus_active_list();
            }
            DuplicateTab => {
                let idx = self.ws.pane(side).active_tab;
                self.ws.duplicate_tab(side, idx);
                self.after_tabs_changed();
            }
            DuplicateTabAt(s, i) => {
                self.ws.duplicate_tab(s, i);
                self.after_tabs_changed();
            }
            MoveTab { from, to } => {
                self.ws.move_tab(from, to, &home);
                self.after_tabs_changed();
                self.focus_active_list();
            }
            SwitchPane => {
                if self.ws.layout == Layout::Dual {
                    self.set_active_pane(side.other());
                    self.focus_active_list();
                }
            }
            ToggleLayout => {
                self.ws.layout = if self.ws.layout == Layout::Dual {
                    Layout::Single
                } else {
                    Layout::Dual
                };
                self.after_tabs_changed();
                self.layout();
            }
            ToggleNavTree => {
                self.config.nav_tree_visible = !self.config.nav_tree_visible;
                self.config_dirty = true;
                self.layout();
            }
            Back | Forward => {
                let tab = self.ws.active_tab_mut();
                let cur = tab.location.clone();
                let target = if cmd == Back {
                    tab.history.back(cur)
                } else {
                    tab.history.forward(cur)
                };
                if let Some(loc) = target {
                    let id = self.ws.active_tab().id;
                    self.navigate(id, loc, false, None);
                }
            }
            Up => {
                let tab = self.ws.active_tab();
                let (id, cur) = (tab.id, tab.location.clone());
                if let Some(parent) = self.parent_of(&cur) {
                    let came_from: Option<Box<[u16]>> = cur
                        .is_file_system()
                        .then(|| cur.display_name().encode_utf16().collect::<Vec<u16>>().into());
                    self.navigate(id, parent, true, came_from);
                }
            }
            Refresh => {
                let id = self.ws.active_tab().id;
                self.start_enum(id, true);
            }
            FocusAddress => ui::addressbar::begin_edit(self, side),
            Navigate(loc) => {
                let id = self.ws.active_tab().id;
                self.navigate(id, loc, true, None);
            }
            OpenInNewTab(loc) => {
                self.ws.open_tab(side, loc, None, true);
                self.after_tabs_changed();
            }
            Open => self.open_selection(false),
            OpenSelectedInNewTab => self.open_selection(true),
            SelectAll => {
                let list = self.panes[side.index()].list;
                if let Some(l) = self
                    .rt
                    .get_mut(&self.ws.active_tab().id)
                    .and_then(|r| r.listing.as_mut())
                {
                    l.select_all();
                    self.suppress_selection = true;
                    listview::apply_selection(list, l);
                    self.suppress_selection = false;
                }
                self.update_status();
            }
            SetViewMode(mode) => {
                self.capture_view(side);
                let tab = self.ws.active_tab_mut();
                tab.view.mode = mode;
                let view = tab.view.clone();
                let list = self.panes[side.index()].list;
                listview::set_view_mode(list, mode, &self.lists);
                listview::set_columns(list, &view, self.dpi);
                self.mark_dirty();
            }
            SortBy(col) => {
                self.capture_view(side);
                let tab = self.ws.active_tab_mut();
                tab.view.sort = if tab.view.sort.column == col {
                    SortSpec {
                        column: col,
                        order: if tab.view.sort.order == SortOrder::Asc {
                            SortOrder::Desc
                        } else {
                            SortOrder::Asc
                        },
                    }
                } else {
                    SortSpec {
                        column: col,
                        order: SortOrder::Asc,
                    }
                };
                let id = tab.id;
                self.resort(id);
                self.mark_dirty();
            }
            ToggleColumn(col) => {
                if col == ColumnId::Name {
                    return;
                }
                self.capture_view(side);
                let tab = self.ws.active_tab_mut();
                if let Some(i) = tab.view.columns.iter().position(|c| c.id == col) {
                    tab.view.columns.remove(i);
                } else {
                    tab.view.columns.push(ColumnSpec {
                        id: col,
                        width: col.default_width(),
                    });
                }
                let view = tab.view.clone();
                listview::set_columns(self.panes[side.index()].list, &view, self.dpi);
                self.mark_dirty();
            }
            ToggleHidden => {
                self.config.show_hidden = !self.config.show_hidden;
                self.apply_filter_change(if self.config.show_hidden {
                    "隠しファイルを表示します"
                } else {
                    "隠しファイルを表示しません"
                });
            }
            ToggleDotfiles => {
                self.config.show_dotfiles = !self.config.show_dotfiles;
                self.apply_filter_change(if self.config.show_dotfiles {
                    "ドットファイルを表示します"
                } else {
                    "ドットファイルを表示しません"
                });
            }
            ToggleExtensions => {
                self.config.show_extensions = !self.config.show_extensions;
                self.config_dirty = true;
                for p in &self.panes {
                    unsafe {
                        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(p.list), None, false);
                    }
                }
            }
            Rename => self.begin_rename(),
            NewFolder => self.new_folder(),
            Delete { permanent } => self.delete_selection(permanent),
            Copy => self.clipboard_copy(false),
            Cut => self.clipboard_copy(true),
            Paste => self.clipboard_paste(),
            Undo => self.undo(),
            Properties => self.properties(),
            WorkspaceList => ui::workspace_dialog::show(self),
            WorkspaceNew { from_current } => self.workspace_new(from_current),
            WorkspaceSaveAs => self.workspace_save_as(),
            WorkspaceRename => {
                let name = self.ws.name.clone();
                self.workspace_rename(&name);
            }
            WorkspaceDelete => {
                let name = self.ws.name.clone();
                self.workspace_delete(&name);
            }
            WorkspaceSwitch(name) => self.workspace_switch(&name),
            Exit => unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
        }
    }

    /// Re-filter every listing and the navigation tree after a visibility setting changed.
    fn apply_filter_change(&mut self, message: &str) {
        self.config_dirty = true;
        let f = self.filter();
        let ids: Vec<TabId> = self.rt.keys().copied().collect();
        for id in ids {
            let Some(sort) = self.tab(id).map(|t| t.view.sort) else {
                continue;
            };
            let cmp = crate::fs::sort::comparator(sort);
            if let Some(rt) = self.rt.get_mut(&id) {
                for l in rt.listing.iter_mut().chain(rt.pending.iter_mut()) {
                    l.set_filter(f, &cmp);
                }
            }
            self.after_listing_change(id, true);
        }
        // Tree children were filtered when inserted: rebuild from the roots.
        self.nav.reset();
        self.build_nav_tree();
        self.show_status(message);
    }

    fn close_tab(&mut self, side: PaneSide, idx: usize) {
        let home = self.home.clone();
        self.ws.close_tab(side, idx, &home);
        self.after_tabs_changed();
    }

    pub fn after_tabs_changed(&mut self) {
        self.refresh_panes();
        self.mark_dirty();
    }

    /// Re-sort the listing of a tab after its sort spec changed.
    fn resort(&mut self, id: TabId) {
        let Some(tab) = self.tab(id) else { return };
        let view = tab.view.clone();
        let cmp = crate::fs::sort::comparator(view.sort);
        if let Some(rt) = self.rt.get_mut(&id) {
            for l in rt.listing.iter_mut().chain(rt.pending.iter_mut()) {
                l.resort(&cmp);
            }
        }
        if let Some(side) = self.shown_side(id) {
            listview::set_sort_arrow(self.panes[side.index()].list, &view);
        }
        self.after_listing_change(id, true);
    }

    pub fn on_command(&mut self, id: i32, code: u16, ctl: HWND) {
        for side in PaneSide::BOTH {
            let i = side.index();
            if id == ID_RETRY[i] && code as u32 == BN_CLICKED {
                if let Some(tab) = self.panes[i].shown {
                    self.start_enum(tab, false);
                    self.place_placeholder(side);
                }
                return;
            }
            if id == ID_PARENT[i] && code as u32 == BN_CLICKED {
                self.set_active_pane(side);
                self.go_nearest_existing_parent(side);
                return;
            }
        }
        if ctl.0.is_null()
            && let Some(cmd) = self.menu.command_for(id)
        {
            self.execute(cmd);
        }
    }

    /// "Go to parent" on a missing folder: walk up until an existing folder is found.
    fn go_nearest_existing_parent(&mut self, side: PaneSide) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(mut loc) = self.tab(id).map(|t| t.location.clone()) else {
            return;
        };
        while let Some(p) = self.parent_of(&loc) {
            loc = p;
            if !loc.is_file_system() || std::path::Path::new(loc.as_str()).is_dir() {
                break;
            }
        }
        self.navigate(id, loc, true, None);
    }

    pub fn apply_run_args(&mut self, args: &crate::cli::RunArgs, forwarded: bool) {
        if forwarded
            && let Some(ws) = &args.workspace
            && !ws.eq_ignore_ascii_case(&self.ws.name)
        {
            self.workspace_switch(ws);
        }
        let side = self.ws.active_pane;
        for p in &args.paths {
            if let Some(loc) = crate::shell::names::resolve(p) {
                self.ws.open_tab(side, loc, None, true);
            }
        }
        if !args.paths.is_empty() {
            self.after_tabs_changed();
        }
    }
}
