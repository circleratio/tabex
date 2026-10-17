//! Explorer-compatible context menus (IContextMenu with IContextMenu3 message forwarding).

use std::cell::RefCell;

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::clipboard::Selection;

const FIRST_ID: u32 = 1;
const CMIC_MASK_UNICODE: u32 = 0x0000_4000;
const LAST_ID: u32 = 0x7FFF;

thread_local! {
    /// Menu being tracked, for forwarding owner-draw and init messages.
    static ACTIVE: RefCell<Option<IContextMenu>> = const { RefCell::new(None) };
}

/// Forward menu messages to the active shell context menu. Returns Some(result) if handled.
pub fn handle_menu_msg(msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    if !matches!(
        msg,
        WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR | WM_MENUSELECT
    ) {
        return None;
    }
    ACTIVE.with(|a| {
        let a = a.borrow();
        let cm = a.as_ref()?;
        unsafe {
            if let Ok(cm3) = cm.cast::<IContextMenu3>() {
                let mut result = LRESULT(0);
                if cm3.HandleMenuMsg2(msg, wp, lp, Some(&mut result)).is_ok() {
                    return Some(result);
                }
            } else if let Ok(cm2) = cm.cast::<IContextMenu2>()
                && cm2.HandleMenuMsg(msg, wp, lp).is_ok()
            {
                return Some(LRESULT(0));
            }
        }
        None
    })
}

pub fn is_active() -> bool {
    ACTIVE.with(|a| a.borrow().is_some())
}

/// Result of showing a menu: a verb tabex handles itself, or nothing (shell handled it).
pub enum Chosen {
    Verb(String),
    None,
}

/// Verbs that tabex executes itself so they use its rename UI and undo history.
const OWN_VERBS: [&str; 6] = ["rename", "delete", "copy", "cut", "paste", "newfolder"];

/// Show the context menu for `sel` (items) or for the folder background (`sel.children` empty).
pub fn show(owner: HWND, sel: &Selection, background_dir: Option<&str>, x: i32, y: i32) -> Result<Chosen> {
    unsafe {
        let cm: IContextMenu = if sel.children.is_empty() {
            sel.folder.CreateViewObject(owner)?
        } else {
            sel.folder.GetUIObjectOf(owner, &sel.ptrs(), None)?
        };
        let menu = CreatePopupMenu()?;
        let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
        let mut flags = CMF_NORMAL | CMF_EXPLORE | CMF_CANRENAME;
        if shift {
            flags |= CMF_EXTENDEDVERBS;
        }
        cm.QueryContextMenu(menu, 0, FIRST_ID, LAST_ID, flags).ok()?;
        ACTIVE.with(|a| *a.borrow_mut() = Some(cm.clone()));
        let id = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, x, y, owner, None).0 as u32;
        ACTIVE.with(|a| *a.borrow_mut() = None);
        let result = if id >= FIRST_ID {
            let offset = id - FIRST_ID;
            let mut verb = [0u16; 128];
            let has_verb = cm
                .GetCommandString(
                    offset as usize,
                    GCS_VERBW,
                    None,
                    PSTR(verb.as_mut_ptr() as *mut u8),
                    verb.len() as u32,
                )
                .is_ok();
            let verb = if has_verb {
                let n = verb.iter().position(|&c| c == 0).unwrap_or(0);
                String::from_utf16_lossy(&verb[..n]).to_ascii_lowercase()
            } else {
                String::new()
            };
            if OWN_VERBS.contains(&verb.as_str()) {
                Chosen::Verb(verb)
            } else {
                let dir = background_dir.map(HSTRING::from);
                let mut info = CMINVOKECOMMANDINFOEX {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                    fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE,
                    hwnd: owner,
                    lpVerb: PCSTR(offset as usize as *const u8),
                    lpVerbW: PCWSTR(offset as usize as *const u16),
                    nShow: SW_SHOWNORMAL.0,
                    ptInvoke: POINT { x, y },
                    ..Default::default()
                };
                if let Some(d) = &dir {
                    info.lpDirectoryW = PCWSTR(d.as_ptr());
                }
                if shift {
                    info.fMask |= CMIC_MASK_SHIFT_DOWN;
                }
                if GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    info.fMask |= CMIC_MASK_CONTROL_DOWN;
                }
                cm.InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO)?;
                Chosen::None
            }
        } else {
            Chosen::None
        };
        let _ = DestroyMenu(menu);
        Ok(result)
    }
}
