//! Toolbar button definitions and the state they show (pure).

use super::command::Command;
use super::workspace::ViewMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonId {
    Undo,
    ViewMode,
    Hidden,
    Dotfiles,
    Extensions,
    NavTree,
    Layout,
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Push,
    Check,
    Dropdown,
}

/// What a button image shows: a character of the icon font, or short text in the UI font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Icon(char),
    Text(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ButtonDef {
    pub id: ButtonId,
    pub kind: ButtonKind,
    /// Tooltip name.
    pub label: &'static str,
    /// None for ViewMode (the mode is chosen from the dropdown).
    pub command: Option<Command>,
    /// A separator goes between buttons of different groups.
    pub group: u8,
    /// Image for every button but ViewMode, whose image follows the view mode.
    pub glyph: Glyph,
}

/// Snapshot of everything the toolbar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarState {
    pub can_undo: bool,
    pub view_mode: ViewMode,
    pub show_hidden: bool,
    pub show_dotfiles: bool,
    pub show_extensions: bool,
    pub nav_visible: bool,
    pub dual: bool,
}

/// Left to right.
pub const BUTTONS: [ButtonDef; 8] = [
    ButtonDef {
        id: ButtonId::Undo,
        kind: ButtonKind::Push,
        label: "元に戻す",
        command: Some(Command::Undo),
        group: 0,
        glyph: Glyph::Icon('\u{E7A7}'),
    },
    ButtonDef {
        id: ButtonId::ViewMode,
        kind: ButtonKind::Dropdown,
        label: "表示モード",
        command: None,
        group: 1,
        glyph: Glyph::Icon('\u{E71D}'),
    },
    ButtonDef {
        id: ButtonId::Hidden,
        kind: ButtonKind::Check,
        label: "隠しファイル",
        command: Some(Command::ToggleHidden),
        group: 2,
        glyph: Glyph::Icon('\u{E7B3}'),
    },
    ButtonDef {
        id: ButtonId::Dotfiles,
        kind: ButtonKind::Check,
        label: "ドットファイル",
        command: Some(Command::ToggleDotfiles),
        group: 2,
        glyph: Glyph::Text(".*"),
    },
    ButtonDef {
        id: ButtonId::Extensions,
        kind: ButtonKind::Check,
        label: "ファイル名拡張子",
        command: Some(Command::ToggleExtensions),
        group: 2,
        glyph: Glyph::Text(".ext"),
    },
    ButtonDef {
        id: ButtonId::NavTree,
        kind: ButtonKind::Check,
        label: "ナビゲーション ウィンドウ",
        command: Some(Command::ToggleNavTree),
        group: 3,
        glyph: Glyph::Icon('\u{E90C}'),
    },
    ButtonDef {
        id: ButtonId::Layout,
        kind: ButtonKind::Check,
        label: "2 ペイン表示",
        command: Some(Command::ToggleLayout),
        group: 3,
        glyph: Glyph::Icon('\u{F57C}'),
    },
    ButtonDef {
        id: ButtonId::Refresh,
        kind: ButtonKind::Push,
        label: "最新の情報に更新",
        command: Some(Command::Refresh),
        group: 4,
        glyph: Glyph::Icon('\u{E72C}'),
    },
];

/// Dropdown order of the view mode button.
///
/// Icon code points draw the same picture in Segoe MDL2 Assets (Windows 10) and
/// Segoe Fluent Icons (Windows 11).
pub const VIEW_MODES: [(ViewMode, &str, Glyph); 5] = [
    (ViewMode::IconsLarge, "大アイコン", Glyph::Icon('\u{EB9F}')),
    (ViewMode::IconsMedium, "中アイコン", Glyph::Icon('\u{F0E2}')),
    (ViewMode::IconsSmall, "小アイコン", Glyph::Icon('\u{E80A}')),
    (ViewMode::List, "一覧", Glyph::Icon('\u{E8FD}')),
    (ViewMode::Details, "詳細", Glyph::Icon('\u{E71D}')),
];

pub fn def(id: ButtonId) -> &'static ButtonDef {
    BUTTONS.iter().find(|b| b.id == id).expect("every ButtonId is defined")
}

pub fn view_mode_index(mode: ViewMode) -> usize {
    VIEW_MODES.iter().position(|(m, _, _)| *m == mode).unwrap_or(0)
}

pub fn checked(id: ButtonId, st: &ToolbarState) -> bool {
    match id {
        ButtonId::Hidden => st.show_hidden,
        ButtonId::Dotfiles => st.show_dotfiles,
        ButtonId::Extensions => st.show_extensions,
        ButtonId::NavTree => st.nav_visible,
        ButtonId::Layout => st.dual,
        ButtonId::Undo | ButtonId::ViewMode | ButtonId::Refresh => false,
    }
}

pub fn enabled(id: ButtonId, st: &ToolbarState) -> bool {
    id != ButtonId::Undo || st.can_undo
}

pub fn glyph(id: ButtonId, st: &ToolbarState) -> Glyph {
    match id {
        ButtonId::ViewMode => VIEW_MODES[view_mode_index(st.view_mode)].2,
        _ => def(id).glyph,
    }
}

/// "隠しファイル (Ctrl+H)", or just the label when the command has no key.
pub fn tooltip(label: &str, key: Option<&str>) -> String {
    match key {
        Some(k) => format!("{label} ({k})"),
        None => label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ToolbarState {
        ToolbarState {
            can_undo: false,
            view_mode: ViewMode::Details,
            show_hidden: false,
            show_dotfiles: false,
            show_extensions: false,
            nav_visible: false,
            dual: false,
        }
    }

    #[test]
    fn order_groups_and_commands() {
        let ids: Vec<ButtonId> = BUTTONS.iter().map(|b| b.id).collect();
        use ButtonId::*;
        assert_eq!(
            ids,
            [Undo, ViewMode, Hidden, Dotfiles, Extensions, NavTree, Layout, Refresh]
        );
        let groups: Vec<u8> = BUTTONS.iter().map(|b| b.group).collect();
        assert_eq!(groups, [0, 1, 2, 2, 2, 3, 3, 4]);
        assert_eq!(def(Undo).command, Some(Command::Undo));
        assert_eq!(def(ViewMode).command, None);
        assert_eq!(def(ViewMode).kind, ButtonKind::Dropdown);
        assert_eq!(def(Hidden).command, Some(Command::ToggleHidden));
        assert_eq!(def(Dotfiles).command, Some(Command::ToggleDotfiles));
        assert_eq!(def(Extensions).command, Some(Command::ToggleExtensions));
        assert_eq!(def(NavTree).command, Some(Command::ToggleNavTree));
        assert_eq!(def(Layout).command, Some(Command::ToggleLayout));
        assert_eq!(def(Refresh).command, Some(Command::Refresh));
        for id in [Hidden, Dotfiles, Extensions, NavTree, Layout] {
            assert_eq!(def(id).kind, ButtonKind::Check);
        }
        for id in [Undo, Refresh] {
            assert_eq!(def(id).kind, ButtonKind::Push);
        }
    }

    #[test]
    fn checked_follows_state() {
        let mut st = state();
        for b in &BUTTONS {
            assert!(!checked(b.id, &st));
        }
        st.show_hidden = true;
        assert!(checked(ButtonId::Hidden, &st));
        assert!(!checked(ButtonId::Dotfiles, &st));
        st.show_dotfiles = true;
        st.show_extensions = true;
        st.nav_visible = true;
        st.dual = true;
        for id in [
            ButtonId::Hidden,
            ButtonId::Dotfiles,
            ButtonId::Extensions,
            ButtonId::NavTree,
            ButtonId::Layout,
        ] {
            assert!(checked(id, &st));
        }
        assert!(!checked(ButtonId::Undo, &st));
        assert!(!checked(ButtonId::Refresh, &st));
    }

    #[test]
    fn only_undo_without_history_is_disabled() {
        let mut st = state();
        assert!(!enabled(ButtonId::Undo, &st));
        for b in BUTTONS.iter().filter(|b| b.id != ButtonId::Undo) {
            assert!(enabled(b.id, &st));
        }
        st.can_undo = true;
        assert!(enabled(ButtonId::Undo, &st));
    }

    #[test]
    fn view_mode_glyph_follows_mode() {
        let mut st = state();
        for (mode, _, g) in VIEW_MODES {
            st.view_mode = mode;
            assert_eq!(glyph(ButtonId::ViewMode, &st), g);
        }
        assert_eq!(glyph(ButtonId::Refresh, &st), def(ButtonId::Refresh).glyph);
        let modes: Vec<ViewMode> = VIEW_MODES.iter().map(|v| v.0).collect();
        assert_eq!(
            modes,
            [
                ViewMode::IconsLarge,
                ViewMode::IconsMedium,
                ViewMode::IconsSmall,
                ViewMode::List,
                ViewMode::Details
            ]
        );
    }

    #[test]
    fn tooltip_text() {
        assert_eq!(tooltip("隠しファイル", Some("Ctrl+H")), "隠しファイル (Ctrl+H)");
        assert_eq!(tooltip("ファイル名拡張子", None), "ファイル名拡張子");
    }
}
