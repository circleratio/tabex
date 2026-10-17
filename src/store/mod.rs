//! Persistence of workspaces and settings under `%APPDATA%\tabex` (or `%TABEX_HOME%`).

pub mod autosave;
pub mod config;
pub mod paths;
pub mod workspace_file;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("{path}: unsupported version {version} (this build supports {supported})")]
    UnsupportedVersion {
        path: PathBuf,
        version: i64,
        supported: i64,
    },
    #[error("{0}")]
    InvalidName(#[from] paths::NameError),
}

impl StoreError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        StoreError::Io {
            path: path.into(),
            source,
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
    }
}

pub const DEFAULT_WORKSPACE: &str = "default";

/// Result of opening the workspace to show at startup or after a switch.
#[derive(Debug)]
pub struct Opened {
    pub workspace: crate::model::workspace::Workspace,
    /// Set when the requested file could not be read; a fresh workspace was created instead.
    pub error: Option<StoreError>,
}

/// Load `name` (default: "default"). A missing file creates and saves a new workspace with
/// `defaults` as the left/right folders. An unreadable file is left untouched and a fresh
/// workspace with an unused "untitled" name is returned together with the error.
pub fn open_or_create(
    base: &std::path::Path,
    name: Option<&str>,
    defaults: (&crate::model::workspace::Location, &crate::model::workspace::Location),
) -> Opened {
    use crate::model::workspace::Workspace;
    let name = name.unwrap_or(DEFAULT_WORKSPACE);
    let path = paths::workspace_path(base, name);
    let fresh = |n: &str| {
        let ws = Workspace::new(n, defaults.0.clone(), defaults.1.clone());
        // Saving can fail (read-only profile); the in-memory workspace is still usable.
        let _ = workspace_file::save(&paths::workspace_path(base, n), &ws);
        ws
    };
    match workspace_file::load(&path, name, defaults.0) {
        Ok(workspace) => Opened { workspace, error: None },
        Err(e) if e.is_not_found() => Opened {
            workspace: fresh(name),
            error: None,
        },
        Err(e) => {
            let n = paths::unique_name("untitled", &paths::list_workspaces(base));
            Opened {
                workspace: fresh(&n),
                error: Some(e),
            }
        }
    }
}

/// Write `contents` so that readers never observe a partially written file.
pub fn write_atomic(path: &std::path::Path, contents: &str) -> Result<(), StoreError> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| StoreError::io(dir, e))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        drop(f);
        replace_file(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(StoreError::io(path, e));
    }
    Ok(())
}

fn replace_file(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
    use windows::core::HSTRING;
    unsafe {
        MoveFileExW(
            &HSTRING::from(from),
            &HSTRING::from(to),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xffff))
    }
}
