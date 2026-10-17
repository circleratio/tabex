//! Win32 user interface pieces. Each control gets the `App` through the main window.

pub mod accel;
pub mod addressbar;
pub mod dpi;
pub mod filterbar;
pub mod format;
pub mod listview;
pub mod navtree;
pub mod tabbar;
pub mod theme;
pub mod workspace_dialog;

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::model::layout::Rect;

pub fn wz(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn loword(v: usize) -> u16 {
    (v & 0xFFFF) as u16
}

pub fn hiword(v: usize) -> u16 {
    ((v >> 16) & 0xFFFF) as u16
}

/// Signed client coordinates packed in an LPARAM.
pub fn point_from_lparam(lp: LPARAM) -> (i32, i32) {
    let x = (lp.0 & 0xFFFF) as u16 as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
    (x, y)
}

pub fn to_win(r: Rect) -> RECT {
    RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

pub fn from_win(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right, r.bottom)
}

pub fn client_rect(hwnd: HWND) -> Rect {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    from_win(rc)
}

pub fn place(hwnd: HWND, r: Rect, visible: bool) {
    unsafe {
        let flags = SWP_NOZORDER | SWP_NOACTIVATE | if visible { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW };
        let _ = SetWindowPos(hwnd, None, r.left, r.top, r.width(), r.height(), flags);
    }
}

pub fn window_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// Copy `s` into a fixed-size wide buffer provided by a control (truncating, NUL-terminated).
///
/// # Safety
/// `buf` must point to at least `cap` writable u16s.
pub unsafe fn copy_to_buf(s: impl IntoIterator<Item = u16>, buf: *mut u16, cap: i32) {
    if buf.is_null() || cap <= 0 {
        return;
    }
    let cap = cap as usize;
    let mut i = 0;
    for c in s {
        if i + 1 >= cap {
            break;
        }
        unsafe { *buf.add(i) = c };
        i += 1;
    }
    unsafe { *buf.add(i) = 0 };
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}
