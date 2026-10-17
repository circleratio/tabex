//! Narrowing the file list to names containing a text (requirement 6.1).

use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;

use super::*;
use crate::ui::filterbar;

impl App {
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

    /// Show or hide the filter bar of `side` to match its tab, refilling the edit text.
    pub fn sync_filter_bar(&mut self, side: PaneSide) {
        let text = self.narrow_of(side).map(str::to_string);
        let p = &self.panes[side.index()];
        let (bar, edit) = (p.filter_bar, p.filter_edit);
        let was_visible = unsafe { IsWindowVisible(bar).as_bool() };
        if let Some(t) = &text
            && filterbar::text(edit) != *t
        {
            self.narrow_syncing = true;
            filterbar::set_text(edit, t);
            self.narrow_syncing = false;
        }
        if was_visible != text.is_some() {
            self.layout();
        }
        self.invalidate_filter_bar(side);
    }

    /// Re-run the narrowing of the tab shown in `side` after its text changed.
    fn apply_narrow(&mut self, side: PaneSide) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
        let Some(tab) = self.tab(id) else { return };
        let cmp = crate::fs::sort::comparator(tab.view.sort);
        let query = tab.narrow.clone();
        if let Some(rt) = self.rt.get_mut(&id) {
            for l in rt.listing.iter_mut().chain(rt.pending.iter_mut()) {
                l.set_query(query.as_deref(), &cmp);
            }
        }
        self.after_listing_change(id, true);
    }

    /// "/": open the input (keeping any text already there) and focus it.
    pub fn narrow_start(&mut self, side: PaneSide) {
        let Some(id) = self.panes[side.index()].shown else {
            return;
        };
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
        if self.narrow_syncing {
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
