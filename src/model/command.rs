use super::workspace::{ColumnId, Location, PaneSide, ViewMode};

/// Every user action goes through this enum (menus, keys, tab bar, forwarded command lines).
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    NewTab,
    CloseTab,
    CloseTabAt(PaneSide, usize),
    CloseOtherTabs(PaneSide, usize),
    ReopenClosedTab,
    NextTab,
    PrevTab,
    SelectTab(u8),
    ActivateTab(PaneSide, usize),
    DuplicateTab,
    DuplicateTabAt(PaneSide, usize),
    MoveTab {
        from: (PaneSide, usize),
        to: (PaneSide, usize),
    },
    SwitchPane,
    ToggleLayout,
    ToggleNavTree,

    Back,
    Forward,
    Up,
    Refresh,
    FocusAddress,
    Navigate(Location),
    OpenInNewTab(Location),

    Open,
    OpenSelectedInNewTab,
    Rename,
    NewFolder,
    Delete {
        permanent: bool,
    },
    Copy,
    Cut,
    Paste,
    Undo,
    SelectAll,
    Properties,

    SetViewMode(ViewMode),
    SortBy(ColumnId),
    ToggleColumn(ColumnId),
    ToggleHidden,
    ToggleDotfiles,
    ToggleExtensions,

    WorkspaceList,
    WorkspaceNew {
        from_current: bool,
    },
    WorkspaceSaveAs,
    WorkspaceRename,
    WorkspaceDelete,
    WorkspaceSwitch(String),

    Exit,
}

impl Command {
    /// Parse the name used in `config.toml` `[keys]` overrides.
    pub fn from_name(name: &str) -> Option<Command> {
        use Command::*;
        Some(match name {
            "NewTab" => NewTab,
            "CloseTab" => CloseTab,
            "ReopenClosedTab" => ReopenClosedTab,
            "NextTab" => NextTab,
            "PrevTab" => PrevTab,
            "DuplicateTab" => DuplicateTab,
            "SwitchPane" => SwitchPane,
            "ToggleLayout" => ToggleLayout,
            "ToggleNavTree" => ToggleNavTree,
            "Back" => Back,
            "Forward" => Forward,
            "Up" => Up,
            "Refresh" => Refresh,
            "FocusAddress" => FocusAddress,
            "Open" => Open,
            "OpenInNewTab" => OpenSelectedInNewTab,
            "Rename" => Rename,
            "NewFolder" => NewFolder,
            "Delete" => Delete { permanent: false },
            "DeletePermanently" => Delete { permanent: true },
            "Copy" => Copy,
            "Cut" => Cut,
            "Paste" => Paste,
            "Undo" => Undo,
            "SelectAll" => SelectAll,
            "Properties" => Properties,
            "ToggleHidden" => ToggleHidden,
            "ToggleDotfiles" => ToggleDotfiles,
            "ToggleExtensions" => ToggleExtensions,
            "WorkspaceList" => WorkspaceList,
            "WorkspaceNew" => WorkspaceNew { from_current: false },
            "WorkspaceSaveAs" => WorkspaceSaveAs,
            "ViewDetails" => SetViewMode(ViewMode::Details),
            "ViewList" => SetViewMode(ViewMode::List),
            "ViewIconsLarge" => SetViewMode(ViewMode::IconsLarge),
            "ViewIconsMedium" => SetViewMode(ViewMode::IconsMedium),
            "ViewIconsSmall" => SetViewMode(ViewMode::IconsSmall),
            _ => {
                let n = name
                    .strip_prefix("SelectTab")?
                    .parse::<u8>()
                    .ok()
                    .filter(|n| (1..=9).contains(n))?;
                SelectTab(n)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(Command::from_name("NewTab"), Some(Command::NewTab));
        assert_eq!(Command::from_name("ToggleDotfiles"), Some(Command::ToggleDotfiles));
        assert_eq!(Command::from_name("SelectTab3"), Some(Command::SelectTab(3)));
        assert_eq!(Command::from_name("SelectTab0"), None);
        assert_eq!(Command::from_name("nope"), None);
    }
}
