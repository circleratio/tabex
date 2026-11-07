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
/// "/?" key on both US and Japanese layouts.
const VK_OEM_2: u16 = 0xBF;

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
        "/" | "slash" => VK_OEM_2,
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

/// Display form of a key spec, e.g. "Ctrl+Shift+H". The inverse of `parse_key`.
pub fn format_key(k: &KeySpec) -> String {
    let name = match k.vk {
        VK_BACK => "Backspace".to_string(),
        VK_TAB => "Tab".to_string(),
        VK_RETURN => "Enter".to_string(),
        VK_ESCAPE => "Esc".to_string(),
        VK_SPACE => "Space".to_string(),
        VK_PRIOR => "PageUp".to_string(),
        VK_NEXT => "PageDown".to_string(),
        VK_END => "End".to_string(),
        VK_HOME => "Home".to_string(),
        VK_LEFT => "Left".to_string(),
        VK_UP => "Up".to_string(),
        VK_RIGHT => "Right".to_string(),
        VK_DOWN => "Down".to_string(),
        VK_INSERT => "Insert".to_string(),
        VK_DELETE => "Delete".to_string(),
        VK_OEM_2 => "/".to_string(),
        0x30..=0x39 | 0x41..=0x5A => (k.vk as u8 as char).to_string(),
        vk if (VK_F1..VK_F1 + 24).contains(&vk) => format!("F{}", vk - VK_F1 + 1),
        vk => format!("0x{vk:02X}"),
    };
    let mut s = String::new();
    for (on, m) in [(k.ctrl, "Ctrl+"), (k.alt, "Alt+"), (k.shift, "Shift+")] {
        if on {
            s.push_str(m);
        }
    }
    s + &name
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
        ("Alt+Home", GoHome),
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
        ("/", NarrowStart),
        ("Esc", NarrowClear),
        ("Ctrl+U", Up),
        ("Ctrl+B", Up),
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
            | Command::NarrowStart
            | Command::NarrowClear
    )
}

/// Plain letter / digit keys (Shift allowed) are bound only while the file list has focus,
/// so the tree's type-ahead and text input elsewhere keep working.
pub fn is_list_only_key(k: &KeySpec) -> bool {
    // VK codes: '0'..'9' = 0x30..0x39, 'A'..'Z' = 0x41..0x5A, punctuation (VK_OEM_*) = 0xBA..0xE2.
    !k.ctrl && !k.alt && matches!(k.vk, 0x30..=0x39 | 0x41..=0x5A | 0xBA..=0xE2)
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
            // A, B, C, U, V, X, Y, Z: B / U are kept so Ctrl+B / Ctrl+U (Up) do nothing while typing.
            0x41 | 0x42
                | 0x43
                | 0x55
                | 0x56
                | 0x58
                | 0x59
                | 0x5A
                | VK_LEFT
                | VK_RIGHT
                | VK_BACK
                | VK_DELETE
                | VK_HOME
                | VK_END
        )
}

#[derive(Debug, Default)]
pub struct KeyMap {
    bindings: Vec<(KeySpec, Command)>,
    /// Bindings by typed character (WM_CHAR), for keys whose position differs between
    /// keyboard layouts such as ":".
    chars: Vec<(char, Command)>,
}

/// Default character bindings; they only act while the file list has focus.
pub fn default_char_bindings() -> Vec<(char, Command)> {
    vec![(':', Command::CommandLine)]
}

/// A single punctuation character that is bound by character rather than by key.
fn parse_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    (it.next().is_none() && c.is_ascii_punctuation()).then_some(c)
}

impl KeyMap {
    /// Defaults plus `config.toml` overrides. A value of "None" removes a binding.
    /// Returns invalid override entries as warnings.
    pub fn new(overrides: &BTreeMap<String, String>) -> (KeyMap, Vec<String>) {
        let mut bindings: Vec<(KeySpec, Command)> = default_bindings()
            .into_iter()
            .map(|(k, c)| (parse_key(k).expect("valid default key"), c))
            .collect();
        let mut chars = default_char_bindings();
        let mut warnings = Vec::new();
        for (key, name) in overrides {
            let Some(spec) = parse_key(key) else {
                if let Some(c) = parse_char(key) {
                    chars.retain(|(k, _)| *k != c);
                    if name.eq_ignore_ascii_case("none") {
                        continue;
                    }
                    match Command::from_name(name) {
                        Some(cmd) => chars.push((c, cmd)),
                        None => warnings.push(format!("不明なコマンドです: {name}")),
                    }
                } else {
                    warnings.push(format!("キー指定が不正です: {key}"));
                }
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
        (KeyMap { bindings, chars }, warnings)
    }

    pub fn lookup(&self, k: &KeySpec) -> Option<&Command> {
        self.bindings.iter().find(|(spec, _)| spec == k).map(|(_, c)| c)
    }

    /// First key bound to `cmd`; defaults come before overrides, so F5 wins over Ctrl+R.
    pub fn key_for(&self, cmd: &Command) -> Option<&KeySpec> {
        self.bindings.iter().find(|(_, c)| c == cmd).map(|(k, _)| k)
    }

    /// Characters bound to `cmd` (such as ":" for the command line).
    pub fn chars_of(&self, cmd: &Command) -> Vec<char> {
        self.chars.iter().filter(|(_, c)| c == cmd).map(|(k, _)| *k).collect()
    }

    pub fn lookup_char(&self, c: char) -> Option<&Command> {
        self.chars.iter().find(|(k, _)| *k == c).map(|(_, cmd)| cmd)
    }
}

/// Effective key bindings while the settings dialog edits them: (key, command name) in
/// `KeyMap` order, each key at most once. Character bindings such as ":" are not in it.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyTable {
    pub bindings: Vec<(KeySpec, &'static str)>,
}

fn named_defaults() -> Vec<(KeySpec, &'static str)> {
    default_bindings()
        .into_iter()
        .map(|(k, c)| {
            let name = crate::model::command::name_of(&c).expect("every default command has a name");
            (parse_key(k).expect("valid default key"), name)
        })
        .collect()
}

impl KeyTable {
    /// Defaults plus `[keys]` overrides, the same way `KeyMap::new` combines them.
    pub fn from_overrides(overrides: &BTreeMap<String, String>) -> KeyTable {
        let mut bindings = named_defaults();
        for (key, name) in overrides {
            let Some(spec) = parse_key(key) else { continue };
            bindings.retain(|(k, _)| *k != spec);
            if let Some(name) = Command::from_name(name).and_then(|c| crate::model::command::name_of(&c)) {
                bindings.push((spec, name));
            }
        }
        KeyTable { bindings }
    }

    pub fn keys_of(&self, name: &str) -> Vec<KeySpec> {
        self.bindings
            .iter()
            .filter(|(_, n)| *n == name)
            .map(|(k, _)| *k)
            .collect()
    }

    pub fn owner_of(&self, key: &KeySpec) -> Option<&'static str> {
        self.bindings.iter().find(|(k, _)| k == key).map(|(_, n)| *n)
    }

    /// Bind `key` to `name`; returns the other command it was taken from, if any.
    pub fn assign(&mut self, key: KeySpec, name: &'static str) -> Option<&'static str> {
        let prev = self.owner_of(&key);
        if prev == Some(name) {
            return None;
        }
        self.remove(&key);
        self.bindings.push((key, name));
        prev
    }

    pub fn remove(&mut self, key: &KeySpec) {
        self.bindings.retain(|(k, _)| k != key);
    }

    /// The command's default keys only, taken back from other commands if needed.
    pub fn reset(&mut self, name: &str) {
        self.bindings.retain(|(_, n)| *n != name);
        for (k, n) in named_defaults().into_iter().filter(|(_, n)| *n == name) {
            self.remove(&k);
            self.bindings.push((k, n));
        }
    }

    pub fn reset_all(&mut self) {
        self.bindings = named_defaults();
    }

    pub fn is_default(&self, name: &str) -> bool {
        let mut now = self.keys_of(name);
        let mut def: Vec<KeySpec> = named_defaults()
            .into_iter()
            .filter(|(_, n)| *n == name)
            .map(|(k, _)| k)
            .collect();
        let key = |k: &KeySpec| (k.vk, k.ctrl, k.alt, k.shift);
        now.sort_by_key(key);
        def.sort_by_key(key);
        now == def
    }

    /// `[keys]` that reproduce this table: "None" for removed defaults, command names for
    /// the rest. Entries of `keep` that are not key specs (characters such as ":") are kept.
    pub fn to_overrides(&self, keep: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let defaults = named_defaults();
        let mut out = BTreeMap::new();
        for (k, n) in &defaults {
            match self.owner_of(k) {
                None => {
                    out.insert(format_key(k), "None".to_string());
                }
                Some(m) if m != *n => {
                    out.insert(format_key(k), m.to_string());
                }
                Some(_) => {}
            }
        }
        for (k, n) in &self.bindings {
            if !defaults.iter().any(|(dk, _)| dk == k) {
                out.insert(format_key(k), n.to_string());
            }
        }
        for (key, name) in keep {
            if parse_key(key).is_none() {
                out.insert(key.clone(), name.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same_as_keymap(t: &KeyTable, m: &KeyMap) {
        assert_eq!(t.bindings.len(), m.bindings.len());
        for (k, n) in &t.bindings {
            assert_eq!(m.lookup(k), Command::from_name(n).as_ref(), "{}", format_key(k));
        }
    }

    #[test]
    fn key_table_matches_keymap() {
        let mut o = BTreeMap::new();
        o.insert("F5".to_string(), "None".to_string());
        o.insert("Ctrl+Q".to_string(), "GoHome".to_string());
        o.insert("Ctrl+T".to_string(), "CloseTab".to_string());
        o.insert(":".to_string(), "None".to_string());
        let t = KeyTable::from_overrides(&o);
        let (m, _) = KeyMap::new(&o);
        same_as_keymap(&t, &m);
        assert_eq!(t.owner_of(&parse_key("Ctrl+T").unwrap()), Some("CloseTab"));
        assert!(t.owner_of(&parse_key("F5").unwrap()).is_none());
        // Every default command has a name.
        assert_eq!(named_defaults().len(), default_bindings().len());
    }

    #[test]
    fn key_table_edits() {
        let mut t = KeyTable::from_overrides(&BTreeMap::new());
        let k = |s: &str| parse_key(s).unwrap();
        assert!(t.is_default("GoHome"));
        // Free key.
        assert_eq!(t.assign(k("Ctrl+Q"), "GoHome"), None);
        assert_eq!(t.keys_of("GoHome"), [k("Alt+Home"), k("Ctrl+Q")]);
        assert!(!t.is_default("GoHome"));
        // Taken from another command.
        assert_eq!(t.assign(k("Ctrl+Shift+H"), "GoHome"), Some("ToggleDotfiles"));
        assert!(t.keys_of("ToggleDotfiles").is_empty());
        assert_eq!(t.assign(k("Ctrl+Shift+H"), "GoHome"), None);
        t.remove(&k("Alt+Home"));
        assert_eq!(t.keys_of("GoHome"), [k("Ctrl+Q"), k("Ctrl+Shift+H")]);
        // Reset takes the default key back from GoHome.
        t.reset("ToggleDotfiles");
        assert_eq!(t.keys_of("ToggleDotfiles"), [k("Ctrl+Shift+H")]);
        assert_eq!(t.keys_of("GoHome"), [k("Ctrl+Q")]);
        t.reset("GoHome");
        assert!(t.is_default("GoHome"));
        t.assign(k("F9"), "Refresh");
        t.reset_all();
        assert_eq!(t, KeyTable::from_overrides(&BTreeMap::new()));
    }

    #[test]
    fn key_table_overrides_round_trip() {
        let mut keep = BTreeMap::new();
        keep.insert(":".to_string(), "None".to_string());
        keep.insert(";".to_string(), "CommandLine".to_string());
        let mut t = KeyTable::from_overrides(&keep);
        assert_eq!(t.to_overrides(&BTreeMap::new()), BTreeMap::new());
        let k = |s: &str| parse_key(s).unwrap();
        t.assign(k("Ctrl+Shift+H"), "GoHome");
        t.remove(&k("F5"));
        t.assign(k("Ctrl+Q"), "OpenSettings");
        let o = t.to_overrides(&keep);
        assert_eq!(o.get("Ctrl+Shift+H").map(String::as_str), Some("GoHome"));
        assert_eq!(o.get("F5").map(String::as_str), Some("None"));
        assert_eq!(o.get("Ctrl+Q").map(String::as_str), Some("OpenSettings"));
        assert_eq!(o.get(";").map(String::as_str), Some("CommandLine"));
        assert_eq!(o.get(":").map(String::as_str), Some("None"));
        let (m, w) = KeyMap::new(&o);
        assert!(w.is_empty());
        same_as_keymap(&KeyTable::from_overrides(&o), &m);
        let mut a = KeyTable::from_overrides(&o).bindings;
        let mut b = t.bindings.clone();
        let key = |x: &(KeySpec, &str)| (x.0.vk, x.0.ctrl, x.0.alt, x.0.shift);
        a.sort_by_key(key);
        b.sort_by_key(key);
        assert_eq!(a, b);
    }

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
    fn format_round_trip() {
        for s in [
            "Ctrl+Shift+H",
            "Ctrl+Alt+W",
            "Ctrl+Alt+Shift+X",
            "F5",
            "F24",
            "Alt+Enter",
            "Shift+Delete",
            "Ctrl+PageDown",
            "Backspace",
            "Esc",
            "Left",
            "/",
            "Ctrl+1",
            "J",
        ] {
            let k = parse_key(s).unwrap();
            assert_eq!(format_key(&k), s);
            assert_eq!(parse_key(&format_key(&k)), Some(k));
        }
        // Modifier order is normalized.
        assert_eq!(format_key(&parse_key("shift+ctrl+h").unwrap()), "Ctrl+Shift+H");
    }

    #[test]
    fn home_key() {
        let (m, _) = KeyMap::new(&BTreeMap::new());
        let k = parse_key("Alt+Home").unwrap();
        assert_eq!(m.lookup(&k), Some(&Command::GoHome));
        assert_eq!(m.key_for(&Command::GoHome).map(format_key).as_deref(), Some("Alt+Home"));
        // Works everywhere: not a list command and not stolen from edits as typing.
        assert!(!is_list_command(&Command::GoHome));
        assert!(!is_edit_key(&k));
    }

    #[test]
    fn key_for_command() {
        let (m, _) = KeyMap::new(&BTreeMap::new());
        let key = |m: &KeyMap, c: Command| m.key_for(&c).map(format_key);
        assert_eq!(key(&m, Command::Refresh).as_deref(), Some("F5"));
        assert_eq!(key(&m, Command::ToggleHidden).as_deref(), Some("Ctrl+H"));
        assert_eq!(key(&m, Command::Undo).as_deref(), Some("Ctrl+Z"));
        assert_eq!(
            key(&m, Command::SetViewMode(ViewMode::IconsLarge)).as_deref(),
            Some("Ctrl+Shift+1")
        );
        assert_eq!(key(&m, Command::ToggleExtensions), None);
        let mut o = BTreeMap::new();
        o.insert("F5".to_string(), "None".to_string());
        o.insert("Ctrl+Q".to_string(), "ToggleExtensions".to_string());
        let (m, _) = KeyMap::new(&o);
        assert_eq!(key(&m, Command::Refresh).as_deref(), Some("Ctrl+R"));
        assert_eq!(key(&m, Command::ToggleExtensions).as_deref(), Some("Ctrl+Q"));
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
        assert!(is_list_only_key(&parse_key("/").unwrap()));
        assert!(is_list_only_key(&parse_key("Shift+/").unwrap()));
    }

    #[test]
    fn char_bindings() {
        let (m, w) = KeyMap::new(&BTreeMap::new());
        assert!(w.is_empty());
        assert_eq!(m.lookup_char(':'), Some(&Command::CommandLine));
        assert_eq!(m.lookup_char(';'), None);
        // "/" stays a key binding, ":" is matched by character.
        assert!(parse_key("/").is_some());
        assert!(parse_key(":").is_none());
        let mut o = BTreeMap::new();
        o.insert(":".to_string(), "None".to_string());
        o.insert(";".to_string(), "CommandLine".to_string());
        o.insert("!".to_string(), "Nope".to_string());
        o.insert("ab".to_string(), "NewTab".to_string());
        let (m, w) = KeyMap::new(&o);
        assert_eq!(m.lookup_char(':'), None);
        assert_eq!(m.lookup_char(';'), Some(&Command::CommandLine));
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn up_keys() {
        let (m, _) = KeyMap::new(&BTreeMap::new());
        assert_eq!(m.lookup(&parse_key("Ctrl+U").unwrap()), Some(&Command::Up));
        assert_eq!(m.lookup(&parse_key("Ctrl+B").unwrap()), Some(&Command::Up));
        // Typing in an edit control never navigates.
        assert!(is_edit_key(&parse_key("Ctrl+U").unwrap()));
        assert!(is_edit_key(&parse_key("Ctrl+B").unwrap()));
        assert!(!is_list_only_key(&parse_key("Ctrl+U").unwrap()));
        assert!(!is_list_command(&Command::Up));
    }

    #[test]
    fn narrow_keys() {
        assert_eq!(parse_key("/").unwrap().vk, 0xBF);
        assert_eq!(parse_key("Slash"), parse_key("/"));
        let (m, _) = KeyMap::new(&BTreeMap::new());
        assert_eq!(m.lookup(&parse_key("/").unwrap()), Some(&Command::NarrowStart));
        assert_eq!(m.lookup(&parse_key("Esc").unwrap()), Some(&Command::NarrowClear));
        assert!(is_list_command(&Command::NarrowStart));
        assert!(is_list_command(&Command::NarrowClear));
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
