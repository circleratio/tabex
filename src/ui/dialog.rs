//! Shared plumbing for the small modal dialogs: a popup window of its own class, child
//! controls laid out in 96-DPI units, and a modal message loop.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::send;
use crate::model::layout::scale;

/// Where a dialog goes relative to its owner.
#[derive(Clone, Copy)]
pub enum Position {
    Center,
    /// Centered horizontally, this many 96-DPI pixels below the owner's top.
    Below(i32),
}

pub struct Dialog {
    pub hwnd: HWND,
    instance: HINSTANCE,
    font: HFONT,
    pub dpi: u32,
}

impl Dialog {
    /// A popup of `class` (registered here with `proc`) whose client area is `size`
    /// 96-DPI pixels.
    pub fn create(
        owner: HWND,
        class: PCWSTR,
        proc: WNDPROC,
        title: &str,
        size: (i32, i32),
        position: Position,
        font: HFONT,
    ) -> Option<Dialog> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
            let wc = WNDCLASSW {
                hInstance: instance,
                lpszClassName: class,
                lpfnWndProc: proc,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let dpi = super::dpi::window_dpi(owner);
            let style = WS_POPUP | WS_CAPTION | WS_SYSMENU;
            let ex = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
            let mut wr = RECT {
                left: 0,
                top: 0,
                right: scale(size.0, dpi),
                bottom: scale(size.1, dpi),
            };
            let _ = AdjustWindowRectExForDpi(&mut wr, style, false, ex, dpi);
            let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
            let mut or = RECT::default();
            let _ = GetWindowRect(owner, &mut or);
            let y = match position {
                Position::Center => (or.top + or.bottom - h) / 2,
                Position::Below(dy) => or.top + scale(dy, dpi),
            };
            let hwnd = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(title),
                style,
                (or.left + or.right - w) / 2,
                y,
                w,
                h,
                Some(owner),
                None,
                Some(instance),
                None,
            )
            .ok()?;
            Some(Dialog {
                hwnd,
                instance,
                font,
                dpi,
            })
        }
    }

    /// A visible child control at `r` = (x, y, width, height) in 96-DPI pixels.
    pub fn child(
        &self,
        class: PCWSTR,
        text: &str,
        style: u32,
        ex: WINDOW_EX_STYLE,
        r: (i32, i32, i32, i32),
        id: i32,
    ) -> HWND {
        let s = |v| scale(v, self.dpi);
        unsafe {
            let c = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                s(r.0),
                s(r.1),
                s(r.2),
                s(r.3),
                Some(self.hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(self.instance),
                None,
            )
            .unwrap_or_default();
            send(c, WM_SETFONT, self.font.0 as usize, 1);
            c
        }
    }

    pub fn label(&self, text: &str, r: (i32, i32, i32, i32)) -> HWND {
        self.child(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), r, 0)
    }

    pub fn button(&self, text: &str, r: (i32, i32, i32, i32), id: i32) -> HWND {
        self.child(
            w!("BUTTON"),
            text,
            WS_TABSTOP.0 | BS_PUSHBUTTON as u32,
            WINDOW_EX_STYLE(0),
            r,
            id,
        )
    }

    /// The button Enter presses.
    pub fn default_button(&self, text: &str, r: (i32, i32, i32, i32), id: i32) -> HWND {
        self.child(
            w!("BUTTON"),
            text,
            WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32,
            WINDOW_EX_STYLE(0),
            r,
            id,
        )
    }

    /// Single-line edit box.
    pub fn edit(&self, text: &str, extra_style: u32, r: (i32, i32, i32, i32), id: i32) -> HWND {
        self.child(
            w!("EDIT"),
            text,
            WS_TABSTOP.0 | ES_AUTOHSCROLL as u32 | extra_style,
            WS_EX_CLIENTEDGE,
            r,
            id,
        )
    }
}

/// Disable `owner`, show `hwnd` and pump messages until `done()`. `intercept` sees each
/// message first and returns true when it handled it. Destroys `hwnd` at the end.
pub fn run_modal(owner: HWND, hwnd: HWND, done: impl Fn() -> bool, mut intercept: impl FnMut(&MSG) -> bool) {
    unsafe {
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut msg = MSG::default();
        loop {
            if done() {
                break;
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            if intercept(&msg) || IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(hwnd);
        let _ = SetForegroundWindow(owner);
    }
}

/// WM_CTLCOLORSTATIC for dialogs with an error line: `error` is drawn in red, every static
/// on the dialog background.
pub fn static_color(wp: WPARAM, lp: LPARAM, error: Option<HWND>) -> LRESULT {
    unsafe {
        let hdc = HDC(wp.0 as *mut _);
        if error.is_some_and(|e| e.0 == lp.0 as *mut _) {
            SetTextColor(hdc, COLORREF(0x0000_20C0));
        }
        SetBkMode(hdc, TRANSPARENT);
        LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
    }
}
