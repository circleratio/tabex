//! Toolbar commands, the view mode dropdown, tooltips and keeping the buttons in sync.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Controls::*;

use super::*;
use crate::model::command::Command;
use crate::model::toolbar::{self, ButtonId, ToolbarState, VIEW_MODES};
use crate::ui::accel::format_key;
use crate::ui::toolbar::Toolbar;

impl App {
    pub fn toolbar_state(&self) -> ToolbarState {
        ToolbarState {
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
            TBN_DROPDOWN if hdr.hwndFrom == self.toolbar.hwnd => {
                self.view_mode_menu();
                Some(LRESULT(TBDDRET_DEFAULT as isize))
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

    fn view_mode_menu(&mut self) {
        let rc = self.toolbar.button_rect(ButtonId::ViewMode);
        let mut pt = POINT {
            x: rc.left,
            y: rc.bottom,
        };
        unsafe {
            let _ = ClientToScreen(self.toolbar.hwnd, &mut pt);
        }
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
