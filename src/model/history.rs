use super::workspace::Location;

pub const MAX_HISTORY: usize = 100;

/// Per-tab back/forward history. The current location is held by the tab, not here.
#[derive(Clone, Debug, Default)]
pub struct History {
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl History {
    /// Record a navigation away from `current`.
    pub fn push(&mut self, current: Location) {
        push_capped(&mut self.back, current);
        self.forward.clear();
    }

    pub fn back(&mut self, current: Location) -> Option<Location> {
        let target = self.back.pop()?;
        push_capped(&mut self.forward, current);
        Some(target)
    }

    pub fn forward(&mut self, current: Location) -> Option<Location> {
        let target = self.forward.pop()?;
        push_capped(&mut self.back, current);
        Some(target)
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

fn push_capped(v: &mut Vec<Location>, loc: Location) {
    if v.last() == Some(&loc) {
        return;
    }
    v.push(loc);
    if v.len() > MAX_HISTORY {
        v.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(s: &str) -> Location {
        Location::new(s)
    }

    #[test]
    fn back_and_forward() {
        let mut h = History::default();
        h.push(l("A"));
        h.push(l("B"));
        // current = C
        assert_eq!(h.back(l("C")), Some(l("B")));
        assert_eq!(h.back(l("B")), Some(l("A")));
        assert_eq!(h.back(l("A")), None);
        assert_eq!(h.forward(l("A")), Some(l("B")));
        assert_eq!(h.forward(l("B")), Some(l("C")));
        assert!(!h.can_forward());
    }

    #[test]
    fn new_navigation_clears_forward() {
        let mut h = History::default();
        h.push(l("A"));
        assert_eq!(h.back(l("B")), Some(l("A")));
        assert!(h.can_forward());
        h.push(l("A"));
        assert!(!h.can_forward());
    }

    #[test]
    fn capped_at_max() {
        let mut h = History::default();
        for i in 0..150 {
            h.push(l(&i.to_string()));
        }
        let mut n = 0;
        let mut cur = l("cur");
        while let Some(prev) = h.back(cur.clone()) {
            cur = prev;
            n += 1;
        }
        assert_eq!(n, MAX_HISTORY);
        assert_eq!(cur, l("50"));
    }
}
