//! Folder enumeration. Plain paths use FindFirstFileExW; shell locations use IShellFolder.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Shell::Common::{ITEMIDLIST, STRRET};
use windows::Win32::UI::Shell::*;
use windows::core::*;

use super::{FsError, extended_path, join};
use crate::model::listing::{ATTR_DIRECTORY, ATTR_HIDDEN, Entry};
use crate::model::workspace::{Location, SortSpec};

/// The first chunk is sent as soon as this many entries are found so the view fills quickly.
pub const FIRST_CHUNK_ITEMS: usize = 1000;
/// Later chunks are sent at this interval. Merging is O(n) per chunk on the UI thread, so
/// fewer, larger chunks are cheaper than many small ones.
pub const CHUNK_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum EnumEvent {
    Chunk(Vec<Entry>),
    Done,
    Failed(FsError),
}

pub struct EnumRequest {
    pub location: Location,
    pub sort: SortSpec,
    pub cancelled: Arc<AtomicBool>,
}

/// Enumerate a folder, delivering entries in sorted chunks. Stops early when cancelled.
pub fn enumerate(req: &EnumRequest, sink: &mut dyn FnMut(EnumEvent)) {
    let cmp = super::sort::comparator(req.sort);
    let mut batch = Batcher {
        buf: Vec::new(),
        last: Instant::now(),
        sent_first: false,
        sink,
        cmp: &cmp,
    };
    let result = if req.location.is_file_system() {
        enumerate_fs(req.location.as_str(), &req.cancelled, &mut |e| batch.push(e))
    } else {
        enumerate_shell(req.location.as_str(), &req.cancelled, &mut |e| batch.push(e))
    };
    if req.cancelled.load(Ordering::Relaxed) {
        return;
    }
    batch.flush();
    match result {
        Ok(()) => (batch.sink)(EnumEvent::Done),
        Err(e) => (batch.sink)(EnumEvent::Failed(e)),
    }
}

struct Batcher<'a> {
    buf: Vec<Entry>,
    last: Instant,
    sent_first: bool,
    sink: &'a mut dyn FnMut(EnumEvent),
    cmp: &'a dyn Fn(&Entry, &Entry) -> std::cmp::Ordering,
}

impl Batcher<'_> {
    fn push(&mut self, e: Entry) {
        self.buf.push(e);
        if (!self.sent_first && self.buf.len() >= FIRST_CHUNK_ITEMS) || self.last.elapsed() >= CHUNK_INTERVAL {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if !self.buf.is_empty() {
            let mut chunk = std::mem::take(&mut self.buf);
            chunk.sort_by(|a, b| (self.cmp)(a, b));
            (self.sink)(EnumEvent::Chunk(chunk));
            self.sent_first = true;
        }
        self.last = Instant::now();
    }
}

fn entry_from_find(d: &WIN32_FIND_DATAW) -> Entry {
    let len = d.cFileName.iter().position(|&c| c == 0).unwrap_or(d.cFileName.len());
    Entry {
        name: d.cFileName[..len].into(),
        attrs: d.dwFileAttributes,
        size: if d.dwFileAttributes & ATTR_DIRECTORY != 0 {
            0
        } else {
            ((d.nFileSizeHigh as u64) << 32) | d.nFileSizeLow as u64
        },
        modified: filetime(d.ftLastWriteTime),
        created: filetime(d.ftCreationTime),
        shell_id: None,
        icon: -1,
        alive: true,
    }
}

pub fn filetime(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

fn is_dot(name: &[u16]) -> bool {
    name == [b'.' as u16] || name == [b'.' as u16, b'.' as u16]
}

fn enumerate_fs(dir: &str, cancelled: &AtomicBool, out: &mut dyn FnMut(Entry)) -> std::result::Result<(), FsError> {
    let pattern = HSTRING::from(extended_path(&join(dir, "*")));
    let mut data = WIN32_FIND_DATAW::default();
    let handle = unsafe {
        FindFirstFileExW(
            &pattern,
            FindExInfoBasic,
            &mut data as *mut _ as *mut _,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    let handle = match handle {
        Ok(h) => h,
        Err(e) => {
            let err = FsError::from_error(&e);
            // An empty drive root has no "." entries and reports FILE_NOT_FOUND.
            if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() && std::path::Path::new(dir).is_dir() {
                return Ok(());
            }
            return Err(err);
        }
    };
    loop {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let e = entry_from_find(&data);
        if !is_dot(&e.name) {
            out(e);
        }
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break;
        }
    }
    unsafe {
        let _ = FindClose(handle);
    }
    Ok(())
}

/// Metadata of a single file, used for change notifications.
pub fn stat(dir: &str, name: &str) -> Option<Entry> {
    let path = HSTRING::from(extended_path(&join(dir, name)));
    let mut data = WIN32_FIND_DATAW::default();
    unsafe {
        let h = FindFirstFileExW(
            &path,
            FindExInfoBasic,
            &mut data as *mut _ as *mut _,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_FLAGS(0),
        )
        .ok()?;
        let _ = FindClose(h);
    }
    Some(entry_from_find(&data))
}

/// Owned absolute PIDL freed with CoTaskMemFree.
pub struct Pidl(pub *mut ITEMIDLIST);

impl Pidl {
    pub fn parse(name: &str) -> Result<Pidl> {
        let mut pidl = std::ptr::null_mut();
        unsafe { SHParseDisplayName(&HSTRING::from(name), None, &mut pidl, 0, None)? };
        Ok(Pidl(pidl))
    }

    pub fn as_ptr(&self) -> *const ITEMIDLIST {
        self.0
    }
}

impl Drop for Pidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CoTaskMemFree(Some(self.0 as _)) };
        }
    }
}

/// Bind to the IShellFolder of an absolute PIDL (the desktop for an empty PIDL).
pub fn bind_folder(pidl: &Pidl) -> Result<IShellFolder> {
    unsafe {
        let desktop = SHGetDesktopFolder()?;
        if (*pidl.0).mkid.cb == 0 {
            return Ok(desktop);
        }
        desktop.BindToObject(pidl.0, None)
    }
}

pub fn strret_to_string(sr: &mut STRRET, child: *const ITEMIDLIST) -> Option<String> {
    unsafe {
        let mut p = PWSTR::null();
        StrRetToStrW(sr, Some(child), &mut p).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

pub fn display_name(folder: &IShellFolder, child: *const ITEMIDLIST, flags: SHGDNF) -> Option<String> {
    unsafe {
        let mut sr = STRRET::default();
        folder.GetDisplayNameOf(child, flags, &mut sr).ok()?;
        strret_to_string(&mut sr, child)
    }
}

/// Size in bytes of a relative PIDL including the terminator.
pub fn pidl_bytes(child: *const ITEMIDLIST) -> Box<[u8]> {
    unsafe {
        let size = ILGetSize(Some(child)) as usize;
        std::slice::from_raw_parts(child as *const u8, size).into()
    }
}

fn enumerate_shell(name: &str, cancelled: &AtomicBool, out: &mut dyn FnMut(Entry)) -> std::result::Result<(), FsError> {
    let pidl = Pidl::parse(name).map_err(|e| match FsError::from_error(&e) {
        FsError::Other(_) => FsError::NotFound,
        other => other,
    })?;
    let folder = bind_folder(&pidl).map_err(|e| FsError::from_error(&e))?;
    let mut list: Option<IEnumIDList> = None;
    let flags = SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0 | SHCONTF_INCLUDEHIDDEN.0;
    let hr = unsafe { folder.EnumObjects(HWND::default(), flags as u32, &mut list) };
    if hr.is_err() {
        return Err(FsError::from_error(&Error::from(hr)));
    }
    let Some(list) = list else {
        return Ok(()); // S_FALSE: nothing to enumerate
    };
    loop {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let mut child = [std::ptr::null_mut(); 1];
        let mut fetched = 0;
        if unsafe { list.Next(&mut child, Some(&mut fetched)) } != S_OK || fetched == 0 {
            break;
        }
        let child = Pidl(child[0]);
        if let Some(e) = shell_entry(&folder, child.as_ptr()) {
            out(e);
        }
    }
    Ok(())
}

fn shell_entry(folder: &IShellFolder, child: *const ITEMIDLIST) -> Option<Entry> {
    let name = display_name(folder, child, SHGDN_INFOLDER)?;
    let mut attrs = (SFGAO_FOLDER | SFGAO_HIDDEN | SFGAO_FILESYSTEM | SFGAO_STREAM).0;
    unsafe {
        let arr = [child];
        let _ = folder.GetAttributesOf(&arr, &mut attrs);
    }
    let mut e = Entry::new(&name, 0);
    // Zip files are folders in the shell namespace but files to the user.
    if attrs & SFGAO_FOLDER.0 != 0 && attrs & SFGAO_STREAM.0 == 0 {
        e.attrs |= ATTR_DIRECTORY;
    }
    if attrs & SFGAO_HIDDEN.0 != 0 {
        e.attrs |= ATTR_HIDDEN;
    }
    if attrs & SFGAO_FILESYSTEM.0 != 0
        && let Some(path) = display_name(folder, child, SHGDN_FORPARSING)
    {
        let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
        if unsafe {
            GetFileAttributesExW(
                &HSTRING::from(extended_path(&path)),
                GetFileExInfoStandard,
                &mut data as *mut _ as *mut _,
            )
        }
        .is_ok()
        {
            e.attrs |= data.dwFileAttributes & !ATTR_DIRECTORY;
            if data.dwFileAttributes & ATTR_DIRECTORY == 0 {
                e.size = ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64;
            }
            e.modified = filetime(data.ftLastWriteTime);
            e.created = filetime(data.ftCreationTime);
        }
    }
    e.shell_id = Some(pidl_bytes(child));
    Some(e)
}
