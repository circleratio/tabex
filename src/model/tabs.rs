//! Tab operations on a workspace. All functions keep `Pane::active_tab` valid.

use super::workspace::*;

impl Workspace {
    /// Open a tab. `at = None` appends to the end of the pane.
    pub fn open_tab(&mut self, side: PaneSide, loc: Location, at: Option<usize>, activate: bool) -> TabId {
        let view = self.pane(side).active().view.clone();
        self.insert_tab(side, Tab::new(loc, view), at, activate)
    }

    fn insert_tab(&mut self, side: PaneSide, tab: Tab, at: Option<usize>, activate: bool) -> TabId {
        let id = tab.id;
        let pane = self.pane_mut(side);
        let at = at.unwrap_or(pane.tabs.len()).min(pane.tabs.len());
        pane.tabs.insert(at, tab);
        if activate {
            pane.active_tab = at;
            self.active_pane = side;
        } else if at <= pane.active_tab && pane.tabs.len() > 1 {
            pane.active_tab += 1;
        }
        id
    }

    /// Close a tab. Closing the last tab of a pane navigates it to `fallback` instead.
    pub fn close_tab(&mut self, side: PaneSide, idx: usize, fallback: &Location) -> Option<ClosedTab> {
        let pane = self.pane_mut(side);
        if idx >= pane.tabs.len() {
            return None;
        }
        let closed = {
            let t = &pane.tabs[idx];
            ClosedTab {
                side,
                index: idx,
                location: t.location.clone(),
                view: t.view.clone(),
            }
        };
        if pane.tabs.len() == 1 {
            let view = pane.tabs[0].view.clone();
            pane.tabs[0] = Tab::new(fallback.clone(), view);
        } else {
            pane.tabs.remove(idx);
            if idx < pane.active_tab || pane.active_tab >= pane.tabs.len() {
                pane.active_tab -= 1;
            }
        }
        self.closed.push(closed.clone());
        if self.closed.len() > MAX_CLOSED_TABS {
            self.closed.remove(0);
        }
        Some(closed)
    }

    pub fn reopen_closed(&mut self) -> Option<TabId> {
        let c = self.closed.pop()?;
        let tab = Tab::new(c.location, c.view);
        Some(self.insert_tab(c.side, tab, Some(c.index), true))
    }

    /// Move a tab. `to.1` is the final index in the destination pane.
    /// Moving the only tab out of a pane leaves a `fallback` tab behind.
    pub fn move_tab(&mut self, from: (PaneSide, usize), to: (PaneSide, usize), fallback: &Location) {
        let (fs, fi) = from;
        let (ts, ti) = to;
        if fi >= self.pane(fs).tabs.len() {
            return;
        }
        if fs == ts {
            let pane = self.pane_mut(fs);
            let ti = ti.min(pane.tabs.len() - 1);
            if fi == ti {
                return;
            }
            let active_id = pane.tabs[pane.active_tab].id;
            let tab = pane.tabs.remove(fi);
            pane.tabs.insert(ti, tab);
            pane.active_tab = pane.tabs.iter().position(|t| t.id == active_id).unwrap();
            return;
        }
        let tab = {
            let src = self.pane_mut(fs);
            if src.tabs.len() == 1 {
                let view = src.tabs[0].view.clone();
                std::mem::replace(&mut src.tabs[0], Tab::new(fallback.clone(), view))
            } else {
                let t = src.tabs.remove(fi);
                if fi < src.active_tab || src.active_tab >= src.tabs.len() {
                    src.active_tab -= 1;
                }
                t
            }
        };
        // A moved tab always becomes active in the destination, like browsers do.
        self.insert_tab(ts, tab, Some(ti), true);
    }

    pub fn duplicate_tab(&mut self, side: PaneSide, idx: usize) -> Option<TabId> {
        let src = self.pane(side).tabs.get(idx)?;
        let mut tab = Tab::new(src.location.clone(), src.view.clone());
        tab.history = src.history.clone();
        Some(self.insert_tab(side, tab, Some(idx + 1), true))
    }

    pub fn activate(&mut self, side: PaneSide, idx: usize) {
        let pane = self.pane_mut(side);
        if idx < pane.tabs.len() {
            pane.active_tab = idx;
            self.active_pane = side;
        }
    }

    /// Cycle the active tab of the active pane by `delta` (wraps around).
    pub fn cycle_tab(&mut self, delta: isize) {
        let side = self.active_pane;
        let pane = self.pane_mut(side);
        let n = pane.tabs.len() as isize;
        pane.active_tab = ((pane.active_tab as isize + delta).rem_euclid(n)) as usize;
    }

    /// Close every tab of the pane except `idx`.
    pub fn close_others(&mut self, side: PaneSide, idx: usize, fallback: &Location) {
        let keep = match self.pane(side).tabs.get(idx) {
            Some(t) => t.id,
            None => return,
        };
        while self.pane(side).tabs.len() > 1 {
            let i = self.pane(side).tabs.iter().rposition(|t| t.id != keep).unwrap();
            self.close_tab(side, i, fallback);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> Workspace {
        Workspace::new("t", Location::new("L0"), Location::new("R0"))
    }

    fn names(w: &Workspace, side: PaneSide) -> Vec<String> {
        w.pane(side).tabs.iter().map(|t| t.location.0.clone()).collect()
    }

    fn active_name(w: &Workspace, side: PaneSide) -> String {
        w.pane(side).active().location.0.clone()
    }

    const FB: &str = "HOME";

    fn fb() -> Location {
        Location::new(FB)
    }

    #[test]
    fn open_appends_and_activates() {
        let mut w = ws();
        w.open_tab(PaneSide::Left, Location::new("L1"), None, true);
        w.open_tab(PaneSide::Left, Location::new("L2"), None, false);
        assert_eq!(names(&w, PaneSide::Left), ["L0", "L1", "L2"]);
        assert_eq!(active_name(&w, PaneSide::Left), "L1");
    }

    #[test]
    fn open_before_active_without_activation_keeps_active() {
        let mut w = ws();
        w.open_tab(PaneSide::Left, Location::new("L1"), None, true);
        w.open_tab(PaneSide::Left, Location::new("X"), Some(0), false);
        assert_eq!(names(&w, PaneSide::Left), ["X", "L0", "L1"]);
        assert_eq!(active_name(&w, PaneSide::Left), "L1");
    }

    #[test]
    fn open_activates_pane() {
        let mut w = ws();
        w.open_tab(PaneSide::Right, Location::new("R1"), None, true);
        assert_eq!(w.active_pane, PaneSide::Right);
    }

    #[test]
    fn close_active_selects_right_neighbor() {
        let mut w = ws();
        for n in ["L1", "L2", "L3"] {
            w.open_tab(PaneSide::Left, Location::new(n), None, false);
        }
        w.activate(PaneSide::Left, 1);
        w.close_tab(PaneSide::Left, 1, &fb());
        assert_eq!(active_name(&w, PaneSide::Left), "L2");
        // Closing the last (active) tab selects the new last.
        w.activate(PaneSide::Left, 2);
        w.close_tab(PaneSide::Left, 2, &fb());
        assert_eq!(active_name(&w, PaneSide::Left), "L2");
    }

    #[test]
    fn close_before_active_shifts_index() {
        let mut w = ws();
        w.open_tab(PaneSide::Left, Location::new("L1"), None, true);
        w.close_tab(PaneSide::Left, 0, &fb());
        assert_eq!(active_name(&w, PaneSide::Left), "L1");
        assert_eq!(w.pane(PaneSide::Left).active_tab, 0);
    }

    #[test]
    fn close_last_tab_navigates_to_fallback() {
        let mut w = ws();
        w.close_tab(PaneSide::Left, 0, &fb());
        assert_eq!(names(&w, PaneSide::Left), [FB]);
    }

    #[test]
    fn reopen_restores_position() {
        let mut w = ws();
        w.open_tab(PaneSide::Left, Location::new("L1"), None, false);
        w.open_tab(PaneSide::Left, Location::new("L2"), None, false);
        w.close_tab(PaneSide::Left, 1, &fb());
        assert_eq!(names(&w, PaneSide::Left), ["L0", "L2"]);
        w.reopen_closed();
        assert_eq!(names(&w, PaneSide::Left), ["L0", "L1", "L2"]);
        assert_eq!(active_name(&w, PaneSide::Left), "L1");
        assert!(w.reopen_closed().is_none());
    }

    #[test]
    fn closed_stack_is_capped() {
        let mut w = ws();
        for i in 0..30 {
            w.open_tab(PaneSide::Left, Location::new(format!("T{i}")), None, false);
            w.close_tab(PaneSide::Left, 1, &fb());
        }
        assert_eq!(w.closed.len(), MAX_CLOSED_TABS);
        let id = w.reopen_closed().unwrap();
        assert_eq!(w.tab_mut(id).unwrap().location.0, "T29");
    }

    #[test]
    fn move_within_pane_keeps_active_tab() {
        let mut w = ws();
        for n in ["L1", "L2", "L3"] {
            w.open_tab(PaneSide::Left, Location::new(n), None, false);
        }
        w.activate(PaneSide::Left, 1); // L1
        w.move_tab((PaneSide::Left, 0), (PaneSide::Left, 3), &fb());
        assert_eq!(names(&w, PaneSide::Left), ["L1", "L2", "L3", "L0"]);
        assert_eq!(active_name(&w, PaneSide::Left), "L1");
        w.move_tab((PaneSide::Left, 3), (PaneSide::Left, 99), &fb());
        assert_eq!(names(&w, PaneSide::Left), ["L1", "L2", "L3", "L0"]);
    }

    #[test]
    fn move_across_panes() {
        let mut w = ws();
        w.open_tab(PaneSide::Left, Location::new("L1"), None, true);
        w.move_tab((PaneSide::Left, 1), (PaneSide::Right, 0), &fb());
        assert_eq!(names(&w, PaneSide::Left), ["L0"]);
        assert_eq!(names(&w, PaneSide::Right), ["L1", "R0"]);
        assert_eq!(w.active_pane, PaneSide::Right);
        assert_eq!(active_name(&w, PaneSide::Right), "L1");
        assert_eq!(active_name(&w, PaneSide::Left), "L0");
    }

    #[test]
    fn move_only_tab_leaves_fallback() {
        let mut w = ws();
        w.move_tab((PaneSide::Left, 0), (PaneSide::Right, 1), &fb());
        assert_eq!(names(&w, PaneSide::Left), [FB]);
        assert_eq!(names(&w, PaneSide::Right), ["R0", "L0"]);
    }

    #[test]
    fn duplicate_inserts_after() {
        let mut w = ws();
        w.open_tab(PaneSide::Left, Location::new("L1"), None, false);
        w.duplicate_tab(PaneSide::Left, 0);
        assert_eq!(names(&w, PaneSide::Left), ["L0", "L0", "L1"]);
        assert_eq!(w.pane(PaneSide::Left).active_tab, 1);
        let ids: Vec<_> = w.pane(PaneSide::Left).tabs.iter().map(|t| t.id).collect();
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn cycle_wraps() {
        let mut w = ws();
        w.open_tab(PaneSide::Left, Location::new("L1"), None, false);
        w.cycle_tab(1);
        assert_eq!(active_name(&w, PaneSide::Left), "L1");
        w.cycle_tab(1);
        assert_eq!(active_name(&w, PaneSide::Left), "L0");
        w.cycle_tab(-1);
        assert_eq!(active_name(&w, PaneSide::Left), "L1");
    }

    #[test]
    fn close_others_keeps_one() {
        let mut w = ws();
        for n in ["L1", "L2", "L3"] {
            w.open_tab(PaneSide::Left, Location::new(n), None, false);
        }
        w.close_others(PaneSide::Left, 2, &fb());
        assert_eq!(names(&w, PaneSide::Left), ["L2"]);
        assert_eq!(w.pane(PaneSide::Left).active_tab, 0);
    }
}
