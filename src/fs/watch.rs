//! Folder change notifications with ReadDirectoryChangesW on one background thread.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::*;
use windows::Win32::System::Threading::*;
use windows::core::*;

use super::enumerate::stat;
use super::extended_path;
use crate::model::listing::Entry;

pub const COALESCE: Duration = Duration::from_millis(100);
const BUF_WORDS: usize = 16 * 1024; // 64KB, the maximum for network shares

#[derive(Debug)]
pub enum Change {
    /// Created or modified: the current metadata.
    Upsert(Entry),
    Removed(Vec<u16>),
}

#[derive(Debug)]
pub enum WatchEvent {
    Changes {
        key: u64,
        changes: Vec<Change>,
    },
    /// Too many changes to report individually: re-enumerate.
    Overflow {
        key: u64,
    },
    /// Watching failed (e.g. folder deleted, network share): stop relying on notifications.
    Failed {
        key: u64,
    },
}

enum Cmd {
    Watch { key: u64, dir: String },
    Unwatch { key: u64 },
    Quit,
}

struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}

pub struct Watcher {
    tx: Sender<Cmd>,
    wake: HANDLE,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    pub fn new(sink: impl Fn(WatchEvent) + Send + 'static) -> Result<Watcher> {
        let wake = unsafe { CreateEventW(None, false, false, None)? };
        let (tx, rx) = channel();
        let w = SendHandle(wake);
        let thread = std::thread::Builder::new()
            .name("watch".into())
            .spawn(move || {
                let w = w;
                run(rx, w.0, &sink)
            })
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        Ok(Watcher {
            tx,
            wake,
            thread: Some(thread),
        })
    }

    pub fn watch(&self, key: u64, dir: &str) {
        self.send(Cmd::Watch {
            key,
            dir: dir.to_string(),
        });
    }

    pub fn unwatch(&self, key: u64) {
        self.send(Cmd::Unwatch { key });
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
        unsafe {
            let _ = SetEvent(self.wake);
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        unsafe {
            let _ = CloseHandle(self.wake);
        }
    }
}

struct Watch {
    key: u64,
    dir: String,
    handle: HANDLE,
    overlapped: Box<OVERLAPPED>,
    buf: Box<[u32; BUF_WORDS]>,
    /// name -> exists after the pending changes
    pending: HashMap<Vec<u16>, bool>,
    first_pending: Option<Instant>,
}

impl Watch {
    fn open(key: u64, dir: &str) -> Option<Watch> {
        unsafe {
            let handle = CreateFileW(
                &HSTRING::from(extended_path(dir)),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
            .ok()?;
            let event = CreateEventW(None, true, false, None).ok()?;
            let mut w = Watch {
                key,
                dir: dir.to_string(),
                handle,
                overlapped: Box::new(OVERLAPPED {
                    hEvent: event,
                    ..Default::default()
                }),
                buf: Box::new([0; BUF_WORDS]),
                pending: HashMap::new(),
                first_pending: None,
            };
            if !w.arm() {
                w.close();
                return None;
            }
            Some(w)
        }
    }

    fn arm(&mut self) -> bool {
        unsafe {
            let _ = ResetEvent(self.overlapped.hEvent);
            ReadDirectoryChangesW(
                self.handle,
                self.buf.as_mut_ptr() as *mut _,
                (BUF_WORDS * 4) as u32,
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES,
                None,
                Some(&mut *self.overlapped),
                None,
            )
            .is_ok()
        }
    }

    /// Parse completed notifications. Returns false on overflow.
    fn collect(&mut self, bytes: usize) -> bool {
        if bytes == 0 {
            return false;
        }
        let base = self.buf.as_ptr() as *const u8;
        let mut off = 0usize;
        loop {
            unsafe {
                let info = &*(base.add(off) as *const FILE_NOTIFY_INFORMATION);
                let name =
                    std::slice::from_raw_parts(info.FileName.as_ptr(), info.FileNameLength as usize / 2).to_vec();
                let exists = !matches!(info.Action, FILE_ACTION_REMOVED | FILE_ACTION_RENAMED_OLD_NAME);
                self.pending.insert(name, exists);
                if info.NextEntryOffset == 0 {
                    break;
                }
                off += info.NextEntryOffset as usize;
            }
        }
        self.first_pending.get_or_insert_with(Instant::now);
        true
    }

    fn take_changes(&mut self) -> Vec<Change> {
        self.first_pending = None;
        self.pending
            .drain()
            .map(|(name, exists)| {
                let found = if exists {
                    stat(&self.dir, &String::from_utf16_lossy(&name))
                } else {
                    None
                };
                match found {
                    Some(e) => Change::Upsert(e),
                    None => Change::Removed(name),
                }
            })
            .collect()
    }

    fn close(&mut self) {
        unsafe {
            let _ = CancelIoEx(self.handle, Some(&*self.overlapped));
            let mut n = 0;
            // Wait for the cancelled I/O so the kernel no longer writes into `buf`.
            let _ = GetOverlappedResult(self.handle, &*self.overlapped, &mut n, true);
            let _ = CloseHandle(self.handle);
            let _ = CloseHandle(self.overlapped.hEvent);
        }
    }
}

fn run(rx: Receiver<Cmd>, wake: HANDLE, sink: &dyn Fn(WatchEvent)) {
    let mut watches: Vec<Watch> = Vec::new();
    loop {
        let now = Instant::now();
        let timeout = watches
            .iter()
            .filter_map(|w| w.first_pending)
            .map(|t| COALESCE.saturating_sub(now.duration_since(t)).as_millis() as u32)
            .min()
            .unwrap_or(INFINITE);
        let mut handles = vec![wake];
        handles.extend(watches.iter().map(|w| w.overlapped.hEvent));
        let r = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
        let idx = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if idx == 0 {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    Cmd::Quit => {
                        for w in &mut watches {
                            w.close();
                        }
                        return;
                    }
                    Cmd::Watch { key, dir } => {
                        if let Some(i) = watches.iter().position(|w| w.key == key) {
                            if watches[i].dir.eq_ignore_ascii_case(&dir) {
                                continue;
                            }
                            watches.remove(i).close();
                        }
                        match Watch::open(key, &dir) {
                            Some(w) => watches.push(w),
                            None => sink(WatchEvent::Failed { key }),
                        }
                    }
                    Cmd::Unwatch { key } => {
                        if let Some(i) = watches.iter().position(|w| w.key == key) {
                            watches.remove(i).close();
                        }
                    }
                }
            }
        } else if idx < handles.len() {
            let i = idx - 1;
            let w = &mut watches[i];
            let mut bytes = 0u32;
            let ok = unsafe { GetOverlappedResult(w.handle, &*w.overlapped, &mut bytes, false) }.is_ok();
            let collected = ok && w.collect(bytes as usize);
            if ok && !collected {
                w.pending.clear();
                w.first_pending = None;
                sink(WatchEvent::Overflow { key: w.key });
            }
            if !ok || !w.arm() {
                let key = w.key;
                watches.remove(i).close();
                sink(WatchEvent::Failed { key });
            }
        }
        // Flush coalesced changes whose quiet period has elapsed.
        let now = Instant::now();
        for w in &mut watches {
            if w.first_pending.is_some_and(|t| now.duration_since(t) >= COALESCE) {
                let changes = w.take_changes();
                if !changes.is_empty() {
                    sink(WatchEvent::Changes { key: w.key, changes });
                }
            }
        }
    }
}
