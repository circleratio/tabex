//! Single instance enforcement and command line forwarding via WM_COPYDATA.

use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::cli::RunArgs;

pub const MAIN_CLASS: PCWSTR = w!("TabexMainWindow");
pub const COPYDATA_RUN_ARGS: usize = 0x7AB0_0001;
const KEY_PROP: PCWSTR = w!("TabexInstanceKey");

/// Instances are keyed by the base directory so that a `TABEX_HOME` test instance can run
/// alongside the normal one without sharing workspace files.
pub fn instance_key(base: &Path) -> u32 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    base.to_string_lossy().to_lowercase().hash(&mut h);
    h.finish() as u32
}

pub enum Acquire {
    /// We are the first instance. Keep the guard alive for the process lifetime.
    First(InstanceGuard),
    /// Another instance owns the key.
    Running,
}

pub struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn acquire(key: u32) -> Result<Acquire> {
    let name = HSTRING::from(format!("Local\\tabex-single-instance-{key:08x}"));
    unsafe {
        let h = CreateMutexW(None, true, &name)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            Ok(Acquire::Running)
        } else {
            Ok(Acquire::First(InstanceGuard(h)))
        }
    }
}

/// Tag our main window so that `forward` finds the right instance.
pub fn tag_window(hwnd: HWND, key: u32) {
    unsafe {
        let _ = SetPropW(hwnd, KEY_PROP, Some(HANDLE(key as usize as *mut _)));
    }
}

fn find_window(key: u32) -> Option<HWND> {
    struct Search {
        key: u32,
        found: Option<HWND>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        unsafe {
            let s = &mut *(lp.0 as *mut Search);
            let mut buf = [0u16; 32];
            let n = GetClassNameW(hwnd, &mut buf) as usize;
            if buf[..n] == *MAIN_CLASS.as_wide() && GetPropW(hwnd, KEY_PROP).0 as usize as u32 == s.key {
                s.found = Some(hwnd);
                return FALSE;
            }
            TRUE
        }
    }
    let mut s = Search { key, found: None };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut s as *mut _ as isize));
    }
    s.found
}

/// Send `args` to the running instance. Waits briefly in case it is still starting up.
pub fn forward(key: u32, args: &RunArgs) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let hwnd = loop {
        if let Some(h) = find_window(key) {
            break h;
        }
        if Instant::now() > deadline {
            return Err(Error::new(E_FAIL, "起動中の tabex のウィンドウが見つかりません"));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let payload = toml::to_string(args).unwrap_or_default();
    let cds = COPYDATASTRUCT {
        dwData: COPYDATA_RUN_ARGS,
        cbData: payload.len() as u32,
        lpData: payload.as_ptr() as *mut _,
    };
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);
        let mut result = 0usize;
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const _ as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&mut result),
        );
    }
    Ok(())
}

/// Decode a WM_COPYDATA payload received by the main window.
///
/// # Safety
/// `lp` must be the LPARAM of a WM_COPYDATA message.
pub unsafe fn decode(lp: LPARAM) -> Option<RunArgs> {
    unsafe {
        let cds = &*(lp.0 as *const COPYDATASTRUCT);
        if cds.dwData != COPYDATA_RUN_ARGS || cds.lpData.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
        toml::from_str(std::str::from_utf8(bytes).ok()?).ok()
    }
}
