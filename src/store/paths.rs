use std::path::{Path, PathBuf};

pub const MAX_NAME_LEN: usize = 64;

/// Base directory: `%TABEX_HOME%` if set (development/tests), otherwise `%APPDATA%\tabex`.
pub fn base_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("TABEX_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home);
    }
    roaming_app_data().unwrap_or_else(std::env::temp_dir).join("tabex")
}

fn roaming_app_data() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

pub fn config_path(base: &Path) -> PathBuf {
    base.join("config.toml")
}

pub fn workspaces_dir(base: &Path) -> PathBuf {
    base.join("workspaces")
}

pub fn workspace_path(base: &Path, name: &str) -> PathBuf {
    workspaces_dir(base).join(format!("{name}.toml"))
}

/// Names of saved workspaces (file stems), sorted case-insensitively.
pub fn list_workspaces(base: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(workspaces_dir(base)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")) && p.is_file() {
                p.file_stem().and_then(|s| s.to_str()).map(str::to_string)
            } else {
                None
            }
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("ワークスペース名を入力してください")]
    Empty,
    #[error("ワークスペース名は {MAX_NAME_LEN} 文字以内にしてください")]
    TooLong,
    #[error("ワークスペース名に使えない文字が含まれています: {0}")]
    InvalidChar(char),
    #[error("ワークスペース名の末尾にピリオドや空白は使えません")]
    TrailingDotOrSpace,
    #[error("\"{0}\" は Windows の予約名のため使えません")]
    Reserved(String),
    #[error("\"{0}\" という名前のワークスペースは既にあります")]
    Duplicate(String),
}

/// Validate a workspace name. `existing` must not contain the workspace being renamed.
pub fn validate_name(name: &str, existing: &[String]) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(NameError::TooLong);
    }
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control())
    {
        return Err(NameError::InvalidChar(c));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(NameError::TrailingDotOrSpace);
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        return Err(NameError::Reserved(name.to_string()));
    }
    let lower = name.to_lowercase();
    if let Some(dup) = existing.iter().find(|e| e.to_lowercase() == lower) {
        return Err(NameError::Duplicate(dup.clone()));
    }
    Ok(())
}

/// First of `base`, `base-2`, `base-3`, ... not in `existing`.
pub fn unique_name(base: &str, existing: &[String]) -> String {
    let taken = |n: &str| existing.iter().any(|e| e.eq_ignore_ascii_case(n));
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base}-{i}")).find(|n| !taken(n)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert_eq!(validate_name("dev", &[]), Ok(()));
        assert_eq!(validate_name("写真 2026", &[]), Ok(()));
        assert_eq!(validate_name("a.b", &[]), Ok(()));
        assert_eq!(validate_name("COM0", &[]), Ok(()));
        assert_eq!(validate_name("CONSOLE", &[]), Ok(()));
        assert_eq!(validate_name(&"あ".repeat(64), &[]), Ok(()));
    }

    #[test]
    fn invalid_names() {
        assert_eq!(validate_name("", &[]), Err(NameError::Empty));
        assert_eq!(validate_name(&"a".repeat(65), &[]), Err(NameError::TooLong));
        for c in ['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\t'] {
            assert_eq!(validate_name(&format!("a{c}b"), &[]), Err(NameError::InvalidChar(c)));
        }
        assert_eq!(validate_name("a.", &[]), Err(NameError::TrailingDotOrSpace));
        assert_eq!(validate_name("a ", &[]), Err(NameError::TrailingDotOrSpace));
        for r in ["CON", "con", "nul.txt", "COM1", "lpt9"] {
            assert!(matches!(validate_name(r, &[]), Err(NameError::Reserved(_))), "{r}");
        }
        assert_eq!(
            validate_name("Dev", &["dev".into()]),
            Err(NameError::Duplicate("dev".into()))
        );
    }

    #[test]
    fn unique() {
        assert_eq!(unique_name("untitled", &[]), "untitled");
        assert_eq!(
            unique_name("untitled", &["Untitled".into(), "untitled-2".into()]),
            "untitled-3"
        );
    }
}
