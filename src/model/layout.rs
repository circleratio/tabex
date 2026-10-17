//! Window geometry computation (pure).

use super::workspace::{Layout, PaneSide, clamp_ratio};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Rect {
            left,
            top,
            right: right.max(left),
            bottom: bottom.max(top),
        }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// Sizes in 96-DPI pixels.
pub const TABBAR_HEIGHT: i32 = 32;
pub const ADDRESS_HEIGHT: i32 = 32;
pub const STATUS_HEIGHT: i32 = 24;
pub const FILTERBAR_HEIGHT: i32 = 28;
pub const SPLITTER_WIDTH: i32 = 6;
pub const DEFAULT_NAV_WIDTH: i32 = 220;
pub const MIN_NAV_WIDTH: i32 = 80;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutState {
    pub layout: Layout,
    pub split_ratio: f32,
    pub active_pane: PaneSide,
    pub nav_visible: bool,
    /// 96-DPI pixels.
    pub nav_width: i32,
    /// Narrowing input shown under the list, indexed by `PaneSide::index()`.
    pub filter_bar: [bool; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaneRects {
    pub tabbar: Rect,
    pub address: Rect,
    pub list: Rect,
    /// Narrowing input under the list, when shown.
    pub filter: Option<Rect>,
    pub whole: Rect,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutRects {
    pub nav: Option<Rect>,
    pub nav_splitter: Option<Rect>,
    /// Indexed by `PaneSide::index()`; `None` when hidden in single layout.
    pub panes: [Option<PaneRects>; 2],
    pub pane_splitter: Option<Rect>,
    pub status: Rect,
}

pub fn scale(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

pub fn compute(client: Rect, dpi: u32, st: &LayoutState) -> LayoutRects {
    let status_h = scale(STATUS_HEIGHT, dpi);
    let status = Rect::new(client.left, client.bottom - status_h, client.right, client.bottom);
    let body = Rect::new(client.left, client.top, client.right, status.top);
    let split_w = scale(SPLITTER_WIDTH, dpi);

    let mut out = LayoutRects {
        status,
        ..Default::default()
    };
    let mut x = body.left;
    if st.nav_visible {
        let max_nav = (body.width() / 2).max(0);
        let nav_w = scale(st.nav_width.max(MIN_NAV_WIDTH), dpi).min(max_nav);
        out.nav = Some(Rect::new(x, body.top, x + nav_w, body.bottom));
        out.nav_splitter = Some(Rect::new(x + nav_w, body.top, x + nav_w + split_w, body.bottom));
        x += nav_w + split_w;
    }
    let area = Rect::new(x, body.top, body.right, body.bottom);
    match st.layout {
        Layout::Single => {
            let i = st.active_pane.index();
            out.panes[i] = Some(pane_rects(area, dpi, st.filter_bar[i]));
        }
        Layout::Dual => {
            let avail = (area.width() - split_w).max(0);
            let left_w = (avail as f32 * clamp_ratio(st.split_ratio)).round() as i32;
            let left = Rect::new(area.left, area.top, area.left + left_w, area.bottom);
            let splitter = Rect::new(left.right, area.top, left.right + split_w, area.bottom);
            let right = Rect::new(splitter.right, area.top, area.right, area.bottom);
            out.panes[0] = Some(pane_rects(left, dpi, st.filter_bar[0]));
            out.panes[1] = Some(pane_rects(right, dpi, st.filter_bar[1]));
            out.pane_splitter = Some(splitter);
        }
    }
    out
}

fn pane_rects(r: Rect, dpi: u32, filter_bar: bool) -> PaneRects {
    let tab_h = scale(TABBAR_HEIGHT, dpi);
    let addr_h = scale(ADDRESS_HEIGHT, dpi);
    let tabbar = Rect::new(r.left, r.top, r.right, r.top + tab_h);
    let address = Rect::new(r.left, tabbar.bottom, r.right, tabbar.bottom + addr_h);
    let filter = filter_bar.then(|| {
        let top = (r.bottom - scale(FILTERBAR_HEIGHT, dpi)).max(address.bottom);
        Rect::new(r.left, top, r.right, r.bottom)
    });
    let list = Rect::new(r.left, address.bottom, r.right, filter.map_or(r.bottom, |f| f.top));
    PaneRects {
        tabbar,
        address,
        list,
        filter,
        whole: r,
    }
}

/// New split ratio when the pane splitter is dragged to client x coordinate `x`.
pub fn ratio_from_drag(client: Rect, dpi: u32, st: &LayoutState, x: i32) -> f32 {
    let rects = compute(
        client,
        dpi,
        &LayoutState {
            layout: Layout::Dual,
            ..*st
        },
    );
    let left = rects.panes[0].unwrap().whole.left;
    let right = rects.panes[1].unwrap().whole.right;
    let avail = (right - left - scale(SPLITTER_WIDTH, dpi)).max(1);
    clamp_ratio((x - left) as f32 / avail as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(layout: Layout, nav: bool) -> LayoutState {
        LayoutState {
            layout,
            split_ratio: 0.5,
            active_pane: PaneSide::Right,
            nav_visible: nav,
            nav_width: 200,
            filter_bar: [false; 2],
        }
    }

    const CLIENT: Rect = Rect {
        left: 0,
        top: 0,
        right: 1206,
        bottom: 800,
    };

    #[test]
    fn dual_without_nav() {
        let r = compute(CLIENT, 96, &st(Layout::Dual, false));
        assert!(r.nav.is_none());
        let l = r.panes[0].unwrap();
        let rr = r.panes[1].unwrap();
        assert_eq!(l.whole.left, 0);
        assert_eq!(l.whole.width(), 600);
        assert_eq!(rr.whole.left, 606);
        assert_eq!(rr.whole.right, 1206);
        assert_eq!(r.status, Rect::new(0, 776, 1206, 800));
        assert_eq!(l.list.top, TABBAR_HEIGHT + ADDRESS_HEIGHT);
        assert_eq!(l.list.bottom, 776);
    }

    #[test]
    fn single_shows_only_active_pane() {
        let r = compute(CLIENT, 96, &st(Layout::Single, true));
        assert!(r.panes[0].is_none());
        let p = r.panes[1].unwrap();
        assert_eq!(p.whole.left, 200 + SPLITTER_WIDTH);
        assert_eq!(p.whole.right, 1206);
        assert!(r.pane_splitter.is_none());
    }

    #[test]
    fn ratio_out_of_range_is_clamped() {
        let mut s = st(Layout::Dual, false);
        s.split_ratio = 5.0;
        let r = compute(CLIENT, 96, &s);
        assert_eq!(r.panes[0].unwrap().whole.width(), 1080);
    }

    #[test]
    fn scales_with_dpi() {
        let r = compute(Rect::new(0, 0, 3000, 1500), 144, &st(Layout::Dual, true));
        assert_eq!(r.nav.unwrap().width(), 300);
        assert_eq!(r.status.height(), 36);
        assert_eq!(r.panes[0].unwrap().tabbar.height(), 48);
    }

    #[test]
    fn nav_never_exceeds_half() {
        let mut s = st(Layout::Dual, true);
        s.nav_width = 5000;
        let r = compute(CLIENT, 96, &s);
        assert_eq!(r.nav.unwrap().width(), 603);
    }

    #[test]
    fn tiny_window_does_not_panic() {
        let r = compute(Rect::new(0, 0, 10, 10), 96, &st(Layout::Dual, true));
        assert!(r.panes[0].unwrap().list.height() >= 0);
    }

    #[test]
    fn filter_bar_shrinks_only_its_pane() {
        let mut s = st(Layout::Dual, false);
        s.filter_bar = [true, false];
        let r = compute(CLIENT, 96, &s);
        let l = r.panes[0].unwrap();
        let rr = r.panes[1].unwrap();
        assert_eq!(l.filter, Some(Rect::new(0, 776 - FILTERBAR_HEIGHT, 600, 776)));
        assert_eq!(l.list.bottom, 776 - FILTERBAR_HEIGHT);
        assert!(rr.filter.is_none());
        assert_eq!(rr.list.bottom, 776);
        // Single layout uses the flag of the pane that is shown.
        let mut s = st(Layout::Single, false);
        s.filter_bar = [false, true];
        let p = compute(CLIENT, 144, &s).panes[1].unwrap();
        assert_eq!(p.filter.unwrap().height(), 42);
        // Never overlaps the address bar in a tiny window.
        let p = compute(Rect::new(0, 0, 100, 80), 96, &s).panes[1].unwrap();
        assert!(p.filter.unwrap().top >= p.address.bottom);
        assert!(p.list.height() >= 0);
    }

    #[test]
    fn drag_ratio() {
        let s = st(Layout::Dual, false);
        assert!((ratio_from_drag(CLIENT, 96, &s, 300) - 0.25).abs() < 1e-3);
        assert_eq!(ratio_from_drag(CLIENT, 96, &s, -50), 0.1);
    }
}
