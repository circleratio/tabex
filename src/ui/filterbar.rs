//! Narrowing input under a file list: "/" prompt, edit control and "n / m 件" count.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::app::App;
use crate::model::layout::{Rect, scale};
use crate::model::workspace::PaneSide;

const CLASS: PCWSTR = w!("TabexFilterBar");
const SUBCLASS_ID: usize = 1;
/// Width of the "/" prompt and of the count area, 96-DPI pixels.
const PROMPT_W: i32 = 18;
const COUNT_W: i32 = 120;
/// Width of the "migemo" label left of the count.
const MIGEMO_W: i32 = 56;

pub fn register(instance: HINSTANCE, cursor: HCURSOR) -> Result<()> {
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        hInstance: instance,
        lpszClassName: CLASS,
        lpfnWndProc: Some(proc_),
        hCursor: cursor,
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) };
    Ok(())
}

/// Returns (bar window, edit control). Both start hidden.
pub fn create(parent: HWND, side: PaneSide, edit_id: i32, font: HFONT) -> Result<(HWND, HWND)> {
    unsafe {
        let bar = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            PCWSTR::null(),
            WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            None,
            None,
        )?;
        SetWindowLongPtrW(bar, GWLP_USERDATA, side.index() as isize);
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            0,
            0,
            0,
            0,
            Some(bar),
            Some(HMENU(edit_id as usize as *mut _)),
            None,
            None,
        )?;
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        let _ = SetWindowSubclass(edit, Some(edit_proc), SUBCLASS_ID, bar.0 as usize);
        Ok((bar, edit))
    }
}

fn side_of(hwnd: HWND) -> PaneSide {
    if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 1 {
        PaneSide::Right
    } else {
        PaneSide::Left
    }
}

fn app_of(bar: HWND) -> Option<&'static mut App> {
    App::from_hwnd(unsafe { GetParent(bar).ok()? })
}

pub fn text(edit: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(edit, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..got])
    }
}

/// Replace the edit text and put the caret at the end.
pub fn set_text(edit: HWND, s: &str) {
    unsafe {
        let _ = SetWindowTextW(edit, &HSTRING::from(s));
    }
    caret_to_end(edit);
}

pub fn caret_to_end(edit: HWND) {
    unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(n)), Some(LPARAM(n as isize)));
    }
}

struct Geometry {
    prompt: Rect,
    edit: Rect,
    count: Rect,
    migemo: Option<Rect>,
}

fn geometry(hwnd: HWND, dpi: u32, font_h: i32, migemo: bool) -> Geometry {
    let c = super::client_rect(hwnd);
    let s = |v| scale(v, dpi);
    // Leave a 1px separator line at the top.
    let top = c.top + 1;
    let prompt = Rect::new(c.left + s(4), top, c.left + s(4) + s(PROMPT_W), c.bottom);
    let count = Rect::new((c.right - s(COUNT_W)).max(prompt.right), top, c.right - s(6), c.bottom);
    let migemo = migemo.then(|| Rect::new((count.left - s(MIGEMO_W)).max(prompt.right), top, count.left, c.bottom));
    let edit_right = migemo.map_or(count.left, |m| m.left) - s(4);
    let edit_top = top + ((c.bottom - top) - font_h).max(0) / 2;
    let edit = Rect::new(prompt.right, edit_top, edit_right, edit_top + font_h);
    Geometry {
        prompt,
        edit,
        count,
        migemo,
    }
}

fn font_height(hwnd: HWND, font: HFONT) -> i32 {
    unsafe {
        let hdc = GetDC(Some(hwnd));
        let old = SelectObject(hdc, font.into());
        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        SelectObject(hdc, old);
        ReleaseDC(Some(hwnd), hdc);
        tm.tmHeight + 2
    }
}

/// Position the edit control inside the bar (call after the bar is resized, the font changes
/// or the "migemo" label appears or disappears).
pub fn layout(bar: HWND, edit: HWND, dpi: u32, font: HFONT, migemo: bool) {
    let g = geometry(bar, dpi, font_height(bar, font), migemo);
    super::place(edit, g.edit, true);
}

fn paint(hwnd: HWND, app: &App, side: PaneSide) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let c = super::client_rect(hwnd);
        let rc = super::to_win(c);
        FillRect(hdc, &rc, GetSysColorBrush(COLOR_WINDOW));
        let line = RECT {
            bottom: rc.top + 1,
            ..rc
        };
        FillRect(hdc, &line, GetSysColorBrush(COLOR_3DSHADOW));
        let old = SelectObject(hdc, app.font.into());
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
        let g = geometry(hwnd, app.dpi, font_height(hwnd, app.font), app.migemo_active(side));
        let mut prompt = super::to_win(g.prompt);
        let command = app.command_line[side.index()];
        let mut mark: Vec<u16> = if command { ":" } else { "/" }.encode_utf16().collect();
        DrawTextW(hdc, &mut mark, &mut prompt, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
        if !command && let Some((n, m)) = app.narrow_counts(side) {
            let mut count = super::to_win(g.count);
            let mut s: Vec<u16> = format!("{n} / {m} 件").encode_utf16().collect();
            SetTextColor(hdc, COLORREF(GetSysColor(COLOR_GRAYTEXT)));
            DrawTextW(hdc, &mut s, &mut count, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);
        }
        if let Some(m) = g.migemo {
            let mut r = super::to_win(m);
            let mut s: Vec<u16> = "migemo".encode_utf16().collect();
            SetTextColor(hdc, COLORREF(GetSysColor(COLOR_GRAYTEXT)));
            DrawTextW(hdc, &mut s, &mut r, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);
        }
        SelectObject(hdc, old);
        let _ = EndPaint(hwnd, &ps);
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(app) = app_of(hwnd) else {
            return DefWindowProcW(hwnd, msg, wp, lp);
        };
        let side = side_of(hwnd);
        match msg {
            WM_PAINT => {
                paint(hwnd, app, side);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                let migemo = app.migemo_active(side);
                layout(hwnd, app.panes[side.index()].filter_edit, app.dpi, app.font, migemo);
                LRESULT(0)
            }
            WM_COMMAND => {
                if super::hiword(wp.0) as u32 == EN_CHANGE {
                    app.on_narrow_changed(side);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // Clicking anywhere on the bar edits the narrowing text.
                let _ = SetFocus(Some(app.panes[side.index()].filter_edit));
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                let hdc = HDC(wp.0 as *mut _);
                SetBkColor(hdc, COLORREF(GetSysColor(COLOR_WINDOW)));
                SetTextColor(hdc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
                LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, bar: usize) -> LRESULT {
    unsafe {
        let bar = HWND(bar as *mut _);
        let side = side_of(bar);
        let empty = || GetWindowTextLengthW(hwnd) == 0;
        match msg {
            WM_KEYDOWN => {
                let Some(app) = app_of(bar) else {
                    return DefSubclassProc(hwnd, msg, wp, lp);
                };
                let command = app.command_line[side.index()];
                match VIRTUAL_KEY(wp.0 as u16) {
                    VK_RETURN if command => app.command_line_enter(side),
                    VK_RETURN => app.narrow_commit(side),
                    VK_ESCAPE if command => app.command_line_close(side),
                    VK_ESCAPE => app.clear_narrow(side),
                    VK_BACK if empty() && command => app.command_line_close(side),
                    VK_BACK if empty() => app.clear_narrow(side),
                    VK_UP | VK_DOWN if command => {}
                    VK_UP => app.narrow_cursor(side, -1),
                    VK_DOWN => app.narrow_cursor(side, 1),
                    _ => return DefSubclassProc(hwnd, msg, wp, lp),
                }
                LRESULT(0)
            }
            // Swallow the characters of keys handled above so the edit does not beep.
            WM_CHAR if wp.0 == 0x0D || wp.0 == 0x1B || (wp.0 == 0x08 && empty()) => LRESULT(0),
            WM_SETFOCUS => {
                if let Some(app) = app_of(bar) {
                    app.set_active_pane(side);
                }
                DefSubclassProc(hwnd, msg, wp, lp)
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTALLKEYS as isize),
            _ => DefSubclassProc(hwnd, msg, wp, lp),
        }
    }
}
