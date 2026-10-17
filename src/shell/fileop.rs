//! File operations through IFileOperation (run on the file operation worker thread).
//! A progress sink records what actually happened so the operation can be undone.

use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use super::undo::UndoOp;

/// Items to copy or move: plain paths, or a data object (clipboard / drag and drop).
pub enum Items {
    Paths(Vec<String>),
    Data(AgileReference<IDataObject>),
}

pub enum Op {
    Copy {
        items: Items,
        dest: String,
    },
    Move {
        items: Items,
        dest: String,
    },
    /// Move each (source, destination folder, new name) — used to undo moves.
    MoveEach(Vec<(String, String, String)>),
    Delete {
        paths: Vec<String>,
        permanent: bool,
    },
    Rename {
        path: String,
        new_name: String,
    },
    NewFolder {
        dest: String,
        name: String,
    },
    /// Delete the folder only if it is still empty (undo of NewFolder).
    DeleteIfEmpty(String),
    /// Put recycled items back to their original locations.
    Restore(Vec<String>),
}

#[derive(Debug, Default)]
pub struct OpResult {
    pub undo: Option<UndoOp>,
    pub error: Option<String>,
    pub aborted: bool,
}

#[derive(Default)]
struct Log {
    copied: Vec<String>,
    moved: Vec<(String, String)>,
    renamed: Vec<(String, String)>,
    created: Vec<String>,
    deleted: Vec<String>,
}

#[implement(IFileOperationProgressSink)]
struct Sink {
    log: Arc<Mutex<Log>>,
}

fn item_path(item: &IShellItem) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

fn ok_pair(hr: HRESULT, src: Ref<IShellItem>, created: Ref<IShellItem>) -> Option<(String, String)> {
    if hr.is_err() {
        return None;
    }
    Some((item_path(src.as_ref()?)?, item_path(created.as_ref()?)?))
}

#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _hr: HRESULT) -> Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _f: u32, _i: Ref<IShellItem>, _n: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostRenameItem(&self, _f: u32, i: Ref<IShellItem>, _n: &PCWSTR, hr: HRESULT, c: Ref<IShellItem>) -> Result<()> {
        if let Some(p) = ok_pair(hr, i, c) {
            self.log.lock().unwrap().renamed.push(p);
        }
        Ok(())
    }
    fn PreMoveItem(&self, _f: u32, _i: Ref<IShellItem>, _d: Ref<IShellItem>, _n: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _f: u32,
        i: Ref<IShellItem>,
        _d: Ref<IShellItem>,
        _n: &PCWSTR,
        hr: HRESULT,
        c: Ref<IShellItem>,
    ) -> Result<()> {
        if let Some(p) = ok_pair(hr, i, c) {
            self.log.lock().unwrap().moved.push(p);
        }
        Ok(())
    }
    fn PreCopyItem(&self, _f: u32, _i: Ref<IShellItem>, _d: Ref<IShellItem>, _n: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _f: u32,
        _i: Ref<IShellItem>,
        _d: Ref<IShellItem>,
        _n: &PCWSTR,
        hr: HRESULT,
        c: Ref<IShellItem>,
    ) -> Result<()> {
        if hr.is_ok()
            && let Some(p) = c.as_ref().and_then(item_path)
        {
            self.log.lock().unwrap().copied.push(p);
        }
        Ok(())
    }
    fn PreDeleteItem(&self, _f: u32, _i: Ref<IShellItem>) -> Result<()> {
        Ok(())
    }
    fn PostDeleteItem(&self, _f: u32, i: Ref<IShellItem>, hr: HRESULT, _c: Ref<IShellItem>) -> Result<()> {
        if hr.is_ok()
            && let Some(p) = i.as_ref().and_then(item_path)
        {
            self.log.lock().unwrap().deleted.push(p);
        }
        Ok(())
    }
    fn PreNewItem(&self, _f: u32, _d: Ref<IShellItem>, _n: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _f: u32,
        _d: Ref<IShellItem>,
        _n: &PCWSTR,
        _t: &PCWSTR,
        _a: u32,
        hr: HRESULT,
        item: Ref<IShellItem>,
    ) -> Result<()> {
        if hr.is_ok()
            && let Some(p) = item.as_ref().and_then(item_path)
        {
            self.log.lock().unwrap().created.push(p);
        }
        Ok(())
    }
    fn UpdateProgress(&self, _t: u32, _s: u32) -> Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}

pub fn shell_item(path: &str) -> Result<IShellItem> {
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None) }
}

fn file_name(path: &str) -> String {
    path.trim_end_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(path)
        .to_string()
}

fn parent_dir(path: &str) -> String {
    let t = path.trim_end_matches('\\');
    match t.rfind('\\') {
        Some(2) if t.as_bytes().get(1) == Some(&b':') => t[..3].to_string(),
        Some(i) => t[..i].to_string(),
        None => t.to_string(),
    }
}

/// Run one operation synchronously. `owner` parents the shell's progress and conflict UI.
pub fn run(owner: HWND, op: Op) -> OpResult {
    match run_inner(owner, &op) {
        Ok(r) => r,
        Err(e) => OpResult {
            error: Some(e.message()),
            ..Default::default()
        },
    }
}

fn run_inner(owner: HWND, op: &Op) -> Result<OpResult> {
    if let Op::Restore(originals) = op {
        return super::undo::restore_from_recycle_bin(owner, originals).map(|_| OpResult::default());
    }
    if let Op::DeleteIfEmpty(path) = op {
        let empty = std::fs::read_dir(path).map(|mut d| d.next().is_none()).unwrap_or(false);
        if !empty {
            return Ok(OpResult {
                error: Some("フォルダーが空でないため元に戻せません".into()),
                ..Default::default()
            });
        }
        return run_inner(
            owner,
            &Op::Delete {
                paths: vec![path.clone()],
                permanent: false,
            },
        );
    }
    unsafe {
        let fo: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        fo.SetOwnerWindow(owner)?;
        let mut flags = FOF_ALLOWUNDO.0 | FOFX_ADDUNDORECORD.0 | FOF_NOCONFIRMMKDIR.0;
        match op {
            Op::Delete { permanent: true, .. } => flags = FOF_NOCONFIRMMKDIR.0,
            Op::Delete { permanent: false, .. } => flags |= FOFX_RECYCLEONDELETE.0,
            _ => {}
        }
        fo.SetOperationFlags(FILEOPERATION_FLAGS(flags))?;
        let log = Arc::new(Mutex::new(Log::default()));
        let sink: IFileOperationProgressSink = Sink { log: log.clone() }.into();
        let cookie = fo.Advise(&sink)?;
        match op {
            Op::Copy { items, dest } | Op::Move { items, dest } => {
                let dest = shell_item(dest)?;
                let is_move = matches!(op, Op::Move { .. });
                match items {
                    Items::Paths(paths) => {
                        for p in paths {
                            let it = shell_item(p)?;
                            if is_move {
                                fo.MoveItem(&it, &dest, PCWSTR::null(), None)?;
                            } else {
                                fo.CopyItem(&it, &dest, PCWSTR::null(), None)?;
                            }
                        }
                    }
                    Items::Data(data) => {
                        let data: IDataObject = data.resolve()?;
                        if is_move {
                            fo.MoveItems(&data, &dest)?;
                        } else {
                            fo.CopyItems(&data, &dest)?;
                        }
                    }
                }
            }
            Op::MoveEach(list) => {
                for (src, dir, name) in list {
                    fo.MoveItem(
                        &shell_item(src)?,
                        &shell_item(dir)?,
                        &HSTRING::from(name.as_str()),
                        None,
                    )?;
                }
            }
            Op::Delete { paths, .. } => {
                for p in paths {
                    fo.DeleteItem(&shell_item(p)?, None)?;
                }
            }
            Op::Rename { path, new_name } => {
                fo.RenameItem(&shell_item(path)?, &HSTRING::from(new_name.as_str()), None)?;
            }
            Op::NewFolder { dest, name } => {
                fo.NewItem(
                    &shell_item(dest)?,
                    windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY.0,
                    &HSTRING::from(name.as_str()),
                    PCWSTR::null(),
                    None,
                )?;
            }
            Op::Restore(_) | Op::DeleteIfEmpty(_) => unreachable!(),
        }
        let performed = fo.PerformOperations();
        let aborted = fo.GetAnyOperationsAborted().map(|b| b.as_bool()).unwrap_or(false);
        let _ = fo.Unadvise(cookie);
        drop(sink);
        let log = std::mem::take(&mut *log.lock().unwrap());
        let undo = match op {
            Op::Copy { .. } if !log.copied.is_empty() => Some(UndoOp::Copied(log.copied)),
            Op::Move { .. } | Op::MoveEach(_) if !log.moved.is_empty() => Some(UndoOp::Moved(log.moved)),
            Op::Rename { .. } => log
                .renamed
                .into_iter()
                .next()
                .map(|(a, b)| UndoOp::Renamed { from: a, to: b }),
            Op::NewFolder { .. } => log.created.into_iter().next().map(UndoOp::NewFolder),
            Op::Delete { permanent: false, .. } if !log.deleted.is_empty() => Some(UndoOp::Recycled(log.deleted)),
            _ => None,
        };
        let error = match performed {
            Err(e) if e.code() != HRESULT::from_win32(ERROR_CANCELLED.0) && e.code() != COPYENGINE_E_USER_CANCELLED => {
                Some(e.message())
            }
            _ => None,
        };
        Ok(OpResult { undo, error, aborted })
    }
}

/// The operation that reverses `u`.
pub fn inverse(u: &UndoOp) -> Op {
    match u {
        UndoOp::Copied(created) => Op::Delete {
            paths: created.clone(),
            permanent: false,
        },
        UndoOp::Moved(pairs) => Op::MoveEach(
            pairs
                .iter()
                .map(|(from, to)| (to.clone(), parent_dir(from), file_name(from)))
                .collect(),
        ),
        UndoOp::Renamed { from, to } => Op::Rename {
            path: to.clone(),
            new_name: file_name(from),
        },
        UndoOp::NewFolder(path) => Op::DeleteIfEmpty(path.clone()),
        UndoOp::Recycled(originals) => Op::Restore(originals.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_ops() {
        match inverse(&UndoOp::Renamed {
            from: "C:\\a\\old.txt".into(),
            to: "C:\\a\\new.txt".into(),
        }) {
            Op::Rename { path, new_name } => {
                assert_eq!(path, "C:\\a\\new.txt");
                assert_eq!(new_name, "old.txt");
            }
            _ => panic!(),
        }
        match inverse(&UndoOp::Moved(vec![("C:\\x\\f.txt".into(), "D:\\y\\f (2).txt".into())])) {
            Op::MoveEach(v) => assert_eq!(
                v,
                [("D:\\y\\f (2).txt".to_string(), "C:\\x".to_string(), "f.txt".to_string())]
            ),
            _ => panic!(),
        }
        match inverse(&UndoOp::Moved(vec![("C:\\f.txt".into(), "D:\\f.txt".into())])) {
            Op::MoveEach(v) => assert_eq!(v[0].1, "C:\\"),
            _ => panic!(),
        }
    }
}
