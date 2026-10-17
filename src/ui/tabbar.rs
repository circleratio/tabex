//! Custom drawn tab bar (one per pane). Reads tab state from the `App` of its parent.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::app::App;
use crate::model::command::Command;
use crate::model::layout::{Rect, scale};
use crate::model::workspace::PaneSide;

const CLASS: PCWSTR = w!("TabexTabBar");
const MIN_TAB: i32 = 80;
const MAX_TAB: i32 = 220;
const PLUS_W: i32 = 32;
const SCROLL_W: i32 = 22;
const DRAG_THRESHOLD: i32 = 6;

pub fn register(instance: HINSTANCE, cursor: HCURSOR) -> Result<()> {
    let wc = WNDCLASSW {
        style: CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW,
        hInstance: instance,
        lpszClassName: CLASS,
        lpfnWndProc: Some(proc_),
        hCursor: cursor,
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) };
    Ok(())
}

pub fn create(parent: HWND, side: PaneSide) -> Result<HWND> {
    unsafe {
        let h = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            None,
            None,
        )?;
        SetWindowLongPtrW(h, GWLP_USERDATA, side.index() as isize);
        Ok(h)
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

/// Scroll offset (first visible tab) per tab bar, kept in a window property.
fn scroll(hwnd: HWND) -> usize {
    unsafe { GetPropW(hwnd, w!("scroll")).0 as usize }
}

fn set_scroll(hwnd: HWND, v: usize) {
    unsafe {
        let _ = SetPropW(hwnd, w!("scroll"), Some(HANDLE(v as *mut _)));
    }
}

struct Geometry {
    tabs: Vec<(usize, Rect)>,
    plus: Rect,
    scroll_left: Option<Rect>,
    scroll_right: Option<Rect>,
}

fn geometry(hwnd: HWND, app: &App) -> Geometry {
    let side = side_of(hwnd);
    let client = super::client_rect(hwnd);
    let s = |v| scale(v, app.dpi);
    let n = app.ws.pane(side).tabs.len();
    let top = client.top + s(4);
    let avail = client.width() - s(PLUS_W) - s(4);
    let mut w = if n == 0 {
        s(MAX_TAB)
    } else {
        (avail / n as i32).clamp(s(MIN_TAB), s(MAX_TAB))
    };
    let overflow = w * n as i32 > avail;
    let mut x = client.left + s(2);
    let mut right_limit = client.right - s(PLUS_W);
    let (mut sl, mut sr) = (None, None);
    let mut first = 0;
    if overflow {
        right_limit -= 2 * s(SCROLL_W);
        w = s(MIN_TAB).max(w);
        let fit = ((right_limit - x) / w).max(1) as usize;
        let active = app.ws.pane(side).active_tab;
        let mut off = scroll(hwnd).min(n.saturating_sub(fit));
        if active < off {
            off = active;
        } else if active >= off + fit {
            off = active + 1 - fit;
        }
        set_scroll(hwnd, off);
        first = off;
        sl = Some(Rect::new(right_limit, top, right_limit + s(SCROLL_W), client.bottom));
        sr = Some(Rect::new(
            right_limit + s(SCROLL_W),
            top,
            right_limit + 2 * s(SCROLL_W),
            client.bottom,
        ));
    }
    let mut tabs = Vec::new();
    for i in first..n {
        if x + w > right_limit + 1 && !tabs.is_empty() {
            break;
        }
        tabs.push((i, Rect::new(x, top, x + w, client.bottom)));
        x += w;
    }
    let plus_left = if overflow { client.right - s(PLUS_W) } else { x + s(2) };
    Geometry {
        tabs,
        plus: Rect::new(plus_left, top, plus_left + s(PLUS_W) - s(4), client.bottom - s(2)),
        scroll_left: sl,
        scroll_right: sr,
    }
}

fn close_rect(tab: Rect, dpi: u32) -> Rect {
    let s = |v| scale(v, dpi);
    let cy = (tab.top + tab.bottom) / 2;
    Rect::new(tab.right - s(24), cy - s(9), tab.right - s(6), cy + s(9))
}

enum Hit {
    Tab(usize),
    Close(usize),
    Plus,
    ScrollLeft,
    ScrollRight,
    Empty,
}

fn hit(hwnd: HWND, app: &App, x: i32, y: i32) -> Hit {
    let g = geometry(hwnd, app);
    if g.scroll_left.is_some_and(|r| r.contains(x, y)) {
        return Hit::ScrollLeft;
    }
    if g.scroll_right.is_some_and(|r| r.contains(x, y)) {
        return Hit::ScrollRight;
    }
    if g.plus.contains(x, y) {
        return Hit::Plus;
    }
    for (i, r) in g.tabs {
        if r.contains(x, y) {
            return if close_rect(r, app.dpi).contains(x, y) {
                Hit::Close(i)
            } else {
                Hit::Tab(i)
            };
        }
    }
    Hit::Empty
}

/// Insertion index for a drop at client x.
fn drop_index(hwnd: HWND, app: &App, x: i32) -> usize {
    let g = geometry(hwnd, app);
    for (i, r) in &g.tabs {
        if x < (r.left + r.right) / 2 {
            return *i;
        }
    }
    g.tabs.last().map(|(i, _)| i + 1).unwrap_or(0)
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn fill(hdc: HDC, r: Rect, c: COLORREF) {
    unsafe {
        let br = CreateSolidBrush(c);
        FillRect(hdc, &super::to_win(r), br);
        let _ = DeleteObject(br.into());
    }
}

fn text(hdc: HDC, s: &str, r: Rect, flags: DRAW_TEXT_FORMAT) {
    let mut w: Vec<u16> = s.encode_utf16().collect();
    let mut rc = super::to_win(r);
    unsafe {
        DrawTextW(hdc, &mut w, &mut rc, flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
    }
}

fn paint(hwnd: HWND, app: &App) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let real = BeginPaint(hwnd, &mut ps);
        let client = super::client_rect(hwnd);
        let mem = CreateCompatibleDC(Some(real));
        let bmp = CreateCompatibleBitmap(real, client.width().max(1), client.height().max(1));
        let old_bmp = SelectObject(mem, bmp.into());
        let old_font = SelectObject(mem, app.font.into());
        SetBkMode(mem, TRANSPARENT);

        let side = side_of(hwnd);
        let pane_active = app.ws.active_pane == side && app.ws.layout == crate::model::workspace::Layout::Dual
            || app.ws.layout == crate::model::workspace::Layout::Single;
        let face = GetSysColor(COLOR_BTNFACE);
        fill(mem, client, COLORREF(face));
        let g = geometry(hwnd, app);
        let pane = app.ws.pane(side);
        let s = |v| scale(v, app.dpi);
        let accent = rgb(0x1a, 0x73, 0xe8);
        for (i, r) in &g.tabs {
            let tab = &pane.tabs[*i];
            let active = *i == pane.active_tab;
            let body = Rect::new(r.left + 1, r.top, r.right - 1, r.bottom);
            if active {
                fill(mem, body, COLORREF(GetSysColor(COLOR_WINDOW)));
                if pane_active {
                    fill(mem, Rect::new(body.left, body.top, body.right, body.top + s(3)), accent);
                }
            } else {
                fill(
                    mem,
                    Rect::new(body.right - 1, body.top + s(6), body.right, body.bottom - s(6)),
                    rgb(0xc0, 0xc0, 0xc0),
                );
            }
            let title = app
                .rt
                .get(&tab.id)
                .map(|r| r.title.clone())
                .unwrap_or_else(|| tab.location.display_name().to_string());
            let loading = app
                .rt
                .get(&tab.id)
                .is_some_and(|r| r.pending.is_some() || r.listing.as_ref().is_some_and(|l| !l.complete));
            let missing = matches!(
                tab.status,
                crate::model::workspace::TabStatus::NotFound | crate::model::workspace::TabStatus::Error(_)
            );
            SetTextColor(
                mem,
                if missing {
                    rgb(0xb0, 0x20, 0x20)
                } else {
                    COLORREF(GetSysColor(COLOR_BTNTEXT))
                },
            );
            let label = if loading { format!("{title} …") } else { title };
            text(
                mem,
                &label,
                Rect::new(body.left + s(10), body.top, body.right - s(26), body.bottom),
                DT_LEFT | DT_END_ELLIPSIS,
            );
            SetTextColor(mem, rgb(0x70, 0x70, 0x70));
            text(mem, "×", close_rect(*r, app.dpi), DT_CENTER);
        }
        SetTextColor(mem, COLORREF(GetSysColor(COLOR_BTNTEXT)));
        text(mem, "+", g.plus, DT_CENTER);
        if let Some(r) = g.scroll_left {
            text(mem, "‹", r, DT_CENTER);
        }
        if let Some(r) = g.scroll_right {
            text(mem, "›", r, DT_CENTER);
        }
        // Insertion marker while a tab is being dragged onto this bar.
        if let Some(d) = &app.tab_drag
            && let (true, Some((ts, ti))) = (d.active, d.target)
            && ts == side
        {
            let x = g
                .tabs
                .iter()
                .find(|(i, _)| *i == ti)
                .map(|(_, r)| r.left)
                .or_else(|| g.tabs.last().map(|(_, r)| r.right))
                .unwrap_or(client.left + s(2));
            fill(
                mem,
                Rect::new(x - s(1), client.top + s(2), x + s(2), client.bottom),
                accent,
            );
        }
        let _ = BitBlt(real, 0, 0, client.width(), client.height(), Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_font);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn tab_menu(hwnd: HWND, app: &mut App, idx: usize, x: i32, y: i32) {
    let side = side_of(hwnd);
    let other = if side == PaneSide::Left { "右" } else { "左" };
    let to_end = app.ws.pane(side.other()).tabs.len();
    let items = vec![
        Some(("タブを閉じる".to_string(), Command::CloseTabAt(side, idx), false)),
        Some((
            "他のタブを閉じる".to_string(),
            Command::CloseOtherTabs(side, idx),
            false,
        )),
        Some(("タブを複製".to_string(), Command::DuplicateTabAt(side, idx), false)),
        None,
        Some((
            format!("{other}のペインへ移動"),
            Command::MoveTab {
                from: (side, idx),
                to: (side.other(), to_end),
            },
            false,
        )),
    ];
    let mut pt = POINT { x, y };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut pt);
    }
    if let Some(cmd) = app.menu.track(hwnd, items, pt.x, pt.y) {
        app.execute(cmd);
    }
}

/// Which tab bar (and insertion index) is under the cursor, in screen coordinates.
fn drop_target(app: &App, sx: i32, sy: i32) -> Option<(PaneSide, usize)> {
    for side in PaneSide::BOTH {
        let bar = app.panes[side.index()].tabbar;
        let mut rc = RECT::default();
        unsafe {
            if !IsWindowVisible(bar).as_bool() || GetWindowRect(bar, &mut rc).is_err() {
                continue;
            }
        }
        // Accept drops a little below the bar as well.
        let slack = scale(16, app.dpi);
        if sx >= rc.left && sx < rc.right && sy >= rc.top - slack && sy < rc.bottom + slack {
            let mut pt = POINT { x: sx, y: sy };
            unsafe {
                let _ = ScreenToClient(bar, &mut pt);
            }
            return Some((side, drop_index(bar, app, pt.x)));
        }
    }
    None
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
            WM_LBUTTONDOWN => {
                let (x, y) = super::point_from_lparam(lp);
                app.set_active_pane(side);
                match hit(hwnd, app, x, y) {
                    Hit::Tab(i) => {
                        app.execute(Command::ActivateTab(side, i));
                        app.tab_drag = Some(crate::app::TabDrag {
                            from: (side, i),
                            start: (x, y),
                            active: false,
                            target: None,
                        });
                        SetCapture(hwnd);
                    }
                    Hit::Close(i) => app.execute(Command::CloseTabAt(side, i)),
                    Hit::Plus => app.execute(Command::NewTab),
                    Hit::ScrollLeft => {
                        set_scroll(hwnd, scroll(hwnd).saturating_sub(1));
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    Hit::ScrollRight => {
                        set_scroll(hwnd, scroll(hwnd) + 1);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    Hit::Empty => {}
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (x, y) = super::point_from_lparam(lp);
                if let Some(d) = app.tab_drag.as_mut() {
                    if !d.active && ((x - d.start.0).abs() > DRAG_THRESHOLD || (y - d.start.1).abs() > DRAG_THRESHOLD) {
                        d.active = true;
                    }
                    if d.active {
                        let mut pt = POINT { x, y };
                        let _ = ClientToScreen(hwnd, &mut pt);
                        let target = drop_target(app, pt.x, pt.y);
                        if let Some(d) = app.tab_drag.as_mut() {
                            d.target = target;
                        }
                        SetCursor(LoadCursorW(None, if target.is_some() { IDC_ARROW } else { IDC_NO }).ok());
                        app.invalidate_chrome();
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if let Some(d) = app.tab_drag.take() {
                    let _ = ReleaseCapture();
                    if d.active {
                        if let Some((ts, mut ti)) = d.target {
                            // Indices after removal of the dragged tab from the same pane.
                            if ts == d.from.0 && ti > d.from.1 {
                                ti -= 1;
                            }
                            if (ts, ti) != d.from {
                                app.execute(Command::MoveTab {
                                    from: d.from,
                                    to: (ts, ti),
                                });
                            }
                        }
                        app.invalidate_chrome();
                    }
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                if app.tab_drag.take().is_some() {
                    app.invalidate_chrome();
                }
                LRESULT(0)
            }
            WM_MBUTTONUP => {
                let (x, y) = super::point_from_lparam(lp);
                if let Hit::Tab(i) | Hit::Close(i) = hit(hwnd, app, x, y) {
                    app.execute(Command::CloseTabAt(side, i));
                }
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                let (x, y) = super::point_from_lparam(lp);
                match hit(hwnd, app, x, y) {
                    Hit::Tab(i) => app.execute(Command::DuplicateTabAt(side, i)),
                    Hit::Empty => {
                        app.set_active_pane(side);
                        app.execute(Command::NewTab);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let (x, y) = super::point_from_lparam(lp);
                if let Hit::Tab(i) | Hit::Close(i) = hit(hwnd, app, x, y) {
                    tab_menu(hwnd, app, i, x, y);
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = super::hiword(wp.0) as i16;
                app.set_active_pane(side);
                app.execute(if delta > 0 { Command::PrevTab } else { Command::NextTab });
                LRESULT(0)
            }
            _ => {
                let _ = (VK_LBUTTON, ());
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
    }
}

/// Activate the tab under the cursor (used while dragging files over a tab bar).
pub fn tab_at_screen(app: &App, sx: i32, sy: i32) -> Option<(PaneSide, usize)> {
    for side in PaneSide::BOTH {
        let bar = app.panes[side.index()].tabbar;
        let mut pt = POINT { x: sx, y: sy };
        unsafe {
            if !IsWindowVisible(bar).as_bool() {
                continue;
            }
            let _ = ScreenToClient(bar, &mut pt);
        }
        if super::client_rect(bar).contains(pt.x, pt.y)
            && let Hit::Tab(i) | Hit::Close(i) = hit(bar, app, pt.x, pt.y)
        {
            return Some((side, i));
        }
    }
    None
}
