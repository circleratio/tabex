//! File system access: enumeration, sorting and change notifications.

pub mod enumerate;
pub mod sort;
pub mod watch;

use windows::Win32::Foundation::*;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FsError {
    #[error("フォルダが見つかりません")]
    NotFound,
    #[error("アクセスが拒否されました")]
    AccessDenied,
    #[error("{0}")]
    Other(String),
}

impl FsError {
    pub fn from_win32(code: WIN32_ERROR) -> FsError {
        match code {
            // Unreachable network hosts report a variety of codes; all mean "not there now".
            ERROR_FILE_NOT_FOUND
            | ERROR_PATH_NOT_FOUND
            | ERROR_BAD_NETPATH
            | ERROR_BAD_NET_NAME
            | ERROR_INVALID_NAME
            | ERROR_DIRECTORY
            | ERROR_INVALID_DRIVE
            | ERROR_REM_NOT_LIST
            | ERROR_NETNAME_DELETED
            | ERROR_NETWORK_UNREACHABLE
            | ERROR_HOST_UNREACHABLE
            | ERROR_NO_NET_OR_BAD_PATH
            | ERROR_NOT_CONNECTED => FsError::NotFound,
            ERROR_ACCESS_DENIED => FsError::AccessDenied,
            other => FsError::Other(windows::core::Error::from(other.to_hresult()).message()),
        }
    }

    pub fn from_error(e: &windows::core::Error) -> FsError {
        let hr = e.code().0 as u32;
        // HRESULT_FROM_WIN32 values carry the Win32 code in the low word.
        if hr & 0xFFFF_0000 == 0x8007_0000 {
            FsError::from_win32(WIN32_ERROR(hr & 0xFFFF))
        } else {
            FsError::Other(e.message())
        }
    }
}

/// Add the `\\?\` prefix to long absolute paths so that Win32 file APIs accept them
/// regardless of the system LongPathsEnabled policy.
pub fn extended_path(path: &str) -> String {
    if path.len() < 240 || path.starts_with("\\\\?\\") {
        return path.to_string();
    }
    let path = path.replace('/', "\\");
    if let Some(unc) = path.strip_prefix("\\\\") {
        format!("\\\\?\\UNC\\{unc}")
    } else {
        format!("\\\\?\\{path}")
    }
}

/// Join a folder location and a child name.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('\\') {
        format!("{dir}{name}")
    } else {
        format!("{dir}\\{name}")
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended() {
        assert_eq!(extended_path("C:\\a"), "C:\\a");
        let long = format!("C:\\{}", "a".repeat(300));
        assert_eq!(extended_path(&long), format!("\\\\?\\{long}"));
        let unc = format!("\\\\srv\\share\\{}", "b".repeat(300));
        assert!(extended_path(&unc).starts_with("\\\\?\\UNC\\srv\\share\\"));
    }

    #[test]
    fn joins() {
        assert_eq!(join("C:\\", "a"), "C:\\a");
        assert_eq!(join("C:\\x", "a"), "C:\\x\\a");
    }
}
