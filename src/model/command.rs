use super::workspace::{ColumnId, Location, PaneSide, ViewMode};

/// Every user action goes through this enum (toolbar, popup menus, keys, tab bar, forwarded
/// command lines).
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
    /// Go to the primary home directory.
    GoHome,
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
    /// Move the selection one row down / up (vi-style j / k).
    CursorDown,
    CursorUp,
    /// Extend the selection one row down / up (Shift+J / Shift+K).
    ExtendSelectionDown,
    ExtendSelectionUp,
    /// Open (or re-edit) the narrowing input of the list ("/").
    NarrowStart,
    /// Leave narrowing and show every item again (Esc in the list).
    NarrowClear,
    /// Open the ":" command line under the list.
    CommandLine,

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
    WorkspaceRename,
    WorkspaceDelete,
    WorkspaceSwitch(String),

    /// Open the settings dialog.
    OpenSettings,
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
            "GoHome" => GoHome,
            "OpenSettings" => OpenSettings,
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
            "CursorDown" => CursorDown,
            "CursorUp" => CursorUp,
            "ExtendSelectionDown" => ExtendSelectionDown,
            "ExtendSelectionUp" => ExtendSelectionUp,
            "Narrow" => NarrowStart,
            "NarrowClear" => NarrowClear,
            "CommandLine" => CommandLine,
            "ToggleHidden" => ToggleHidden,
            "ToggleDotfiles" => ToggleDotfiles,
            "ToggleExtensions" => ToggleExtensions,
            "WorkspaceList" => WorkspaceList,
            "WorkspaceNew" => WorkspaceNew { from_current: false },
            // Former "save as" menu item; creating from the current layout does the same.
            "WorkspaceSaveAs" => WorkspaceNew { from_current: true },
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Tab,
    Move,
    File,
    View,
    Workspace,
    Other,
}

/// A command as shown in the settings dialog's key page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandInfo {
    /// Name used in `[keys]`.
    pub name: &'static str,
    pub label: &'static str,
    pub category: Category,
}

const fn info(name: &'static str, label: &'static str, category: Category) -> CommandInfo {
    CommandInfo { name, label, category }
}

/// Every command `from_name` accepts (aliases such as "WorkspaceSaveAs" excluded), in
/// display order.
pub const CATALOG: &[CommandInfo] = {
    use Category::*;
    &[
        info("NewTab", "新しいタブ", Tab),
        info("CloseTab", "タブを閉じる", Tab),
        info("ReopenClosedTab", "閉じたタブを開き直す", Tab),
        info("DuplicateTab", "タブを複製", Tab),
        info("NextTab", "次のタブ", Tab),
        info("PrevTab", "前のタブ", Tab),
        info("SelectTab1", "タブ 1 を選ぶ", Tab),
        info("SelectTab2", "タブ 2 を選ぶ", Tab),
        info("SelectTab3", "タブ 3 を選ぶ", Tab),
        info("SelectTab4", "タブ 4 を選ぶ", Tab),
        info("SelectTab5", "タブ 5 を選ぶ", Tab),
        info("SelectTab6", "タブ 6 を選ぶ", Tab),
        info("SelectTab7", "タブ 7 を選ぶ", Tab),
        info("SelectTab8", "タブ 8 を選ぶ", Tab),
        info("SelectTab9", "最後のタブを選ぶ", Tab),
        info("SwitchPane", "左右のペインを切り替える", Tab),
        info("Back", "戻る", Move),
        info("Forward", "進む", Move),
        info("Up", "上へ", Move),
        info("GoHome", "ホーム", Move),
        info("FocusAddress", "パスを直接入力する", Move),
        info("Open", "開く", Move),
        info("OpenInNewTab", "新しいタブで開く", Move),
        info("CursorDown", "選択を下へ", Move),
        info("CursorUp", "選択を上へ", Move),
        info("ExtendSelectionDown", "選択範囲を下へ広げる", Move),
        info("ExtendSelectionUp", "選択範囲を上へ広げる", Move),
        info("Narrow", "絞り込み表示", Move),
        info("NarrowClear", "絞り込みを解除", Move),
        info("Rename", "名前の変更", File),
        info("NewFolder", "新しいフォルダー", File),
        info("Delete", "削除（ごみ箱へ）", File),
        info("DeletePermanently", "完全に削除", File),
        info("Copy", "コピー", File),
        info("Cut", "切り取り", File),
        info("Paste", "貼り付け", File),
        info("Undo", "元に戻す", File),
        info("SelectAll", "すべて選択", File),
        info("Properties", "プロパティ", File),
        info("Refresh", "最新の情報に更新", View),
        info("ViewIconsLarge", "大アイコン", View),
        info("ViewIconsMedium", "中アイコン", View),
        info("ViewIconsSmall", "小アイコン", View),
        info("ViewList", "一覧", View),
        info("ViewDetails", "詳細", View),
        info("ToggleHidden", "隠しファイル", View),
        info("ToggleDotfiles", "ドットファイル", View),
        info("ToggleExtensions", "ファイル名拡張子", View),
        info("ToggleNavTree", "ナビゲーション ウィンドウ", View),
        info("ToggleLayout", "2 ペイン表示", View),
        info("WorkspaceList", "ワークスペースの一覧", Workspace),
        info("WorkspaceNew", "新しいワークスペース", Workspace),
        info("CommandLine", "コマンド行", Other),
        info("OpenSettings", "設定", Other),
    ]
};

/// Name of `cmd` in `[keys]`, if it has one.
pub fn name_of(cmd: &Command) -> Option<&'static str> {
    CATALOG
        .iter()
        .find(|i| Command::from_name(i.name).as_ref() == Some(cmd))
        .map(|i| i.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        for (i, c) in CATALOG.iter().enumerate() {
            assert!(Command::from_name(c.name).is_some(), "{} does not parse", c.name);
            assert!(CATALOG[..i].iter().all(|d| d.name != c.name), "duplicate {}", c.name);
        }
        assert_eq!(name_of(&Command::OpenSettings), Some("OpenSettings"));
        assert_eq!(name_of(&Command::SelectTab(3)), Some("SelectTab3"));
        assert_eq!(name_of(&Command::Delete { permanent: true }), Some("DeletePermanently"));
        assert_eq!(name_of(&Command::WorkspaceNew { from_current: true }), None);
    }

    #[test]
    fn names() {
        assert_eq!(Command::from_name("NewTab"), Some(Command::NewTab));
        assert_eq!(Command::from_name("ToggleDotfiles"), Some(Command::ToggleDotfiles));
        assert_eq!(Command::from_name("SelectTab3"), Some(Command::SelectTab(3)));
        assert_eq!(Command::from_name("CursorDown"), Some(Command::CursorDown));
        assert_eq!(
            Command::from_name("ExtendSelectionUp"),
            Some(Command::ExtendSelectionUp)
        );
        assert_eq!(Command::from_name("Narrow"), Some(Command::NarrowStart));
        assert_eq!(Command::from_name("NarrowClear"), Some(Command::NarrowClear));
        assert_eq!(Command::from_name("CommandLine"), Some(Command::CommandLine));
        assert_eq!(
            Command::from_name("WorkspaceSaveAs"),
            Some(Command::WorkspaceNew { from_current: true })
        );
        assert_eq!(Command::from_name("Exit"), None);
        assert_eq!(Command::from_name("GoHome"), Some(Command::GoHome));
        assert_eq!(Command::from_name("SelectTab0"), None);
        assert_eq!(Command::from_name("nope"), None);
    }
}
