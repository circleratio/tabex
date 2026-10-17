use std::sync::atomic::{AtomicU64, Ordering};

use super::history::History;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneSide {
    Left,
    Right,
}

impl PaneSide {
    pub fn index(self) -> usize {
        match self {
            PaneSide::Left => 0,
            PaneSide::Right => 1,
        }
    }

    pub fn other(self) -> PaneSide {
        match self {
            PaneSide::Left => PaneSide::Right,
            PaneSide::Right => PaneSide::Left,
        }
    }

    pub const BOTH: [PaneSide; 2] = [PaneSide::Left, PaneSide::Right];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Single,
    Dual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Details,
    List,
    IconsLarge,
    IconsMedium,
    IconsSmall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnId {
    Name,
    Modified,
    Created,
    Type,
    Size,
    Attributes,
    Extension,
}

impl ColumnId {
    pub const ALL: [ColumnId; 7] = [
        ColumnId::Name,
        ColumnId::Modified,
        ColumnId::Created,
        ColumnId::Type,
        ColumnId::Size,
        ColumnId::Attributes,
        ColumnId::Extension,
    ];

    /// Default width in 96-DPI pixels.
    pub fn default_width(self) -> u32 {
        match self {
            ColumnId::Name => 320,
            ColumnId::Modified | ColumnId::Created => 150,
            ColumnId::Type => 140,
            ColumnId::Size => 100,
            ColumnId::Attributes => 80,
            ColumnId::Extension => 80,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub column: ColumnId,
    pub order: SortOrder,
}

impl Default for SortSpec {
    fn default() -> Self {
        SortSpec {
            column: ColumnId::Name,
            order: SortOrder::Asc,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    pub id: ColumnId,
    /// Width in 96-DPI pixels.
    pub width: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewState {
    pub mode: ViewMode,
    pub sort: SortSpec,
    pub columns: Vec<ColumnSpec>,
}

impl ViewState {
    pub fn default_columns() -> Vec<ColumnSpec> {
        [ColumnId::Name, ColumnId::Modified, ColumnId::Type, ColumnId::Size]
            .into_iter()
            .map(|id| ColumnSpec {
                id,
                width: id.default_width(),
            })
            .collect()
    }
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            mode: ViewMode::Details,
            sort: SortSpec::default(),
            columns: Self::default_columns(),
        }
    }
}

/// A shell parsing name: a file system path, a UNC path, `shell:...` or `::{CLSID}`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location(pub String);

impl Location {
    pub fn new(s: impl Into<String>) -> Self {
        Location(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True for plain drive or UNC paths that can be enumerated with the Win32 file API.
    pub fn is_file_system(&self) -> bool {
        let b = self.0.as_bytes();
        (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':') || self.0.starts_with("\\\\")
    }

    /// Parent location for plain paths. `None` at a drive root or for shell locations.
    pub fn parent(&self) -> Option<Location> {
        if !self.is_file_system() {
            return None;
        }
        let s = self.0.trim_end_matches('\\');
        let idx = s.rfind('\\')?;
        let parent = &s[..idx];
        if parent.len() == 2 && parent.as_bytes()[1] == b':' {
            // "C:\foo" -> "C:\"
            return Some(Location(format!("{parent}\\")));
        }
        if self.0.starts_with("\\\\") {
            // Do not walk above "\\server\share".
            if parent.trim_start_matches('\\').matches('\\').count() < 1 {
                return None;
            }
        }
        if parent.is_empty() {
            return None;
        }
        Some(Location(parent.to_string()))
    }

    /// Breadcrumb segments as (location, label), root first. Shell locations have none.
    pub fn segments(&self) -> Vec<(Location, String)> {
        if !self.is_file_system() {
            return Vec::new();
        }
        let s = self.0.trim_end_matches('\\');
        let mut out = Vec::new();
        let (root, rest) = if let Some(unc) = s.strip_prefix("\\\\") {
            // "\\server\share" is the root of a UNC path.
            let mut it = unc.splitn(3, '\\');
            let server = it.next().unwrap_or("");
            let share = it.next();
            match share {
                Some(sh) => (format!("\\\\{server}\\{sh}"), it.next().unwrap_or("")),
                None => (format!("\\\\{server}"), ""),
            }
        } else {
            let (drive, rest) = s.split_at(2.min(s.len()));
            (format!("{drive}\\"), rest.trim_start_matches('\\'))
        };
        out.push((Location(root.clone()), root.trim_end_matches('\\').to_string()));
        let mut cur = root.trim_end_matches('\\').to_string();
        for part in rest.split('\\').filter(|p| !p.is_empty()) {
            cur = format!("{cur}\\{part}");
            out.push((Location(cur.clone()), part.to_string()));
        }
        out
    }

    /// Last path component used as tab title.
    pub fn display_name(&self) -> &str {
        let s = self.0.trim_end_matches('\\');
        match s.rfind('\\') {
            Some(i) if i + 1 < s.len() => &s[i + 1..],
            _ if s.is_empty() => &self.0,
            _ => s,
        }
    }
}

pub type TabId = u64;

static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_tab_id() -> TabId {
    NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabStatus {
    Loading,
    Ready,
    NotFound,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Tab {
    pub id: TabId,
    pub location: Location,
    pub view: ViewState,
    pub history: History,
    pub status: TabStatus,
    /// Narrowing text of the list (runtime only, never saved). `Some("")` = input open.
    pub narrow: Option<String>,
}

impl Tab {
    pub fn new(location: Location, view: ViewState) -> Self {
        Tab {
            id: next_tab_id(),
            location,
            view,
            history: History::default(),
            status: TabStatus::Loading,
            narrow: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pane {
    /// Invariant: never empty.
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
}

impl Pane {
    pub fn new(first: Tab) -> Self {
        Pane {
            tabs: vec![first],
            active_tab: 0,
        }
    }

    pub fn active(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    pub fn active_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }
}

#[derive(Clone, Debug)]
pub struct ClosedTab {
    pub side: PaneSide,
    pub index: usize,
    pub location: Location,
    pub view: ViewState,
}

pub const MAX_CLOSED_TABS: usize = 20;

#[derive(Clone, Debug)]
pub struct Workspace {
    pub name: String,
    pub panes: [Pane; 2],
    pub active_pane: PaneSide,
    pub layout: Layout,
    pub split_ratio: f32,
    /// Runtime only: stack of recently closed tabs, newest last.
    pub closed: Vec<ClosedTab>,
}

impl Workspace {
    pub fn new(name: impl Into<String>, left: Location, right: Location) -> Self {
        Workspace {
            name: name.into(),
            panes: [
                Pane::new(Tab::new(left, ViewState::default())),
                Pane::new(Tab::new(right, ViewState::default())),
            ],
            active_pane: PaneSide::Left,
            layout: Layout::Dual,
            split_ratio: 0.5,
            closed: Vec::new(),
        }
    }

    pub fn pane(&self, side: PaneSide) -> &Pane {
        &self.panes[side.index()]
    }

    pub fn pane_mut(&mut self, side: PaneSide) -> &mut Pane {
        &mut self.panes[side.index()]
    }

    pub fn active_tab(&self) -> &Tab {
        self.pane(self.active_pane).active()
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        let side = self.active_pane;
        self.pane_mut(side).active_mut()
    }

    pub fn find_tab(&self, id: TabId) -> Option<(PaneSide, usize)> {
        PaneSide::BOTH
            .into_iter()
            .find_map(|side| self.pane(side).tabs.iter().position(|t| t.id == id).map(|i| (side, i)))
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        let (side, idx) = self.find_tab(id)?;
        Some(&mut self.pane_mut(side).tabs[idx])
    }

    /// Tabs currently shown on screen (one per visible pane).
    pub fn visible_tabs(&self) -> Vec<TabId> {
        match self.layout {
            Layout::Single => vec![self.active_tab().id],
            Layout::Dual => PaneSide::BOTH.iter().map(|s| self.pane(*s).active().id).collect(),
        }
    }
}

pub fn clamp_ratio(r: f32) -> f32 {
    if r.is_nan() { 0.5 } else { r.clamp(0.1, 0.9) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_of_paths() {
        assert_eq!(Location::new("C:\\a\\b").parent(), Some(Location::new("C:\\a")));
        assert_eq!(Location::new("C:\\a").parent(), Some(Location::new("C:\\")));
        assert_eq!(Location::new("C:\\").parent(), None);
        assert_eq!(
            Location::new("\\\\srv\\share\\x").parent(),
            Some(Location::new("\\\\srv\\share"))
        );
        assert_eq!(Location::new("\\\\srv\\share").parent(), None);
        assert_eq!(Location::new("shell:RecycleBinFolder").parent(), None);
    }

    #[test]
    fn display_names() {
        assert_eq!(Location::new("C:\\a\\bb").display_name(), "bb");
        assert_eq!(Location::new("C:\\a\\bb\\").display_name(), "bb");
        assert_eq!(Location::new("C:\\").display_name(), "C:");
        assert_eq!(Location::new("shell:Downloads").display_name(), "shell:Downloads");
    }

    #[test]
    fn breadcrumb_segments() {
        let seg = |s: &str| -> Vec<(String, String)> {
            Location::new(s).segments().into_iter().map(|(l, n)| (l.0, n)).collect()
        };
        assert_eq!(
            seg("C:\\a\\b"),
            [
                ("C:\\".into(), "C:".into()),
                ("C:\\a".into(), "a".into()),
                ("C:\\a\\b".into(), "b".into())
            ]
        );
        assert_eq!(seg("C:\\"), [("C:\\".to_string(), "C:".to_string())]);
        assert_eq!(
            seg("\\\\srv\\share\\x"),
            [
                ("\\\\srv\\share".into(), "\\\\srv\\share".into()),
                ("\\\\srv\\share\\x".into(), "x".into())
            ]
        );
        assert!(seg("shell:Downloads").is_empty());
    }

    #[test]
    fn ratio_is_clamped() {
        assert_eq!(clamp_ratio(0.0), 0.1);
        assert_eq!(clamp_ratio(1.5), 0.9);
        assert_eq!(clamp_ratio(f32::NAN), 0.5);
        assert_eq!(clamp_ratio(0.3), 0.3);
    }
}
