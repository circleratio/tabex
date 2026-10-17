//! Undo records and restoring recycled items.

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::*;

use crate::fs::enumerate::{Pidl, bind_folder, display_name};

pub const MAX_UNDO: usize = 50;

/// What an operation did, in terms of full paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoOp {
    /// Items created by a copy.
    Copied(Vec<String>),
    /// (original path, new path) of moved items.
    Moved(Vec<(String, String)>),
    Renamed {
        from: String,
        to: String,
    },
    NewFolder(String),
    /// Original paths of items moved to the recycle bin.
    Recycled(Vec<String>),
}

impl UndoOp {
    pub fn describe(&self) -> String {
        match self {
            UndoOp::Copied(v) => format!("コピー ({} 項目)", v.len()),
            UndoOp::Moved(v) => format!("移動 ({} 項目)", v.len()),
            UndoOp::Renamed { .. } => "名前の変更".into(),
            UndoOp::NewFolder(_) => "新しいフォルダーの作成".into(),
            UndoOp::Recycled(v) => format!("削除 ({} 項目)", v.len()),
        }
    }
}

/// "Original location" column of the recycle bin (FMTID_Displaced, PID_DISPLACED_FROM).
const DISPLACED_FROM: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9b174b33_40ff_11d2_a27e_00c04fc30871),
    pid: 2,
};

fn variant_string(v: &VARIANT) -> Option<String> {
    unsafe {
        let mut out = PWSTR::null();
        VariantToStringAlloc(v).ok().map(|p| {
            out = p;
            let s = out.to_string().unwrap_or_default();
            CoTaskMemFree(Some(out.0 as _));
            s
        })
    }
}

/// Undelete recycled items whose original path is in `originals`.
pub fn restore_from_recycle_bin(owner: HWND, originals: &[String]) -> Result<()> {
    let bin = Pidl::parse("shell:RecycleBinFolder")?;
    let folder = bind_folder(&bin)?;
    let folder2: IShellFolder2 = folder.cast()?;
    let mut list: Option<IEnumIDList> = None;
    unsafe {
        folder
            .EnumObjects(
                HWND::default(),
                (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0 | SHCONTF_INCLUDEHIDDEN.0) as u32,
                &mut list,
            )
            .ok()?;
    }
    let Some(list) = list else { return Ok(()) };
    let wanted: Vec<String> = originals.iter().map(|s| s.to_lowercase()).collect();
    let mut found: Vec<Pidl> = Vec::new();
    loop {
        let mut child = [std::ptr::null_mut(); 1];
        let mut n = 0;
        if unsafe { list.Next(&mut child, Some(&mut n)) } != S_OK || n == 0 {
            break;
        }
        let child = Pidl(child[0]);
        // The editing name is the original file name with its extension, regardless of the
        // "hide extensions" setting.
        let Some(name) = display_name(&folder, child.as_ptr(), SHGDNF(SHGDN_INFOLDER.0 | SHGDN_FOREDITING.0)) else {
            continue;
        };
        let from =
            unsafe { folder2.GetDetailsEx(child.as_ptr(), &DISPLACED_FROM).ok() }.and_then(|v| variant_string(&v));
        let Some(from) = from else { continue };
        let full = crate::fs::join(&from, &name).to_lowercase();
        if wanted.contains(&full) {
            found.push(child);
        }
    }
    if found.is_empty() {
        return Err(Error::new(E_FAIL, "ごみ箱に元の項目が見つかりません"));
    }
    let ptrs: Vec<*const ITEMIDLIST> = found.iter().map(|p| p.as_ptr()).collect();
    unsafe {
        let cm: IContextMenu = folder.GetUIObjectOf(owner, &ptrs, None)?;
        let verb = s!("undelete");
        let info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            hwnd: owner,
            lpVerb: verb,
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        cm.InvokeCommand(&info)?;
    }
    Ok(())
}
