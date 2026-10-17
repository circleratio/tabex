//! Narrowing the file list to names containing a text (requirement 6.1).

use std::rc::Rc;

use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;

use super::*;
use crate::model::narrow::{Pattern, Query};
use crate::shell::migemo::Migemo;
use crate::ui::filterbar;

pub enum MigemoState {
    NotLoaded,
    /// Disabled, no DLL, or loading failed (reported once).
    Off,
    Ready(Migemo),
}

impl App {
    /// Load rustmigemo.dll the first time narrowing starts.
    fn ensure_migemo(&mut self) {
        if !matches!(self.migemo, MigemoState::NotLoaded) {
            return;
        }
        self.migemo = MigemoState::Off;
        if !self.config.migemo {
            return;
        }
        let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        else {
            return;
        };
        match crate::shell::migemo::load(&dir) {
            Ok(Some(m)) => self.migemo = MigemoState::Ready(m),
            Ok(None) => {}
            Err(e) => self.show_status(&format!("migemo を使えません: {e}")),
        }
    }

    /// Narrowing condition for `text`, with a migemo pattern when the DLL is loaded.
    pub fn query_for(&mut self, text: Option<&str>) -> Option<Rc<Query>> {
        let text = text?;
        let pattern = match &mut self.migemo {
            MigemoState::Ready(m) if !text.is_empty() => m.query(text).and_then(|p| Pattern::parse(&p).ok()),
            _ => None,
        };
        Some(Rc::new(Query::new(text, pattern)))
    }

    /// Whether the filter bar of `side` shows the "migemo" label.
    pub fn migemo_active(&self, side: PaneSide) -> bool {
        matches!(self.migemo, MigemoState::Ready(_))
            && !self.command_line[side.index()]
            && self.narrow_of(side).is_some()
    }

    /// Narrowing text of the tab shown in `side`; `Some("")` while the input is open but empty.
    pub fn narrow_of(&self, side: PaneSide) -> Option<&str> {
        let id = self.panes[side.index()].shown?;
        self.tab(id)?.narrow.as_deref()
    }

    /// (shown, total) item counts for the filter bar of `side`.
    pub fn narrow_counts(&self, side: PaneSide) -> Option<(usize, usize)> {
        let id = self.panes[side.index()].shown?;
        let l = self.rt.get(&id)?.listing.as_ref()?;
        Some((l.len(), l.total_len()))
    }

    pub fn invalidate_filter_bar(&self, side: PaneSide) {
        unsafe {
            let _ = InvalidateRect(Some(self.panes[side.index()].filter_bar), None, false);
        }
    }

    /// Whether the bar of `side` is shown: narrowing is on or the command line is open.
    pub fn filter_bar_visible(&self, side: PaneSide) -> bool {
        self.command_line[side.index()] || self.narrow_of(side).is_some()
    }

    /// Show or hide the filter bar of `side` to match its tab, refilling the edit text.
    /// While the command line is open its text is left alone.
    pub fn sync_filter_bar(&mut self, side: PaneSide) {
        let visible = self.filter_bar_visible(side);
        let text = if self.command_line[side.index()] {
            None
        } else {
            self.narrow_of(side).map(str::to_string)
        };
        let p = &self.panes[side.index()];
        let (bar, edit) = (p.filter_bar, p.filter_edit);
        let was_visible = unsafe { IsWindowVisible(bar).as_bool() };
        if let Some(t) = &text
            && filterbar::text(edit) != *t
        {
            self.set_bar_text(edit, t);
        }
        if was_visible != visible {
            self.layout();
        }
        // The "migemo" label narrows the edit; it changes with the tab and the bar mode.
        filterbar::layout(bar, edit, self.dpi, self.font, self.migemo_active(side));
        self.invalidate_filter_bar(side);
    }

    /// Replace the edit text without treating it as typing.
    fn set_bar_text(&mut self, edit: HWND, text: &str) {
        self.narrow_syncing = true;
        filterbar::set_text(edit, text);
        self.narrow_syncing = false;
    }

    /// ":": open the command line under the list of `side`.
    pub fn command_line_open(&mut self, side: PaneSide) {
        if self.panes[side.index()].shown.is_none() {
            return;
        }
        self.command_line[side.index()] = true;
        let edit = self.panes[side.index()].filter_edit;
        self.set_bar_text(edit, "");
        self.sync_filter_bar(side);
        unsafe {
            let _ = SetFocus(Some(edit));
        }
    }

    /// Close the command line; the narrowing input (if any) is shown again.
    pub fn command_line_close(&mut self, side: PaneSide) {
        if !std::mem::take(&mut self.command_line[side.index()]) {
            return;
        }
        self.sync_filter_bar(side);
        unsafe {
            let _ = SetFocus(Some(self.panes[side.index()].list));
        }
    }

    /// Enter in the command line: close it and run what was typed.
    pub fn command_line_enter(&mut self, side: PaneSide) {
        let text = filterbar::text(self.panes[side.index()].filter_edit);
        self.command_line_close(side);
        match crate::model::cmdline::parse(&text) {
            Ok(None) => {}
            Ok(Some(crate::model::cmdline::CmdLine::Shell)) => self.open_shell(side),
            Err(cmd) => self.show_status(&format!("不明なコマンドです: {cmd}")),
        }
    }

    /// ":sh": start a shell in the folder shown in `side`.
    pub fn open_shell(&mut self, side: PaneSide) {
        use crate::shell::terminal;
        let Some(tab) = self.panes[side.index()].shown.and_then(|id| self.tab(id)) else {
            return;
        };
        if matches!(tab.status, TabStatus::NotFound | TabStatus::Error(_)) {
            return self.show_status("フォルダが見つかりません");
        }
        if !tab.location.is_file_system() {
            return self.show_status("このフォルダではシェルを開けません");
        }
        let dir = tab.location.as_str().to_string();
        let kind = terminal::choose(self.config.shell.as_deref(), terminal::on_path);
        if dir.starts_with(r"\\") && terminal::is_cmd(&kind) {
            return self.show_status("コマンドプロンプトは UNC パスのフォルダでは開けません");
        }
        if let Err(e) = terminal::launch(&kind, &dir) {
            self.show_status(&format!("シェルを起動できませんでした: {}", e.trim()));
        }
    }

    /// Re-run the narrowing of the tab shown in `side` after its text changed.
    fn apply_narrow(&mut self, side: PaneSide) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(tab) = self.tab(id) else { return };
        let sort = tab.view.sort;
        let narrow = tab.narrow.clone();
        let query = self.query_for(narrow.as_deref());
        let cmp = crate::fs::sort::comparator(sort);
        if let Some(rt) = self.rt.get_mut(&id) {
            for l in rt.listing.iter_mut().chain(rt.pending.iter_mut()) {
                l.set_query(query.clone(), &cmp);
            }
        }
        self.after_listing_change(id, true);
    }

    /// "/": open the input (keeping any text already there) and focus it.
    pub fn narrow_start(&mut self, side: PaneSide) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        self.ensure_migemo();
        let Some(tab) = self.ws.tab_mut(id) else { return };
        tab.narrow.get_or_insert_with(String::new);
        self.sync_filter_bar(side);
        let edit = self.panes[side.index()].filter_edit;
        filterbar::caret_to_end(edit);
        unsafe {
            let _ = SetFocus(Some(edit));
        }
    }

    /// EN_CHANGE from the edit control: narrow as the user types.
    pub fn on_narrow_changed(&mut self, side: PaneSide) {
        if self.narrow_syncing || self.command_line[side.index()] {
            return;
        }
        let text = filterbar::text(self.panes[side.index()].filter_edit);
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(tab) = self.ws.tab_mut(id) else { return };
        if tab.narrow.is_none() {
            return;
        }
        tab.narrow = Some(text);
        self.apply_narrow(side);
    }

    /// Enter: keep the narrowed view and go back to the list.
    pub fn narrow_commit(&mut self, side: PaneSide) {
        let list = self.panes[side.index()].list;
        let first_needed = self.panes[side.index()]
            .shown
            .and_then(|id| self.rt.get(&id))
            .and_then(|rt| rt.listing.as_ref())
            .is_some_and(|l| l.selection_count() == 0 && !l.is_empty());
        unsafe {
            let _ = SetFocus(Some(list));
        }
        if first_needed {
            self.select_row(side, 0);
        }
    }

    /// ↑ / ↓ in the input move the selection of the narrowed list.
    pub fn narrow_cursor(&mut self, side: PaneSide, delta: isize) {
        self.cursor_step(side, delta, false);
    }

    /// Esc (or Backspace on an empty input): show every item again.
    pub fn clear_narrow(&mut self, side: PaneSide) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(tab) = self.ws.tab_mut(id) else { return };
        if tab.narrow.take().is_none() {
            return;
        }
        self.apply_narrow(side);
        self.sync_filter_bar(side);
        unsafe {
            let _ = SetFocus(Some(self.panes[side.index()].list));
        }
    }
}
