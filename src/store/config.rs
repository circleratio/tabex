use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{StoreError, write_atomic};

pub const VERSION: i64 = 1;
pub const MAX_RECENT: usize = 10;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub maximized: bool,
}

fn yes() -> bool {
    true
}

fn version() -> i64 {
    VERSION
}

fn default_nav_width() -> i32 {
    crate::model::layout::DEFAULT_NAV_WIDTH
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Config {
    #[serde(default = "version")]
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_workspace: Option<String>,
    #[serde(default)]
    pub recent_workspaces: Vec<String>,
    #[serde(default)]
    pub show_hidden: bool,
    /// Names starting with '.' (hidden by default, UNIX convention).
    #[serde(default)]
    pub show_dotfiles: bool,
    #[serde(default = "yes")]
    pub show_extensions: bool,
    #[serde(default = "yes")]
    pub nav_tree_visible: bool,
    /// Whether the "ワークスペース" node of the navigation tree is expanded.
    #[serde(default = "yes")]
    pub nav_workspaces_expanded: bool,
    #[serde(default = "default_nav_width")]
    pub nav_width: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowPlacement>,
    /// Key overrides, e.g. `"Ctrl+Alt+W" = "WorkspaceList"`.
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl Config {
    /// Move `name` to the front of the recent list.
    pub fn touch_recent(&mut self, name: &str) {
        self.recent_workspaces.retain(|n| !n.eq_ignore_ascii_case(name));
        self.recent_workspaces.insert(0, name.to_string());
        self.recent_workspaces.truncate(MAX_RECENT);
        self.last_workspace = Some(name.to_string());
    }

    pub fn forget(&mut self, name: &str) {
        self.recent_workspaces.retain(|n| !n.eq_ignore_ascii_case(name));
        if self
            .last_workspace
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
        {
            self.last_workspace = self.recent_workspaces.first().cloned();
        }
    }

    pub fn rename(&mut self, old: &str, new: &str) {
        for n in self.recent_workspaces.iter_mut().chain(self.last_workspace.iter_mut()) {
            if n.eq_ignore_ascii_case(old) {
                *n = new.to_string();
            }
        }
    }
}

pub fn from_toml(text: &str, path: &Path) -> Result<Config, StoreError> {
    let cfg: Config = toml::from_str(text).map_err(|e| StoreError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    if cfg.version != VERSION {
        return Err(StoreError::UnsupportedVersion {
            path: path.to_path_buf(),
            version: cfg.version,
            supported: VERSION,
        });
    }
    Ok(cfg)
}

/// A missing file yields the defaults; any other failure is an error.
pub fn load(path: &Path) -> Result<Config, StoreError> {
    match std::fs::read_to_string(path) {
        Ok(text) => from_toml(&text, path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(StoreError::io(path, e)),
    }
}

pub fn save(path: &Path, cfg: &Config) -> Result<(), StoreError> {
    write_atomic(path, &toml::to_string(cfg).expect("config is always serializable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let c = Config::default();
        assert_eq!(c.version, 1);
        assert!(c.show_extensions);
        assert!(c.nav_tree_visible);
        assert!(c.nav_workspaces_expanded);
        assert!(!c.show_hidden);
        assert!(!c.show_dotfiles);
        assert!(c.last_workspace.is_none());
    }

    #[test]
    fn round_trip_and_keys() {
        let text = r#"
version = 1
last_workspace = "dev"
recent_workspaces = ["dev", "photo"]
window = { x = 100, y = 100, width = 1400, height = 900, maximized = false }

[keys]
"Ctrl+Alt+W" = "WorkspaceList"
"#;
        let c = from_toml(text, Path::new("c.toml")).unwrap();
        assert_eq!(c.keys.get("Ctrl+Alt+W").map(String::as_str), Some("WorkspaceList"));
        assert_eq!(c.window.unwrap().width, 1400);
        let again = from_toml(&toml::to_string(&c).unwrap(), Path::new("c.toml")).unwrap();
        assert_eq!(c, again);
    }

    #[test]
    fn recent_list() {
        let mut c = Config::default();
        for i in 0..12 {
            c.touch_recent(&format!("w{i}"));
        }
        assert_eq!(c.recent_workspaces.len(), MAX_RECENT);
        assert_eq!(c.recent_workspaces[0], "w11");
        c.touch_recent("W5");
        assert_eq!(c.recent_workspaces[0], "W5");
        assert_eq!(
            c.recent_workspaces
                .iter()
                .filter(|n| n.eq_ignore_ascii_case("w5"))
                .count(),
            1
        );
        c.forget("w5");
        assert_eq!(c.last_workspace.as_deref(), Some("w11"));
        c.rename("w11", "z");
        assert_eq!(c.recent_workspaces[0], "z");
        assert_eq!(c.last_workspace.as_deref(), Some("z"));
    }

    #[test]
    fn bad_version() {
        assert!(matches!(
            from_toml("version = 9", Path::new("c.toml")),
            Err(StoreError::UnsupportedVersion { version: 9, .. })
        ));
    }
}
