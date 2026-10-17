//! Shell data objects, clipboard formats and the clipboard itself.

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use crate::fs::enumerate::{Pidl, bind_folder};

pub const DROPEFFECT_COPY_: u32 = 1;
pub const DROPEFFECT_MOVE_: u32 = 2;

/// Children of a folder, as relative PIDLs owned by this struct.
pub struct Selection {
    pub folder: IShellFolder,
    pub folder_pidl: Pidl,
    pub children: Vec<Pidl>,
}

impl Selection {
    pub fn ptrs(&self) -> Vec<*const ITEMIDLIST> {
        self.children.iter().map(|p| p.as_ptr()).collect()
    }
}

/// Resolve the folder `location` and child items (file names, or raw relative PIDLs for
/// virtual folders).
pub fn selection(location: &str, names: &[String], raw: &[Box<[u8]>]) -> Result<Selection> {
    let folder_pidl = Pidl::parse(location)?;
    let folder = bind_folder(&folder_pidl)?;
    let mut children = Vec::new();
    for n in names {
        let mut pidl = std::ptr::null_mut();
        let mut wide: Vec<u16> = n.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            folder.ParseDisplayName(
                HWND::default(),
                None,
                PCWSTR(wide.as_mut_ptr()),
                None,
                &mut pidl,
                std::ptr::null_mut(),
            )?;
        }
        children.push(Pidl(pidl));
    }
    for r in raw {
        unsafe {
            let copy = ILClone(r.as_ptr() as *const ITEMIDLIST);
            children.push(Pidl(copy));
        }
    }
    Ok(Selection {
        folder,
        folder_pidl,
        children,
    })
}

pub fn data_object(sel: &Selection, owner: HWND) -> Result<IDataObject> {
    unsafe { sel.folder.GetUIObjectOf(owner, &sel.ptrs(), None) }
}

fn format(name: PCWSTR) -> u16 {
    unsafe { RegisterClipboardFormatW(name) as u16 }
}

pub fn preferred_drop_effect() -> u16 {
    format(w!("Preferred DropEffect"))
}

fn fmt_etc(cf: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

pub fn set_dword(obj: &IDataObject, name: PCWSTR, value: u32) -> Result<()> {
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, 4)?;
        let p = GlobalLock(h) as *mut u32;
        *p = value;
        let _ = GlobalUnlock(h);
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: h },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        };
        obj.SetData(&fmt_etc(format(name)), &medium, true)
    }
}

pub fn get_dword(obj: &IDataObject, name: PCWSTR) -> Option<u32> {
    unsafe {
        let mut medium = obj.GetData(&fmt_etc(format(name))).ok()?;
        let h = medium.u.hGlobal;
        let p = GlobalLock(h) as *const u32;
        let v = if p.is_null() { None } else { Some(*p) };
        let _ = GlobalUnlock(h);
        ReleaseStgMedium(&mut medium);
        v
    }
}

/// File system paths in a data object (CF_HDROP), if any.
pub fn hdrop_paths(obj: &IDataObject) -> Vec<String> {
    unsafe {
        let Ok(mut medium) = obj.GetData(&fmt_etc(CF_HDROP.0)) else {
            return Vec::new();
        };
        let hdrop = HDROP(medium.u.hGlobal.0);
        let n = DragQueryFileW(hdrop, u32::MAX, None);
        let mut out = Vec::new();
        for i in 0..n {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            out.push(String::from_utf16_lossy(&buf[..len]));
        }
        ReleaseStgMedium(&mut medium);
        out
    }
}

pub fn set_clipboard(obj: &IDataObject, cut: bool) -> Result<()> {
    set_dword(
        obj,
        w!("Preferred DropEffect"),
        if cut { DROPEFFECT_MOVE_ } else { DROPEFFECT_COPY_ },
    )?;
    unsafe { OleSetClipboard(obj) }
}

/// The clipboard contents and whether they were cut (move) rather than copied.
pub fn clipboard() -> Option<(IDataObject, bool)> {
    let obj = unsafe { OleGetClipboard().ok()? };
    let is_move = get_dword(&obj, w!("Preferred DropEffect")).is_some_and(|e| e & DROPEFFECT_MOVE_ != 0);
    Some((obj, is_move))
}

/// After a cut/paste completes, tell the source it can delete its copy and clear the clipboard.
pub fn finish_cut_paste(obj: &IDataObject) {
    let _ = set_dword(obj, w!("Performed DropEffect"), DROPEFFECT_MOVE_);
    let _ = set_dword(obj, w!("Paste Succeeded"), DROPEFFECT_MOVE_);
    unsafe {
        let _ = OleSetClipboard(None);
    }
}

/// Render our data on the clipboard before exit so it survives the process.
pub fn flush_if_owner(obj: Option<&IDataObject>) {
    if let Some(o) = obj {
        #[link(name = "ole32")]
        unsafe extern "system" {
            #[link_name = "OleIsCurrentClipboard"]
            fn ole_is_current_clipboard(p: *mut core::ffi::c_void) -> HRESULT;
        }
        unsafe {
            // S_OK means we still own the clipboard; S_FALSE means someone else does.
            if ole_is_current_clipboard(o.as_raw()) == S_OK {
                let _ = OleFlushClipboard();
            }
        }
    }
}
