//! Resolving user input and shell locations to canonical parsing names and display names.

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use crate::fs::enumerate::Pidl;
use crate::model::workspace::Location;

fn name_of(pidl: *const ITEMIDLIST, kind: SIGDN) -> Option<String> {
    unsafe {
        let p = SHGetNameFromIDList(pidl, kind).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// A location for an absolute PIDL that the shell can parse again.
///
/// The canonical name (`SIGDN_DESKTOPABSOLUTEPARSING`) does not always round-trip: for
/// "Home" it is `::{679F85CB-...}`, which `SHParseDisplayName` rejects, while the same
/// name with a `shell:` prefix works. Names that cannot be parsed back are not returned.
pub fn location_of(pidl: *const ITEMIDLIST) -> Option<Location> {
    let canonical = name_of(pidl, SIGDN_DESKTOPABSOLUTEPARSING)?;
    let loc = Location::new(canonical.clone());
    if loc.is_file_system() || Pidl::parse(&canonical).is_ok() {
        return Some(loc);
    }
    let prefixed = format!("shell:{canonical}");
    Pidl::parse(&prefixed).is_ok().then(|| Location::new(prefixed))
}

pub fn expand_env(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let src = HSTRING::from(s);
    unsafe {
        let n = ExpandEnvironmentStringsW(&src, None);
        if n == 0 {
            return s.to_string();
        }
        let mut buf = vec![0u16; n as usize];
        let n = ExpandEnvironmentStringsW(&src, Some(&mut buf));
        String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1)])
    }
}

/// Turn user input (path, `shell:` name, `::{CLSID}`, `%VAR%`) into a folder location.
/// File system folders become plain paths; virtual folders keep their parsing name.
pub fn resolve(input: &str) -> Option<Location> {
    let text = expand_env(input.trim().trim_matches('"'));
    if text.is_empty() {
        return None;
    }
    let loc = Location::new(text.clone());
    if loc.is_file_system() {
        let p = std::path::absolute(&text).ok()?;
        return p.is_dir().then(|| Location::new(normalize(&p.to_string_lossy())));
    }
    let pidl = Pidl::parse(&text).ok()?;
    // Keep the input itself if its canonical form cannot be parsed back.
    let resolved = location_of(pidl.as_ptr()).unwrap_or(loc);
    if resolved.is_file_system() {
        std::path::Path::new(resolved.as_str()).is_dir().then_some(resolved)
    } else {
        Some(resolved)
    }
}

/// Keep drive roots as "C:\" and strip trailing separators elsewhere.
pub fn normalize(p: &str) -> String {
    let t = p.trim_end_matches('\\');
    if t.len() == 2 && t.as_bytes()[1] == b':' {
        format!("{t}\\")
    } else {
        t.to_string()
    }
}

/// Human-readable name for tab titles and the address bar.
pub fn display_name(loc: &Location) -> String {
    if loc.is_file_system() {
        return loc.display_name().to_string();
    }
    Pidl::parse(loc.as_str())
        .ok()
        .and_then(|p| name_of(p.as_ptr(), SIGDN_NORMALDISPLAY))
        .unwrap_or_else(|| loc.as_str().to_string())
}

/// Absolute parsing name of a child of `parent` given its relative PIDL bytes.
pub fn child_location(parent: &Pidl, child: &[u8]) -> Option<Location> {
    unsafe {
        let abs = ILCombine(Some(parent.as_ptr()), Some(child.as_ptr() as *const ITEMIDLIST));
        if abs.is_null() {
            return None;
        }
        let abs = Pidl(abs);
        location_of(abs.as_ptr())
    }
}

pub fn known_folder(id: &GUID) -> Option<String> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

pub fn home() -> Location {
    known_folder(&FOLDERID_Profile)
        .map(Location::new)
        .unwrap_or_else(|| Location::new("C:\\"))
}

pub fn downloads() -> Location {
    known_folder(&FOLDERID_Downloads)
        .map(Location::new)
        .unwrap_or_else(home)
}
