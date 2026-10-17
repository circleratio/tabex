//! Optional rustmigemo.dll next to the exe: romaji to Japanese patterns for narrowing.

use std::ffi::{CStr, c_char, c_void};
use std::path::Path;

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;

pub const DLL_NAME: &str = "rustmigemo.dll";
pub const DICT_NAME: &str = "migemo-compact-dict";

/// Mirrors `Migemo` in rustmigemo's `src/lib.rs` (windows-dll feature).
#[repr(C, align(8))]
struct Desc {
    dict: *mut c_void,
    result_ptr: *mut c_char,
    result_len: u32,
}

type LoadFn = unsafe extern "C" fn(buffer: *const u8, len: u32) -> Desc;
type QueryFn = unsafe extern "C" fn(m: *mut Desc, buffer: *const u8, len: u32) -> bool;
type DestroyFn = unsafe extern "C" fn(m: *mut Desc);

#[derive(Debug, thiserror::Error)]
pub enum MigemoError {
    #[error("{0} を読み込めません: {1}")]
    Dll(String, String),
    #[error("辞書 {0} を読み込めません: {1}")]
    Dict(String, String),
}

/// A loaded dictionary. UI thread only.
pub struct Migemo {
    lib: HMODULE,
    desc: Desc,
    query: QueryFn,
    destroy: DestroyFn,
}

fn dll_error(msg: impl ToString) -> MigemoError {
    MigemoError::Dll(DLL_NAME.to_string(), msg.to_string())
}

fn dict_error(msg: impl ToString) -> MigemoError {
    MigemoError::Dict(DICT_NAME.to_string(), msg.to_string())
}

/// Load `dir\rustmigemo.dll` with `dir\migemo-compact-dict`; `Ok(None)` when there is no DLL.
pub fn load(dir: &Path) -> std::result::Result<Option<Migemo>, MigemoError> {
    let dll = dir.join(DLL_NAME);
    if !dll.is_file() {
        return Ok(None);
    }
    let lib = unsafe {
        LoadLibraryExW(
            &HSTRING::from(dll.as_os_str()),
            None,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    }
    .map_err(|e| dll_error(e.message()))?;
    let free = |e: MigemoError| {
        unsafe {
            let _ = FreeLibrary(lib);
        }
        e
    };
    let proc_ = |name: PCSTR| unsafe { GetProcAddress(lib, name) };
    let (Some(load), Some(query), Some(destroy)) = (proc_(s!("load")), proc_(s!("query")), proc_(s!("destroy"))) else {
        return Err(free(dll_error("load / query / destroy 関数がありません")));
    };
    type Farproc = unsafe extern "system" fn() -> isize;
    let (load, query, destroy) = unsafe {
        (
            std::mem::transmute::<Farproc, LoadFn>(load),
            std::mem::transmute::<Farproc, QueryFn>(query),
            std::mem::transmute::<Farproc, DestroyFn>(destroy),
        )
    };
    let bytes = std::fs::read(dir.join(DICT_NAME)).map_err(|e| free(dict_error(e)))?;
    // An empty buffer would make the DLL panic, which aborts the process across the FFI boundary.
    if bytes.is_empty() {
        return Err(free(dict_error("ファイルが空です")));
    }
    let len = u32::try_from(bytes.len()).map_err(|_| free(dict_error("ファイルが大きすぎます")))?;
    let desc = unsafe { load(bytes.as_ptr(), len) };
    Ok(Some(Migemo {
        lib,
        desc,
        query,
        destroy,
    }))
}

impl Migemo {
    /// Pattern for `text`, None when the DLL reports failure or gives nothing back.
    pub fn query(&mut self, text: &str) -> Option<String> {
        let len = u32::try_from(text.len()).ok()?;
        let ok = unsafe { (self.query)(&mut self.desc, text.as_ptr(), len) };
        if !ok || self.desc.result_ptr.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(self.desc.result_ptr) }
            .to_string_lossy()
            .into_owned();
        (!s.is_empty()).then_some(s)
    }
}

impl Drop for Migemo {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(&mut self.desc);
            let _ = FreeLibrary(self.lib);
        }
    }
}
