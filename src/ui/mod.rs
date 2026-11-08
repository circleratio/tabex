//! Win32 user interface pieces. Each control gets the `App` through the main window.

pub mod accel;
pub mod addressbar;
pub mod dialog;
pub mod dpi;
pub mod filterbar;
pub mod format;
pub mod listview;
pub mod navtree;
pub mod settings_dialog;
pub mod tabbar;
pub mod theme;
pub mod toolbar;
pub mod workspace_dialog;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::app::App;
use crate::model::layout::Rect;
use crate::model::workspace::PaneSide;

/// SendMessageW with plain integer arguments.
pub fn send(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wp)), Some(LPARAM(lp))) }
}

/// Register the window class of a custom-drawn control.
pub fn register_class(instance: HINSTANCE, class: PCWSTR, proc: WNDPROC, cursor: HCURSOR, style: WNDCLASS_STYLES) {
    let wc = WNDCLASSW {
        style,
        hInstance: instance,
        lpszClassName: class,
        lpfnWndProc: proc,
        hCursor: cursor,
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) };
}

/// Create a zero-sized child window; the layout pass places it later.
pub fn create_child(
    parent: HWND,
    class: PCWSTR,
    text: PCWSTR,
    style: WINDOW_STYLE,
    ex: WINDOW_EX_STYLE,
    id: i32,
) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            ex,
            class,
            text,
            style,
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(id as usize as *mut _)),
            None,
            None,
        )
    }
}

pub fn set_font(hwnd: HWND, font: HFONT) {
    send(hwnd, WM_SETFONT, font.0 as usize, 1);
}

/// Select characters `start..end` of an edit control (`end` -1 selects to the end).
pub fn set_edit_sel(edit: HWND, start: usize, end: isize) {
    send(edit, EM_SETSEL, start, end);
}

/// Remember which pane a per-pane bar window belongs to.
pub fn set_bar_side(bar: HWND, side: PaneSide) {
    unsafe {
        SetWindowLongPtrW(bar, GWLP_USERDATA, side.index() as isize);
    }
}

/// The pane a per-pane bar window was created for (see `set_bar_side`).
pub fn bar_side(bar: HWND) -> PaneSide {
    if unsafe { GetWindowLongPtrW(bar, GWLP_USERDATA) } == 1 {
        PaneSide::Right
    } else {
        PaneSide::Left
    }
}

/// The `App` of a control whose parent is the main window.
pub fn parent_app(hwnd: HWND) -> Option<&'static mut App> {
    App::from_hwnd(unsafe { GetParent(hwnd).ok()? })
}

pub fn fill_rect(hdc: HDC, r: Rect, c: COLORREF) {
    unsafe {
        let br = CreateSolidBrush(c);
        FillRect(hdc, &to_win(r), br);
        let _ = DeleteObject(br.into());
    }
}

pub fn draw_text(hdc: HDC, s: &str, r: Rect, flags: DRAW_TEXT_FORMAT) {
    let mut w: Vec<u16> = s.encode_utf16().collect();
    let mut rc = to_win(r);
    unsafe {
        DrawTextW(hdc, &mut w, &mut rc, flags);
    }
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
