//! Keyboard shortcut table: key spec parsing (pure) and lookup.

use std::collections::BTreeMap;

use crate::model::command::Command;
use crate::model::workspace::ViewMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeySpec {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub vk: u16,
}

const VK_BACK: u16 = 0x08;
const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_PRIOR: u16 = 0x21;
const VK_NEXT: u16 = 0x22;
const VK_END: u16 = 0x23;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_INSERT: u16 = 0x2D;
const VK_DELETE: u16 = 0x2E;
const VK_F1: u16 = 0x70;

pub fn parse_key(s: &str) -> Option<KeySpec> {
    let mut k = KeySpec {
        ctrl: false,
        shift: false,
        alt: false,
        vk: 0,
    };
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last()?;
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => k.ctrl = true,
            "shift" => k.shift = true,
            "alt" => k.alt = true,
            _ => return None,
        }
    }
    let lower = key.to_ascii_lowercase();
    k.vk = match lower.as_str() {
        "backspace" | "back" => VK_BACK,
        "tab" => VK_TAB,
        "enter" | "return" => VK_RETURN,
        "esc" | "escape" => VK_ESCAPE,
        "space" => VK_SPACE,
        "pageup" => VK_PRIOR,
        "pagedown" => VK_NEXT,
        "end" => VK_END,
        "home" => VK_HOME,
        "left" | "←" => VK_LEFT,
        "up" | "↑" => VK_UP,
        "right" | "→" => VK_RIGHT,
        "down" | "↓" => VK_DOWN,
        "insert" => VK_INSERT,
        "delete" | "del" => VK_DELETE,
        _ if lower.len() == 1 && lower.as_bytes()[0].is_ascii_alphanumeric() => {
            lower.to_ascii_uppercase().as_bytes()[0] as u16
        }
        _ if lower.starts_with('f') => {
            let n: u16 = lower[1..].parse().ok().filter(|n| (1..=24).contains(n))?;
            VK_F1 + n - 1
        }
        _ => return None,
    };
    Some(k)
}

pub fn default_bindings() -> Vec<(&'static str, Command)> {
    use Command::*;
    let mut v = vec![
        ("Ctrl+T", NewTab),
        ("Ctrl+W", CloseTab),
        ("Ctrl+F4", CloseTab),
        ("Ctrl+Shift+T", ReopenClosedTab),
        ("Ctrl+Tab", NextTab),
        ("Ctrl+Shift+Tab", PrevTab),
        ("Ctrl+PageDown", NextTab),
        ("Ctrl+PageUp", PrevTab),
        ("Tab", SwitchPane),
        ("Alt+Left", Back),
        ("Alt+Right", Forward),
        ("Alt+Up", Up),
        ("Backspace", Up),
        ("F5", Refresh),
        ("Ctrl+R", Refresh),
        ("Ctrl+L", FocusAddress),
        ("Alt+D", FocusAddress),
        ("F4", FocusAddress),
        ("Enter", Open),
        ("Ctrl+Enter", OpenSelectedInNewTab),
        ("F2", Rename),
        ("Ctrl+Shift+N", NewFolder),
        ("Delete", Delete { permanent: false }),
        ("Shift+Delete", Delete { permanent: true }),
        ("Ctrl+C", Copy),
        ("Ctrl+X", Cut),
        ("Ctrl+V", Paste),
        ("Ctrl+Z", Undo),
        ("Ctrl+A", SelectAll),
        ("Alt+Enter", Properties),
        ("Ctrl+H", ToggleHidden),
        ("Ctrl+Shift+H", ToggleDotfiles),
        ("Ctrl+Alt+W", WorkspaceList),
        ("Ctrl+Shift+1", SetViewMode(ViewMode::IconsLarge)),
        ("Ctrl+Shift+2", SetViewMode(ViewMode::IconsMedium)),
        ("Ctrl+Shift+3", SetViewMode(ViewMode::IconsSmall)),
        ("Ctrl+Shift+4", SetViewMode(ViewMode::List)),
        ("Ctrl+Shift+5", SetViewMode(ViewMode::Details)),
        // vi-style movement; plain letters only act inside the file list.
        ("J", CursorDown),
        ("K", CursorUp),
        ("Shift+J", ExtendSelectionDown),
        ("Shift+K", ExtendSelectionUp),
        ("H", PrevTab),
        ("L", NextTab),
    ];
    const DIGITS: [&str; 9] = [
        "Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+6", "Ctrl+7", "Ctrl+8", "Ctrl+9",
    ];
    for (i, k) in DIGITS.iter().enumerate() {
        v.push((k, SelectTab(i as u8 + 1)));
    }
    v
}

/// Commands that act on the file list and only apply while it has focus.
pub fn is_list_command(c: &Command) -> bool {
    matches!(
        c,
        Command::Open
            | Command::OpenSelectedInNewTab
            | Command::Rename
            | Command::Delete { .. }
            | Command::Copy
            | Command::Cut
            | Command::Paste
            | Command::SelectAll
            | Command::Properties
            | Command::CursorDown
            | Command::CursorUp
            | Command::ExtendSelectionDown
            | Command::ExtendSelectionUp
    )
}

/// Plain letter / digit keys (Shift allowed) are bound only while the file list has focus,
/// so the tree's type-ahead and text input elsewhere keep working.
pub fn is_list_only_key(k: &KeySpec) -> bool {
    // VK codes: '0'..'9' = 0x30..0x39, 'A'..'Z' = 0x41..0x5A.
    !k.ctrl && !k.alt && matches!(k.vk, 0x30..=0x39 | 0x41..=0x5A)
}

/// Keys an edit control needs for itself; never steal them while editing text.
pub fn is_edit_key(k: &KeySpec) -> bool {
    if !k.ctrl && !k.alt {
        return true;
    }
    k.ctrl
        && !k.alt
        && matches!(
            k.vk,
            0x41 | 0x43 | 0x56 | 0x58 | 0x59 | 0x5A | VK_LEFT | VK_RIGHT | VK_BACK | VK_DELETE | VK_HOME | VK_END
        )
}

#[derive(Debug, Default)]
pub struct KeyMap {
    bindings: Vec<(KeySpec, Command)>,
}

impl KeyMap {
    /// Defaults plus `config.toml` overrides. A value of "None" removes a binding.
    /// Returns invalid override entries as warnings.
    pub fn new(overrides: &BTreeMap<String, String>) -> (KeyMap, Vec<String>) {
        let mut bindings: Vec<(KeySpec, Command)> = default_bindings()
            .into_iter()
            .map(|(k, c)| (parse_key(k).expect("valid default key"), c))
            .collect();
        let mut warnings = Vec::new();
        for (key, name) in overrides {
            let Some(spec) = parse_key(key) else {
                warnings.push(format!("キー指定が不正です: {key}"));
                continue;
            };
            bindings.retain(|(k, _)| *k != spec);
            if name.eq_ignore_ascii_case("none") {
                continue;
            }
            match Command::from_name(name) {
                Some(c) => bindings.push((spec, c)),
                None => warnings.push(format!("不明なコマンドです: {name}")),
            }
        }
        (KeyMap { bindings }, warnings)
    }

    pub fn lookup(&self, k: &KeySpec) -> Option<&Command> {
        self.bindings.iter().find(|(spec, _)| spec == k).map(|(_, c)| c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        assert_eq!(
            parse_key("Ctrl+Alt+W"),
            Some(KeySpec {
                ctrl: true,
                shift: false,
                alt: true,
                vk: b'W' as u16
            })
        );
        assert_eq!(parse_key("F5").unwrap().vk, 0x74);
        assert_eq!(
            parse_key("shift+delete").unwrap(),
            KeySpec {
                ctrl: false,
                shift: true,
                alt: false,
                vk: VK_DELETE
            }
        );
        assert_eq!(parse_key("Ctrl+1").unwrap().vk, b'1' as u16);
        assert_eq!(parse_key("Hyper+X"), None);
        assert_eq!(parse_key("F25"), None);
        assert_eq!(parse_key(""), None);
    }

    #[test]
    fn defaults_parse() {
        let (m, w) = KeyMap::new(&BTreeMap::new());
        assert!(w.is_empty());
        assert_eq!(m.lookup(&parse_key("Ctrl+T").unwrap()), Some(&Command::NewTab));
        assert_eq!(m.lookup(&parse_key("Ctrl+9").unwrap()), Some(&Command::SelectTab(9)));
        assert_eq!(
            m.lookup(&parse_key("Ctrl+Shift+H").unwrap()),
            Some(&Command::ToggleDotfiles)
        );
    }

    #[test]
    fn overrides() {
        let mut o = BTreeMap::new();
        o.insert("Ctrl+T".to_string(), "None".to_string());
        o.insert("Ctrl+Q".to_string(), "NewTab".to_string());
        o.insert("Bad+Key".to_string(), "NewTab".to_string());
        o.insert("Ctrl+K".to_string(), "Nope".to_string());
        let (m, w) = KeyMap::new(&o);
        assert_eq!(m.lookup(&parse_key("Ctrl+T").unwrap()), None);
        assert_eq!(m.lookup(&parse_key("Ctrl+Q").unwrap()), Some(&Command::NewTab));
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn edit_keys() {
        assert!(is_edit_key(&parse_key("Backspace").unwrap()));
        assert!(is_edit_key(&parse_key("Ctrl+A").unwrap()));
        assert!(!is_edit_key(&parse_key("Ctrl+T").unwrap()));
        assert!(!is_edit_key(&parse_key("Alt+Left").unwrap()));
    }

    #[test]
    fn list_only_keys() {
        assert!(is_list_only_key(&parse_key("J").unwrap()));
        assert!(is_list_only_key(&parse_key("Shift+K").unwrap()));
        assert!(is_list_only_key(&parse_key("1").unwrap()));
        assert!(!is_list_only_key(&parse_key("Ctrl+J").unwrap()));
        assert!(!is_list_only_key(&parse_key("Alt+H").unwrap()));
        assert!(!is_list_only_key(&parse_key("Tab").unwrap()));
        assert!(!is_list_only_key(&parse_key("Down").unwrap()));
        assert!(!is_list_only_key(&parse_key("F5").unwrap()));
    }

    #[test]
    fn vi_keys() {
        let (m, _) = KeyMap::new(&BTreeMap::new());
        let get = |k: &str| m.lookup(&parse_key(k).unwrap()).cloned();
        assert_eq!(get("J"), Some(Command::CursorDown));
        assert_eq!(get("K"), Some(Command::CursorUp));
        assert_eq!(get("Shift+J"), Some(Command::ExtendSelectionDown));
        assert_eq!(get("Shift+K"), Some(Command::ExtendSelectionUp));
        assert_eq!(get("H"), Some(Command::PrevTab));
        assert_eq!(get("L"), Some(Command::NextTab));
        let mut o = BTreeMap::new();
        o.insert("J".to_string(), "None".to_string());
        let (m, w) = KeyMap::new(&o);
        assert!(w.is_empty());
        assert_eq!(m.lookup(&parse_key("J").unwrap()), None);
    }
}
