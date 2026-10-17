//! TOML representation of a workspace. DTOs mirror the file layout; the model never sees serde.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{StoreError, write_atomic};
use crate::model::workspace::*;

pub const VERSION: i64 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SideDto {
    #[default]
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum LayoutDto {
    Single,
    #[default]
    Dual,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ViewDto {
    #[default]
    Details,
    List,
    IconsLarge,
    IconsMedium,
    IconsSmall,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ColumnDtoId {
    Name,
    Modified,
    Created,
    Type,
    Size,
    Attributes,
    Extension,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum OrderDto {
    #[default]
    Asc,
    Desc,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
struct SortDto {
    column: ColumnDtoId,
    #[serde(default)]
    order: OrderDto,
}

impl Default for SortDto {
    fn default() -> Self {
        SortDto {
            column: ColumnDtoId::Name,
            order: OrderDto::Asc,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct ColumnDto {
    id: ColumnDtoId,
    width: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct TabDto {
    path: String,
    #[serde(default)]
    view: ViewDto,
    #[serde(default)]
    sort: SortDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    columns: Option<Vec<ColumnDto>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
struct PaneDto {
    #[serde(default)]
    active_tab: usize,
    #[serde(default)]
    tabs: Vec<TabDto>,
}

fn default_ratio() -> f32 {
    0.5
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct WorkspaceDto {
    version: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    active_pane: SideDto,
    #[serde(default)]
    layout: LayoutDto,
    #[serde(default = "default_ratio")]
    split_ratio: f32,
    #[serde(default)]
    left: PaneDto,
    #[serde(default)]
    right: PaneDto,
}

#[derive(Deserialize)]
struct VersionProbe {
    version: Option<i64>,
}

fn col_to_model(c: ColumnDtoId) -> ColumnId {
    match c {
        ColumnDtoId::Name => ColumnId::Name,
        ColumnDtoId::Modified => ColumnId::Modified,
        ColumnDtoId::Created => ColumnId::Created,
        ColumnDtoId::Type => ColumnId::Type,
        ColumnDtoId::Size => ColumnId::Size,
        ColumnDtoId::Attributes => ColumnId::Attributes,
        ColumnDtoId::Extension => ColumnId::Extension,
    }
}

fn col_to_dto(c: ColumnId) -> ColumnDtoId {
    match c {
        ColumnId::Name => ColumnDtoId::Name,
        ColumnId::Modified => ColumnDtoId::Modified,
        ColumnId::Created => ColumnDtoId::Created,
        ColumnId::Type => ColumnDtoId::Type,
        ColumnId::Size => ColumnDtoId::Size,
        ColumnId::Attributes => ColumnDtoId::Attributes,
        ColumnId::Extension => ColumnDtoId::Extension,
    }
}

fn view_to_model(t: &TabDto) -> ViewState {
    let mode = match t.view {
        ViewDto::Details => ViewMode::Details,
        ViewDto::List => ViewMode::List,
        ViewDto::IconsLarge => ViewMode::IconsLarge,
        ViewDto::IconsMedium => ViewMode::IconsMedium,
        ViewDto::IconsSmall => ViewMode::IconsSmall,
    };
    let sort = SortSpec {
        column: col_to_model(t.sort.column),
        order: match t.sort.order {
            OrderDto::Asc => SortOrder::Asc,
            OrderDto::Desc => SortOrder::Desc,
        },
    };
    let mut columns: Vec<ColumnSpec> = match &t.columns {
        Some(cols) => {
            let mut seen = Vec::new();
            cols.iter()
                .filter(|c| {
                    let fresh = !seen.contains(&c.id);
                    seen.push(c.id);
                    fresh
                })
                .map(|c| ColumnSpec {
                    id: col_to_model(c.id),
                    width: c.width.clamp(20, 2000),
                })
                .collect()
        }
        None => ViewState::default_columns(),
    };
    // The name column is mandatory and always first.
    if let Some(i) = columns.iter().position(|c| c.id == ColumnId::Name) {
        let c = columns.remove(i);
        columns.insert(0, c);
    } else {
        columns.insert(
            0,
            ColumnSpec {
                id: ColumnId::Name,
                width: ColumnId::Name.default_width(),
            },
        );
    }
    ViewState { mode, sort, columns }
}

fn view_to_dto(path: &str, v: &ViewState) -> TabDto {
    TabDto {
        path: path.to_string(),
        view: match v.mode {
            ViewMode::Details => ViewDto::Details,
            ViewMode::List => ViewDto::List,
            ViewMode::IconsLarge => ViewDto::IconsLarge,
            ViewMode::IconsMedium => ViewDto::IconsMedium,
            ViewMode::IconsSmall => ViewDto::IconsSmall,
        },
        sort: SortDto {
            column: col_to_dto(v.sort.column),
            order: match v.sort.order {
                SortOrder::Asc => OrderDto::Asc,
                SortOrder::Desc => OrderDto::Desc,
            },
        },
        columns: Some(
            v.columns
                .iter()
                .map(|c| ColumnDto {
                    id: col_to_dto(c.id),
                    width: c.width,
                })
                .collect(),
        ),
    }
}

fn pane_to_model(p: PaneDto, fallback: &Location) -> Pane {
    let mut tabs: Vec<Tab> = p
        .tabs
        .iter()
        .filter(|t| !t.path.trim().is_empty())
        .map(|t| Tab::new(Location::new(t.path.clone()), view_to_model(t)))
        .collect();
    if tabs.is_empty() {
        tabs.push(Tab::new(fallback.clone(), ViewState::default()));
    }
    let active_tab = p.active_tab.min(tabs.len() - 1);
    Pane { tabs, active_tab }
}

fn pane_to_dto(p: &Pane) -> PaneDto {
    PaneDto {
        active_tab: p.active_tab,
        tabs: p
            .tabs
            .iter()
            .map(|t| view_to_dto(t.location.as_str(), &t.view))
            .collect(),
    }
}

/// Parse workspace TOML. `name` (the file stem) wins over the `name` key inside the file.
pub fn from_toml(text: &str, name: &str, fallback: &Location, path: &Path) -> Result<Workspace, StoreError> {
    let parse_err = |e: toml::de::Error| StoreError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    };
    let probe: VersionProbe = toml::from_str(text).map_err(parse_err)?;
    match probe.version {
        None => {
            return Err(StoreError::Parse {
                path: path.to_path_buf(),
                message: "version がありません".into(),
            });
        }
        Some(v) if v != VERSION => {
            return Err(StoreError::UnsupportedVersion {
                path: path.to_path_buf(),
                version: v,
                supported: VERSION,
            });
        }
        _ => {}
    }
    let dto: WorkspaceDto = toml::from_str(text).map_err(parse_err)?;
    Ok(Workspace {
        name: name.to_string(),
        panes: [pane_to_model(dto.left, fallback), pane_to_model(dto.right, fallback)],
        active_pane: match dto.active_pane {
            SideDto::Left => PaneSide::Left,
            SideDto::Right => PaneSide::Right,
        },
        layout: match dto.layout {
            LayoutDto::Single => Layout::Single,
            LayoutDto::Dual => Layout::Dual,
        },
        split_ratio: clamp_ratio(dto.split_ratio),
        closed: Vec::new(),
    })
}

pub fn to_toml(ws: &Workspace) -> String {
    let dto = WorkspaceDto {
        version: VERSION,
        name: ws.name.clone(),
        active_pane: match ws.active_pane {
            PaneSide::Left => SideDto::Left,
            PaneSide::Right => SideDto::Right,
        },
        layout: match ws.layout {
            Layout::Single => LayoutDto::Single,
            Layout::Dual => LayoutDto::Dual,
        },
        split_ratio: (ws.split_ratio * 1000.0).round() / 1000.0,
        left: pane_to_dto(&ws.panes[0]),
        right: pane_to_dto(&ws.panes[1]),
    };
    toml::to_string(&dto).expect("workspace DTO is always serializable")
}

pub fn load(path: &Path, name: &str, fallback: &Location) -> Result<Workspace, StoreError> {
    let text = std::fs::read_to_string(path).map_err(|e| StoreError::io(path, e))?;
    from_toml(&text, name, fallback, path)
}

pub fn save(path: &Path, ws: &Workspace) -> Result<(), StoreError> {
    write_atomic(path, &to_toml(ws))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from doc/requirement.md, verbatim.
    pub const EXAMPLE: &str = r#"
version = 1
name = "dev"
active_pane = "left"
layout = "dual"        # "single" | "dual"
split_ratio = 0.5

[left]
active_tab = 0

[[left.tabs]]
path = 'C:\src\project'
view = "details"       # "details" | "list" | "icons_large" | "icons_medium" | "icons_small"
sort = { column = "name", order = "asc" }
columns = [
  { id = "name", width = 320 },
  { id = "modified", width = 160 },
  { id = "type", width = 120 },
  { id = "size", width = 100 },
]

[[left.tabs]]
path = 'C:\src\project\docs'
view = "list"
sort = { column = "modified", order = "desc" }

[right]
active_tab = 0

[[right.tabs]]
path = 'D:\data'
view = "details"
sort = { column = "name", order = "asc" }
"#;

    fn fb() -> Location {
        Location::new("C:\\Users\\me")
    }

    fn p() -> &'static Path {
        Path::new("test.toml")
    }

    /// Comparable projection of a workspace (ignores runtime-only ids/history).
    fn shape(ws: &Workspace) -> String {
        let mut s = format!("{}|{:?}|{:?}|{}\n", ws.name, ws.active_pane, ws.layout, ws.split_ratio);
        for pane in &ws.panes {
            s += &format!("active={}\n", pane.active_tab);
            for t in &pane.tabs {
                s += &format!("  {} {:?}\n", t.location.0, t.view);
            }
        }
        s
    }

    #[test]
    fn reads_requirement_example() {
        let ws = from_toml(EXAMPLE, "dev", &fb(), p()).unwrap();
        assert_eq!(ws.name, "dev");
        assert_eq!(ws.layout, Layout::Dual);
        let left = &ws.panes[0];
        assert_eq!(left.tabs.len(), 2);
        assert_eq!(left.tabs[0].location.0, "C:\\src\\project");
        assert_eq!(
            left.tabs[0].view.columns[1],
            ColumnSpec {
                id: ColumnId::Modified,
                width: 160
            }
        );
        assert_eq!(left.tabs[1].view.mode, ViewMode::List);
        assert_eq!(
            left.tabs[1].view.sort,
            SortSpec {
                column: ColumnId::Modified,
                order: SortOrder::Desc
            }
        );
        // Missing columns fall back to the defaults.
        assert_eq!(left.tabs[1].view.columns, ViewState::default_columns());
        assert_eq!(ws.panes[1].tabs[0].location.0, "D:\\data");
    }

    #[test]
    fn round_trip() {
        let ws = from_toml(EXAMPLE, "dev", &fb(), p()).unwrap();
        let text = to_toml(&ws);
        let again = from_toml(&text, "dev", &fb(), p()).unwrap();
        assert_eq!(shape(&ws), shape(&again));
    }

    #[test]
    fn minimal_file_gets_defaults() {
        let ws = from_toml("version = 1\n", "x", &fb(), p()).unwrap();
        assert_eq!(ws.panes[0].tabs.len(), 1);
        assert_eq!(ws.panes[0].tabs[0].location, fb());
        assert_eq!(ws.panes[1].tabs[0].location, fb());
        assert_eq!(ws.layout, Layout::Dual);
        assert_eq!(ws.split_ratio, 0.5);
        assert_eq!(ws.panes[0].tabs[0].view, ViewState::default());
    }

    #[test]
    fn file_stem_wins_over_inner_name() {
        let ws = from_toml("version = 1\nname = \"other\"\n", "dev", &fb(), p()).unwrap();
        assert_eq!(ws.name, "dev");
    }

    #[test]
    fn out_of_range_values_are_fixed() {
        let text = "version = 1\nsplit_ratio = 3.0\n[left]\nactive_tab = 9\n[[left.tabs]]\npath='C:\\\\a'\ncolumns=[{id=\"size\",width=5},{id=\"size\",width=50}]\n";
        let ws = from_toml(text, "x", &fb(), p()).unwrap();
        assert_eq!(ws.split_ratio, 0.9);
        assert_eq!(ws.panes[0].active_tab, 0);
        let cols = &ws.panes[0].tabs[0].view.columns;
        assert_eq!(cols[0].id, ColumnId::Name);
        assert_eq!(
            cols[1],
            ColumnSpec {
                id: ColumnId::Size,
                width: 20
            }
        );
        assert_eq!(cols.len(), 2);
    }

    #[test]
    fn unknown_version_is_rejected() {
        let e = from_toml("version = 2\n[left]\ntabs = 3\n", "x", &fb(), p()).unwrap_err();
        assert!(matches!(e, StoreError::UnsupportedVersion { version: 2, .. }));
    }

    #[test]
    fn missing_version_is_rejected() {
        let e = from_toml("name = \"x\"\n", "x", &fb(), p()).unwrap_err();
        assert!(matches!(e, StoreError::Parse { .. }));
    }

    #[test]
    fn syntax_error_is_reported() {
        let e = from_toml("xxx", "x", &fb(), p()).unwrap_err();
        assert!(matches!(e, StoreError::Parse { .. }));
        let e = from_toml("version = 1\nlayout = \"triple\"\n", "x", &fb(), p()).unwrap_err();
        assert!(matches!(e, StoreError::Parse { .. }));
    }
}
