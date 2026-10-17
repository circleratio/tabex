//! Middle-click on a folder (file list or navigation tree) opens it in a background tab.

use windows::Win32::Foundation::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::*;

const SUBCLASS_ID: usize = 2;

impl App {
    pub fn install_middle_click(&self) {
        let main = self.hwnd.0 as usize;
        for h in self.panes.iter().map(|p| p.list).chain(std::iter::once(self.nav.hwnd)) {
            unsafe {
                let _ = SetWindowSubclass(h, Some(middle_proc), SUBCLASS_ID, main);
            }
        }
    }

    /// Open `loc` in a new tab right after the active one, without switching to it.
    pub fn open_in_background_tab(&mut self, side: PaneSide, loc: Location) {
        let at = self.ws.pane(side).active_tab + 1;
        self.ws.open_tab(side, loc, Some(at), false);
        self.after_tabs_changed();
    }

    fn folder_at(&self, hwnd: HWND, x: i32, y: i32) -> Option<(PaneSide, Location)> {
        if let Some(side) = self.side_of_list(hwnd) {
            let mut hit = LVHITTESTINFO {
                pt: POINT { x, y },
                ..Default::default()
            };
            let idx = unsafe { SendMessageW(hwnd, LVM_HITTEST, None, Some(LPARAM(&mut hit as *mut _ as isize))) }.0;
            if idx < 0 {
                return None;
            }
            let id = self.panes[side.index()].shown?;
            let tab = self.tab(id)?;
            let rt = self.rt.get(&id)?;
            let e = rt.listing.as_ref()?.get(idx as usize)?;
            if !e.is_dir() {
                return None;
            }
            // Virtual folders resolve their children through the folder PIDL.
            let loc = match (&e.shell_id, rt.folder_pidl.as_ref()) {
                (Some(child), Some(parent)) => crate::shell::names::child_location(parent, child)?,
                _ => Location::new(crate::fs::join(tab.location.as_str(), &e.name_string())),
            };
            return Some((side, loc));
        }
        if hwnd == self.nav.hwnd {
            let loc = self.nav.location_at(x, y)?;
            return Some((self.ws.active_pane, loc));
        }
        None
    }
}

extern "system" fn middle_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, main: usize) -> LRESULT {
    unsafe {
        if msg == WM_MBUTTONUP
            && let Some(app) = App::from_hwnd(HWND(main as *mut _))
        {
            let (x, y) = ui::point_from_lparam(lp);
            if let Some((side, loc)) = app.folder_at(hwnd, x, y) {
                app.open_in_background_tab(side, loc);
                return LRESULT(0);
            }
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
