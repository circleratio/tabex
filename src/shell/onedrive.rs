//! OneDrive accounts configured on this PC, read from the registry for the navigation tree.

use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::*;
use windows::core::*;

const ACCOUNTS_KEY: PCWSTR = w!("Software\\Microsoft\\OneDrive\\Accounts");
const PERSONAL_KEY: &str = "Personal";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The "Personal" key; the others are work or school accounts ("Business1", ...).
    pub personal: bool,
    /// Sync folder (UserFolder).
    pub folder: String,
    /// "::{CLSID}" of the namespace item Explorer shows in its navigation pane.
    pub namespace: Option<String>,
    /// Organization name of a work or school account (DisplayName).
    pub org: Option<String>,
}

/// REG_SZ value `name` of `HKCU\...\Accounts\<sub>`; None when missing or empty.
fn read_string(accounts: HKEY, sub: &str, name: PCWSTR) -> Option<String> {
    let sub = HSTRING::from(sub);
    let mut size = 0u32;
    unsafe {
        RegGetValueW(accounts, &sub, name, RRF_RT_REG_SZ, None, None, Some(&mut size))
            .ok()
            .ok()?;
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        RegGetValueW(
            accounts,
            &sub,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        (!s.trim().is_empty()).then_some(s)
    }
}

fn subkeys(key: HKEY) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

/// Accounts that have an existing sync folder; empty when OneDrive is not set up or the
/// registry cannot be read. Keys without a folder (an unused "Business1", "FileCoAuth")
/// are skipped.
pub fn accounts() -> Vec<Account> {
    let mut key = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, ACCOUNTS_KEY, None, KEY_READ, &mut key) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for sub in subkeys(key) {
        let Some(folder) = read_string(key, &sub, w!("UserFolder")) else {
            continue;
        };
        if !std::path::Path::new(&folder).is_dir() {
            continue;
        }
        out.push(Account {
            personal: sub.eq_ignore_ascii_case(PERSONAL_KEY),
            folder,
            namespace: read_string(key, &sub, w!("NamespaceRootId")).map(|id| format!("::{id}")),
            org: read_string(key, &sub, w!("DisplayName")),
        });
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    out
}

/// Name used when the namespace item cannot be resolved.
pub fn fallback_name(a: &Account) -> String {
    match (&a.org, a.personal) {
        (_, true) => "OneDrive - Personal".to_string(),
        (Some(org), false) => format!("OneDrive - {org}"),
        (None, false) => "OneDrive".to_string(),
    }
}

/// Personal first, then work or school accounts by their shown name in natural order.
/// Later entries whose folder equals an earlier one (ignoring case) are dropped.
pub fn order<T>(mut items: Vec<(Account, T)>, name: impl Fn(&T) -> &str) -> Vec<(Account, T)> {
    items.sort_by(|(a, x), (b, y)| {
        b.personal.cmp(&a.personal).then_with(|| {
            let (x, y): (Vec<u16>, Vec<u16>) = (name(x).encode_utf16().collect(), name(y).encode_utf16().collect());
            crate::fs::sort::compare_names(&x, &y)
        })
    });
    let mut seen: Vec<String> = Vec::new();
    items.retain(|(a, _)| {
        let key = a.folder.trim_end_matches('\\').to_lowercase();
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        true
    });
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(personal: bool, folder: &str, org: Option<&str>) -> Account {
        Account {
            personal,
            folder: folder.to_string(),
            namespace: None,
            org: org.map(str::to_string),
        }
    }

    #[test]
    fn fallback_names() {
        assert_eq!(fallback_name(&account(true, r"C:\o", None)), "OneDrive - Personal");
        assert_eq!(
            fallback_name(&account(false, r"C:\o", Some("Contoso"))),
            "OneDrive - Contoso"
        );
        assert_eq!(fallback_name(&account(false, r"C:\o", None)), "OneDrive");
    }

    #[test]
    fn personal_first_then_natural_order() {
        let items = vec![
            (account(false, r"C:\b10", None), "OneDrive - Org10".to_string()),
            (account(false, r"C:\b2", None), "OneDrive - Org2".to_string()),
            (account(true, r"C:\p", None), "OneDrive - Personal".to_string()),
            (account(false, r"C:\a", None), "OneDrive - Contoso".to_string()),
        ];
        let names: Vec<String> = order(items, |s| s.as_str()).into_iter().map(|(_, n)| n).collect();
        assert_eq!(
            names,
            [
                "OneDrive - Personal",
                "OneDrive - Contoso",
                "OneDrive - Org2",
                "OneDrive - Org10"
            ]
        );
    }

    #[test]
    fn duplicate_folders_keep_the_first() {
        let items = vec![
            (
                account(true, r"C:\Users\me\OneDrive", None),
                "OneDrive - Personal".to_string(),
            ),
            (
                account(false, r"c:\users\ME\OneDrive\", None),
                "OneDrive - X".to_string(),
            ),
            (
                account(false, r"C:\Users\me\OneDrive - X", None),
                "OneDrive - Y".to_string(),
            ),
        ];
        let names: Vec<String> = order(items, |s| s.as_str()).into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, ["OneDrive - Personal", "OneDrive - Y"]);
    }

    #[test]
    fn empty_is_empty() {
        assert!(order(Vec::<(Account, String)>::new(), |s| s.as_str()).is_empty());
    }
}
