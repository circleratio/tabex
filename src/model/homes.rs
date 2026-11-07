//! Home directories for the toolbar's home button (pure).

use super::workspace::Location;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub name: Option<String>,
    /// Path after environment variable expansion, otherwise as written.
    pub location: Location,
}

/// Valid entries in order, or `[fallback]` when none is valid, plus one warning per
/// invalid entry. Entries are `(name, path)` as written in `[[homes]]`; `expand` expands
/// `%VAR%`.
pub fn resolve<'a>(
    entries: impl IntoIterator<Item = (Option<&'a str>, Option<&'a str>)>,
    expand: impl Fn(&str) -> String,
    fallback: &Location,
) -> (Vec<Home>, Vec<String>) {
    let mut homes = Vec::new();
    let mut warnings = Vec::new();
    for (i, (name, path)) in entries.into_iter().enumerate() {
        let warn = |reason: String| format!("ホームディレクトリの設定が不正です: {} 番目（{reason}）", i + 1);
        let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) else {
            warnings.push(warn("path がありません".to_string()));
            continue;
        };
        let path = expand(path).trim().to_string();
        if !is_absolute(&path) {
            warnings.push(warn(format!("絶対パスではありません: {path}")));
            continue;
        }
        homes.push(Home {
            name: name.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string),
            location: Location::new(trim_separator(&path)),
        });
    }
    if homes.is_empty() {
        homes.push(Home {
            name: None,
            location: fallback.clone(),
        });
    }
    (homes, warnings)
}

/// A drive path ("D:\...") or a UNC path ("\\server\share...").
pub fn is_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    let unc = path.len() > 2 && path.starts_with("\\\\");
    drive || unc
}

/// Drop trailing separators except the one of a drive root ("D:\").
fn trim_separator(path: &str) -> &str {
    let t = path.trim_end_matches('\\');
    if t.len() == 2 && t.ends_with(':') {
        &path[..3]
    } else {
        t
    }
}

/// Same folder, ignoring case and a trailing separator.
pub fn same_place(a: &Location, b: &Location) -> bool {
    trim_separator(a.as_str()).to_lowercase() == trim_separator(b.as_str()).to_lowercase()
}

/// Menu text: "name\tpath" (two columns in a menu) or just the path. `&` is doubled so it
/// is not taken as an access key.
pub fn menu_label(h: &Home) -> String {
    let path = h.location.as_str().replace('&', "&&");
    match &h.name {
        Some(name) => format!("{}\t{path}", name.replace('&', "&&")),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback() -> Location {
        Location::new(r"C:\Users\me")
    }

    fn run(entries: &[(Option<&str>, Option<&str>)]) -> (Vec<Home>, Vec<String>) {
        let expand = |s: &str| s.replace("%USERPROFILE%", r"C:\Users\me");
        resolve(entries.iter().copied(), expand, &fallback())
    }

    #[test]
    fn keeps_order_and_names() {
        let (homes, warnings) = run(&[
            (Some("仕事"), Some(r"D:\work")),
            (None, Some(r"\\nas\share")),
            (Some("  "), Some(r"E:\x\")),
        ]);
        assert!(warnings.is_empty());
        assert_eq!(
            homes,
            [
                Home {
                    name: Some("仕事".into()),
                    location: Location::new(r"D:\work")
                },
                Home {
                    name: None,
                    location: Location::new(r"\\nas\share")
                },
                Home {
                    name: None,
                    location: Location::new(r"E:\x")
                },
            ]
        );
    }

    #[test]
    fn invalid_entries_warn_with_their_number() {
        let (homes, warnings) = run(&[
            (Some("a"), Some(r"D:\a")),
            (Some("no path"), None),
            (None, Some("   ")),
            (None, Some(r"relative\dir")),
            (None, Some("shell:Downloads")),
        ]);
        assert_eq!(homes.len(), 1);
        assert_eq!(
            warnings,
            [
                "ホームディレクトリの設定が不正です: 2 番目（path がありません）",
                "ホームディレクトリの設定が不正です: 3 番目（path がありません）",
                r"ホームディレクトリの設定が不正です: 4 番目（絶対パスではありません: relative\dir）",
                "ホームディレクトリの設定が不正です: 5 番目（絶対パスではありません: shell:Downloads）",
            ]
        );
    }

    #[test]
    fn expands_variables_and_keeps_drive_roots() {
        let (homes, _) = run(&[
            (None, Some(r"%USERPROFILE%\docs\")),
            (None, Some(r"D:\")),
            (None, Some(r"D:\\")),
        ]);
        let locs: Vec<&str> = homes.iter().map(|h| h.location.as_str()).collect();
        assert_eq!(locs, [r"C:\Users\me\docs", r"D:\", r"D:\"]);
    }

    #[test]
    fn falls_back_when_nothing_is_valid() {
        let (homes, warnings) = run(&[]);
        assert!(warnings.is_empty());
        assert_eq!(
            homes,
            [Home {
                name: None,
                location: fallback()
            }]
        );
        let (homes, warnings) = run(&[(None, Some("x"))]);
        assert_eq!(homes[0].location, fallback());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn absolute_paths() {
        assert!(is_absolute(r"D:\work"));
        assert!(is_absolute(r"d:\"));
        assert!(is_absolute(r"\\nas\share"));
        assert!(!is_absolute("D:"));
        assert!(!is_absolute("D:work"));
        assert!(!is_absolute(r"work\x"));
        assert!(!is_absolute(r"\\"));
        assert!(!is_absolute("shell:Downloads"));
        assert!(!is_absolute("::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"));
    }

    #[test]
    fn same_place_ignores_case_and_separator() {
        let l = |s: &str| Location::new(s);
        assert!(same_place(&l(r"D:\Work"), &l(r"d:\work\")));
        assert!(same_place(&l(r"D:\"), &l(r"d:\")));
        assert!(!same_place(&l(r"D:\work"), &l(r"D:\work2")));
    }

    #[test]
    fn labels() {
        let h = |name: Option<&str>, path: &str| Home {
            name: name.map(str::to_string),
            location: Location::new(path),
        };
        assert_eq!(menu_label(&h(Some("仕事"), r"D:\work")), "仕事\tD:\\work");
        assert_eq!(menu_label(&h(None, r"\\nas\share")), r"\\nas\share");
        assert_eq!(menu_label(&h(Some("A&B"), r"D:\a&b")), "A&&B\tD:\\a&&b");
    }
}
