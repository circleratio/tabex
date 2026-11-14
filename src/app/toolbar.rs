//! Toolbar commands, the view mode dropdown, tooltips and keeping the buttons in sync.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Controls::*;

use super::*;
use crate::model::command::Command;
use crate::model::homes;
use crate::model::toolbar::{self, ButtonId, ToolbarState, VIEW_MODES};
use crate::ui::accel::format_key;
use crate::ui::toolbar::Toolbar;

impl App {
    pub fn toolbar_state(&self) -> ToolbarState {
        let tab = self.ws.active_tab();
        let can_new_folder = tab.location.is_file_system();
        let selected = self
            .rt
            .get(&tab.id)
            .and_then(|r| r.listing.as_ref())
            .is_some_and(|l| l.selection_count() > 0);
        ToolbarState {
            can_new_folder,
            can_rename: can_new_folder && selected,
            can_undo: !self.undo_stack.is_empty(),
            view_mode: self.ws.active_tab().view.mode,
            show_hidden: self.config.show_hidden,
            show_dotfiles: self.config.show_dotfiles,
            show_extensions: self.config.show_extensions,
            nav_visible: self.config.nav_tree_visible,
            dual: self.ws.layout == Layout::Dual,
        }
    }

    /// Cheap when nothing changed, so it is called after every command.
    pub fn sync_toolbar(&mut self) {
        let st = self.toolbar_state();
        self.toolbar.apply(&st);
    }

    pub fn on_toolbar_command(&mut self, id: i32) {
        if let Some(cmd) = Toolbar::button_of(id).and_then(|b| toolbar::def(b).command.clone()) {
            self.execute(cmd);
        } else {
            // A check button flips its own look on click; put it back.
            self.sync_toolbar();
        }
    }

    pub fn on_toolbar_notify(&mut self, hdr: &NMHDR, lp: LPARAM) -> Option<LRESULT> {
        match hdr.code {
            TBN_DROPDOWN if self.toolbar.owns(hdr.hwndFrom) => {
                self.view_mode_menu();
                Some(LRESULT(TBDDRET_DEFAULT as isize))
            }
            NM_RCLICK if self.toolbar.owns(hdr.hwndFrom) => {
                let nm = unsafe { &*(lp.0 as *const NMMOUSE) };
                if Toolbar::button_of(nm.dwItemSpec as i32) == Some(ButtonId::Home) {
                    self.home_menu();
                }
                // Non-zero: no default handling and no WM_CONTEXTMENU.
                Some(LRESULT(1))
            }
            TTN_GETDISPINFOW => {
                let button = Toolbar::button_of(hdr.idFrom as i32)?;
                let def = toolbar::def(button);
                let key = def
                    .command
                    .as_ref()
                    .and_then(|c| self.keymap.key_for(c))
                    .map(format_key);
                let text = toolbar::tooltip(def.label, key.as_deref());
                let info = unsafe { &mut *(lp.0 as *mut NMTTDISPINFOW) };
                info.lpszText = self.toolbar.set_tip(&text);
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    /// Screen position just below a button, for its menu.
    fn below_button(&self, id: ButtonId) -> POINT {
        let rc = self.toolbar.button_rect(id);
        let mut pt = POINT {
            x: rc.left,
            y: rc.bottom,
        };
        unsafe {
            let _ = ClientToScreen(self.toolbar.bar_of(id), &mut pt);
        }
        pt
    }

    /// Move the active tab to a home directory, like typing it in the address bar; nothing
    /// happens when the tab already shows it.
    pub fn go_home_to(&mut self, loc: &Location) {
        // A missing folder does not resolve: go there as written and show "not found".
        let loc = crate::shell::names::resolve(loc.as_str()).unwrap_or_else(|| loc.clone());
        if homes::same_place(&self.ws.active_tab().location, &loc) {
            return;
        }
        self.execute(Command::Navigate(loc));
    }

    /// Right click on the home button: every home directory, the current one checked.
    fn home_menu(&mut self) {
        let pt = self.below_button(ButtonId::Home);
        let current = self.ws.active_tab().location.clone();
        // Compare with the configured paths only, so network paths never slow the menu down.
        let items = self
            .homes
            .iter()
            .map(|h| {
                Some((
                    homes::menu_label(h),
                    Command::Navigate(h.location.clone()),
                    homes::same_place(&h.location, &current),
                ))
            })
            .collect();
        if let Some(Command::Navigate(loc)) = self.menu.track(self.hwnd, items, pt.x, pt.y) {
            self.go_home_to(&loc);
        }
    }

    fn view_mode_menu(&mut self) {
        let pt = self.below_button(ButtonId::ViewMode);
        let current = self.ws.active_tab().view.mode;
        let items = VIEW_MODES
            .iter()
            .map(|(mode, label, _)| {
                let cmd = Command::SetViewMode(*mode);
                let text = match self.keymap.key_for(&cmd) {
                    Some(k) => format!("{label}\t{}", format_key(k)),
                    None => label.to_string(),
                };
                Some((text, cmd, *mode == current))
            })
            .collect();
        if let Some(cmd) = self.menu.track(self.hwnd, items, pt.x, pt.y) {
            self.execute(cmd);
        }
    }
}
