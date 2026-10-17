//! Navigation buttons, breadcrumb and path entry for one pane.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::app::App;
use crate::model::command::Command;
use crate::model::layout::{Rect, scale};
use crate::model::workspace::{Location, PaneSide};

const CLASS: PCWSTR = w!("TabexAddressBar");
const BTN_W: i32 = 30;
const SUBCLASS_ID: usize = 1;

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

/// Returns (address bar window, edit control).
pub fn create(parent: HWND, side: PaneSide, edit_id: i32, font: HFONT) -> Result<(HWND, HWND)> {
    unsafe {
        let bar = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
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
            WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
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
        let _ = SHAutoComplete(edit, SHACF_FILESYS_DIRS | SHACF_USETAB);
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

fn app_of(hwnd: HWND) -> Option<&'static mut App> {
    App::from_hwnd(unsafe { GetParent(hwnd).ok()? })
}

struct Geometry {
    back: Rect,
    forward: Rect,
    up: Rect,
    field: Rect,
}

fn geometry(hwnd: HWND, dpi: u32) -> Geometry {
    let c = super::client_rect(hwnd);
    let s = |v| scale(v, dpi);
    let top = c.top + s(3);
    let bottom = c.bottom - s(3);
    let b = |i: i32| {
        Rect::new(
            c.left + s(2) + i * s(BTN_W),
            top,
            c.left + s(2) + (i + 1) * s(BTN_W),
            bottom,
        )
    };
    Geometry {
        back: b(0),
        forward: b(1),
        up: b(2),
        field: Rect::new(c.left + s(4) + 3 * s(BTN_W), top, c.right - s(4), bottom),
    }
}

/// Breadcrumb segments that fit, with their rectangles (leading ones dropped if needed).
fn crumbs(hdc: HDC, app: &App, side: PaneSide, field: Rect) -> Vec<(Location, String, Rect)> {
    let Some(id) = app.panes[side.index()].shown else {
        return Vec::new();
    };
    let Some(tab) = app.tab(id) else { return Vec::new() };
    let mut segs = tab.location.segments();
    if segs.is_empty() {
        let title = app
            .rt
            .get(&id)
            .map(|r| r.title.clone())
            .unwrap_or_else(|| tab.location.0.clone());
        segs.push((tab.location.clone(), title));
    }
    let s = |v| scale(v, app.dpi);
    let widths: Vec<i32> = segs
        .iter()
        .map(|(_, label)| {
            let w: Vec<u16> = label.encode_utf16().collect();
            let mut sz = SIZE::default();
            unsafe {
                let _ = GetTextExtentPoint32W(hdc, &w, &mut sz);
            }
            sz.cx + s(16)
        })
        .collect();
    let sep = s(14);
    let avail = field.width() - s(8);
    let mut first = 0;
    while first + 1 < segs.len() && widths[first..].iter().sum::<i32>() + sep * (segs.len() - first) as i32 > avail {
        first += 1;
    }
    let mut x = field.left + s(4);
    let mut out = Vec::new();
    for (i, (loc, label)) in segs.into_iter().enumerate().skip(first) {
        let r = Rect::new(x, field.top, (x + widths[i]).min(field.right), field.bottom);
        out.push((loc, label, r));
        x = r.right + sep;
    }
    out
}

fn paint(hwnd: HWND, app: &App) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let side = side_of(hwnd);
        let client = super::client_rect(hwnd);
        let g = geometry(hwnd, app.dpi);
        let face = CreateSolidBrush(COLORREF(GetSysColor(COLOR_BTNFACE)));
        FillRect(hdc, &super::to_win(client), face);
        let _ = DeleteObject(face.into());
        let old = SelectObject(hdc, app.font.into());
        SetBkMode(hdc, TRANSPARENT);
        let tab = app.panes[side.index()].shown.and_then(|id| app.tab(id));
        let can_back = tab.is_some_and(|t| t.history.can_back());
        let can_fwd = tab.is_some_and(|t| t.history.can_forward());
        let can_up = tab.is_some_and(|t| app.parent_of(&t.location).is_some());
        for (r, glyph, enabled) in [(g.back, "←", can_back), (g.forward, "→", can_fwd), (g.up, "↑", can_up)] {
            SetTextColor(
                hdc,
                COLORREF(GetSysColor(if enabled { COLOR_BTNTEXT } else { COLOR_GRAYTEXT })),
            );
            let mut w: Vec<u16> = glyph.encode_utf16().collect();
            let mut rc = super::to_win(r);
            DrawTextW(hdc, &mut w, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        // Field background and border (red after a failed path entry).
        let field = super::to_win(g.field);
        let bg = CreateSolidBrush(COLORREF(GetSysColor(COLOR_WINDOW)));
        FillRect(hdc, &field, bg);
        let _ = DeleteObject(bg.into());
        let border = if app.panes[side.index()].addr_error {
            COLORREF(0x0000_20D0)
        } else {
            COLORREF(GetSysColor(COLOR_BTNSHADOW))
        };
        let pen_brush = CreateSolidBrush(border);
        FrameRect(hdc, &field, pen_brush);
        let _ = DeleteObject(pen_brush.into());
        if !IsWindowVisible(app.panes[side.index()].edit).as_bool() {
            SetTextColor(hdc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
            let list = crumbs(hdc, app, side, g.field);
            let sep_w = scale(14, app.dpi);
            for (i, (_, label, r)) in list.iter().enumerate() {
                let mut w: Vec<u16> = label.encode_utf16().collect();
                let mut rc = super::to_win(*r);
                DrawTextW(
                    hdc,
                    &mut w,
                    &mut rc,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                let is_fs = app.panes[side.index()]
                    .shown
                    .and_then(|id| app.tab(id))
                    .is_some_and(|t| t.location.is_file_system());
                if i + 1 < list.len() || (list.len() == 1 && is_fs) {
                    let mut sep: Vec<u16> = "›".encode_utf16().collect();
                    let mut rc = super::to_win(Rect::new(r.right, r.top, r.right + sep_w, r.bottom));
                    SetTextColor(hdc, COLORREF(GetSysColor(COLOR_GRAYTEXT)));
                    DrawTextW(hdc, &mut sep, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                    SetTextColor(hdc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
                }
            }
        }
        SelectObject(hdc, old);
        let _ = EndPaint(hwnd, &ps);
    }
}

pub fn begin_edit(app: &mut App, side: PaneSide) {
    let p = &app.panes[side.index()];
    let Some(id) = p.shown else { return };
    let Some(tab) = app.tab(id) else { return };
    let text = tab.location.0.clone();
    let g = geometry(p.address, app.dpi);
    let s = |v| scale(v, app.dpi);
    let f = g.field;
    unsafe {
        let _ = SetWindowTextW(p.edit, &HSTRING::from(text));
        let _ = SetWindowPos(
            p.edit,
            None,
            f.left + s(4),
            f.top + s(4),
            f.width() - s(8),
            f.height() - s(7),
            SWP_NOZORDER | SWP_SHOWWINDOW,
        );
        let _ = SetFocus(Some(p.edit));
        SendMessageW(p.edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        let _ = InvalidateRect(Some(p.address), None, false);
    }
    app.set_active_pane(side);
}

fn end_edit(bar: HWND, commit: bool) {
    let Some(app) = app_of(bar) else { return };
    let side = side_of(bar);
    let edit = app.panes[side.index()].edit;
    if commit {
        let text = super::window_text(edit);
        match crate::shell::names::resolve(&text) {
            Some(loc) => {
                app.panes[side.index()].addr_error = false;
                unsafe {
                    let _ = ShowWindow(edit, SW_HIDE);
                }
                app.set_active_pane(side);
                app.execute(Command::Navigate(loc));
                app.focus_active_list();
            }
            None => {
                app.panes[side.index()].addr_error = true;
                app.show_status(&format!("フォルダが見つかりません: {text}"));
                unsafe {
                    let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONWARNING);
                    let _ = InvalidateRect(Some(bar), None, false);
                }
                return;
            }
        }
    } else {
        app.panes[side.index()].addr_error = false;
        unsafe {
            let _ = ShowWindow(edit, SW_HIDE);
        }
    }
    unsafe {
        let _ = InvalidateRect(Some(bar), None, false);
    }
}

/// Drop-down of subfolders shown when a breadcrumb separator is clicked.
fn subfolder_menu(bar: HWND, app: &mut App, loc: &Location, x: i32, y: i32) {
    let Ok(rd) = std::fs::read_dir(loc.as_str()) else {
        return;
    };
    let mut dirs: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .take(500)
        .collect();
    dirs.sort_by(|a, b| crate::fs::sort::compare_names(&crate::fs::wide(a), &crate::fs::wide(b)));
    if dirs.is_empty() {
        return;
    }
    let items = dirs
        .iter()
        .map(|d| {
            Some((
                d.clone(),
                Command::Navigate(Location::new(crate::fs::join(loc.as_str(), d))),
                false,
            ))
        })
        .collect();
    let mut pt = POINT { x, y };
    unsafe {
        let _ = ClientToScreen(bar, &mut pt);
    }
    if let Some(cmd) = app.menu.track(bar, items, pt.x, pt.y) {
        app.set_active_pane(side_of(bar));
        app.execute(cmd);
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
                paint(hwnd, app);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_LBUTTONUP => {
                let (x, y) = super::point_from_lparam(lp);
                let g = geometry(hwnd, app.dpi);
                app.set_active_pane(side);
                if g.back.contains(x, y) {
                    app.execute(Command::Back);
                } else if g.forward.contains(x, y) {
                    app.execute(Command::Forward);
                } else if g.up.contains(x, y) {
                    app.execute(Command::Up);
                } else if g.field.contains(x, y) {
                    let hdc = GetDC(Some(hwnd));
                    let old = SelectObject(hdc, app.font.into());
                    let list = crumbs(hdc, app, side, g.field);
                    SelectObject(hdc, old);
                    ReleaseDC(Some(hwnd), hdc);
                    let sep_w = scale(14, app.dpi);
                    if let Some((loc, _, _)) = list.iter().find(|(_, _, r)| r.contains(x, y)) {
                        let loc = loc.clone();
                        let current = app.panes[side.index()]
                            .shown
                            .and_then(|id| app.tab(id))
                            .map(|t| t.location.clone());
                        if current.as_ref() != Some(&loc) {
                            app.execute(Command::Navigate(loc));
                        } else {
                            begin_edit(app, side);
                        }
                    } else if let Some((loc, _, r)) = list
                        .iter()
                        .find(|(_, _, r)| x >= r.right && x < r.right + sep_w && y >= r.top && y < r.bottom)
                    {
                        let (loc, bottom) = (loc.clone(), r.bottom);
                        subfolder_menu(hwnd, app, &loc, x, bottom);
                    } else {
                        begin_edit(app, side);
                    }
                }
                LRESULT(0)
            }
            WM_SIZE => {
                if IsWindowVisible(app.panes[side.index()].edit).as_bool() {
                    begin_edit(app, side);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, bar: usize) -> LRESULT {
    unsafe {
        let bar = HWND(bar as *mut _);
        match msg {
            WM_KEYDOWN if VIRTUAL_KEY(wp.0 as u16) == VK_RETURN => {
                end_edit(bar, true);
                LRESULT(0)
            }
            WM_KEYDOWN if VIRTUAL_KEY(wp.0 as u16) == VK_ESCAPE => {
                end_edit(bar, false);
                if let Some(app) = app_of(bar) {
                    app.focus_active_list();
                }
                LRESULT(0)
            }
            WM_CHAR if wp.0 == 0x0D || wp.0 == 0x1B => LRESULT(0),
            WM_KILLFOCUS => {
                let r = DefSubclassProc(hwnd, msg, wp, lp);
                end_edit(bar, false);
                r
            }
            WM_GETDLGCODE => LRESULT(DLGC_WANTALLKEYS as isize),
            _ => DefSubclassProc(hwnd, msg, wp, lp),
        }
    }
}
