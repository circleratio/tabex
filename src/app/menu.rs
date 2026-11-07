//! Popup menus (there is no menu bar).

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::*;
use crate::model::command::Command;

const POPUP_BASE: i32 = 5000;

#[derive(Default)]
pub struct MenuState {
    /// Commands of the popup menu currently being tracked.
    popup: Vec<Command>,
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}

impl MenuState {
    fn command_for(&self, id: i32) -> Option<Command> {
        if id < POPUP_BASE {
            return None;
        }
        self.popup.get((id - POPUP_BASE) as usize).cloned()
    }

    /// Show a popup menu built from `items` (None = separator, bool = checked) and return
    /// the chosen command.
    pub fn track(
        &mut self,
        owner: HWND,
        items: Vec<Option<(String, Command, bool)>>,
        x: i32,
        y: i32,
    ) -> Option<Command> {
        self.track_ex(owner, items, &[], x, y)
    }

    /// Like `track`, but items whose command is in `disabled` are grayed out.
    pub fn track_ex(
        &mut self,
        owner: HWND,
        items: Vec<Option<(String, Command, bool)>>,
        disabled: &[Command],
        x: i32,
        y: i32,
    ) -> Option<Command> {
        unsafe {
            let m = CreatePopupMenu().ok()?;
            self.popup.clear();
            for it in items {
                match it {
                    None => separator(m),
                    Some((text, cmd, checked)) => {
                        let id = POPUP_BASE + self.popup.len() as i32;
                        let mut flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                        if disabled.contains(&cmd) {
                            flags |= MF_GRAYED;
                        }
                        self.popup.push(cmd);
                        let _ = AppendMenuW(m, flags, id as usize, &HSTRING::from(text));
                    }
                }
            }
            let id = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, x, y, owner, None).0;
            let _ = DestroyMenu(m);
            self.command_for(id)
        }
    }
}

impl App {
    pub fn column_menu(&mut self, side: PaneSide) {
        let tab = self.ws.pane(side).active();
        let items = ColumnId::ALL
            .iter()
            .filter(|c| **c != ColumnId::Name)
            .map(|c| {
                let on = tab.view.columns.iter().any(|x| x.id == *c);
                Some((
                    ui::listview::column_title(*c).to_string(),
                    Command::ToggleColumn(*c),
                    on,
                ))
            })
            .collect();
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        if let Some(cmd) = self.menu.track(self.hwnd, items, pt.x, pt.y) {
            self.execute(cmd);
        }
    }
}
