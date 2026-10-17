//! System image lists and per-extension icon/type-name caches.

use std::collections::HashMap;

use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES};
use windows::Win32::UI::Controls::HIMAGELIST;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use crate::model::listing::Entry;

/// System image lists. The big ones are fetched on first use: they cost memory per icon.
pub struct ImageLists {
    pub small: HIMAGELIST,
    pub large: HIMAGELIST,
    extra_large: std::cell::Cell<Option<HIMAGELIST>>,
    jumbo: std::cell::Cell<Option<HIMAGELIST>>,
}

fn system_list(i: u32) -> HIMAGELIST {
    unsafe {
        SHGetImageList::<windows::Win32::UI::Controls::IImageList>(i as i32)
            .map(|il| HIMAGELIST(il.as_raw() as _))
            .unwrap_or_default()
    }
}

impl ImageLists {
    pub fn extra_large(&self) -> HIMAGELIST {
        let h = self.extra_large.get().unwrap_or_else(|| system_list(SHIL_EXTRALARGE));
        self.extra_large.set(Some(h));
        h
    }

    pub fn jumbo(&self) -> HIMAGELIST {
        let h = self.jumbo.get().unwrap_or_else(|| system_list(SHIL_JUMBO));
        self.jumbo.set(Some(h));
        h
    }
}

pub fn image_lists() -> ImageLists {
    ImageLists {
        small: system_list(SHIL_SMALL),
        large: system_list(SHIL_LARGE),
        extra_large: std::cell::Cell::new(None),
        jumbo: std::cell::Cell::new(None),
    }
}

fn file_info(path: PCWSTR, attrs: FILE_FLAGS_AND_ATTRIBUTES, flags: SHGFI_FLAGS) -> Option<SHFILEINFOW> {
    let mut info = SHFILEINFOW::default();
    let r = unsafe {
        SHGetFileInfoW(
            path,
            attrs,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    (r != 0).then_some(info)
}

fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// Icon index of an absolute PIDL.
pub fn pidl_icon(pidl: *const ITEMIDLIST) -> i32 {
    file_info(
        PCWSTR(pidl as *const u16),
        FILE_FLAGS_AND_ATTRIBUTES(0),
        SHGFI_PIDL | SHGFI_SYSICONINDEX | SHGFI_SMALLICON,
    )
    .map(|i| i.iIcon)
    .unwrap_or(0)
}

/// Icon index of a real file (reads the file; use off the UI thread for custom icons).
pub fn path_icon(path: &str) -> i32 {
    let p = HSTRING::from(path);
    file_info(
        PCWSTR(p.as_ptr()),
        FILE_FLAGS_AND_ATTRIBUTES(0),
        SHGFI_SYSICONINDEX | SHGFI_SMALLICON,
    )
    .map(|i| i.iIcon)
    .unwrap_or(-1)
}

/// Extensions whose icon depends on the file itself rather than its type.
pub fn has_custom_icon(e: &Entry) -> bool {
    let ext = String::from_utf16_lossy(e.extension()).to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "exe" | "ico" | "lnk" | "url" | "cur" | "ani" | "msc" | "appref-ms"
    )
}

#[derive(Default)]
pub struct IconCache {
    by_ext: HashMap<Vec<u16>, i32>,
    type_names: HashMap<Vec<u16>, String>,
    folder: Option<i32>,
    folder_type: Option<String>,
}

impl IconCache {
    fn ext_key(e: &Entry) -> Vec<u16> {
        let ext = e.extension();
        let mut k: Vec<u16> = vec![b'.' as u16];
        k.extend(ext.iter().map(|&c| {
            if (b'A' as u16..=b'Z' as u16).contains(&c) {
                c + 32
            } else {
                c
            }
        }));
        k
    }

    /// Fast icon from the file type only (no disk access).
    pub fn generic_icon(&mut self, e: &Entry) -> i32 {
        if e.is_dir() {
            return *self.folder.get_or_insert_with(|| {
                file_info(
                    w!("folder"),
                    FILE_ATTRIBUTE_DIRECTORY,
                    SHGFI_USEFILEATTRIBUTES | SHGFI_SYSICONINDEX | SHGFI_SMALLICON,
                )
                .map(|i| i.iIcon)
                .unwrap_or(0)
            });
        }
        let key = Self::ext_key(e);
        if let Some(&i) = self.by_ext.get(&key) {
            return i;
        }
        let mut z = key.clone();
        z.push(0);
        let icon = file_info(
            PCWSTR(z.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            SHGFI_USEFILEATTRIBUTES | SHGFI_SYSICONINDEX | SHGFI_SMALLICON,
        )
        .map(|i| i.iIcon)
        .unwrap_or(0);
        self.by_ext.insert(key, icon);
        icon
    }

    pub fn type_name(&mut self, e: &Entry) -> String {
        if e.is_dir() {
            return self
                .folder_type
                .get_or_insert_with(|| {
                    file_info(
                        w!("folder"),
                        FILE_ATTRIBUTE_DIRECTORY,
                        SHGFI_USEFILEATTRIBUTES | SHGFI_TYPENAME,
                    )
                    .map(|i| wstr(&i.szTypeName))
                    .unwrap_or_default()
                })
                .clone();
        }
        let key = Self::ext_key(e);
        if let Some(s) = self.type_names.get(&key) {
            return s.clone();
        }
        let mut z = key.clone();
        z.push(0);
        let name = file_info(
            PCWSTR(z.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            SHGFI_USEFILEATTRIBUTES | SHGFI_TYPENAME,
        )
        .map(|i| wstr(&i.szTypeName))
        .unwrap_or_default();
        self.type_names.insert(key, name.clone());
        name
    }
}
