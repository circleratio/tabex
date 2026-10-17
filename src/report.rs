//! User-facing messages from a GUI-subsystem process: stdout if redirected, the parent
//! console if there is one, otherwise a message box.

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Console::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

pub fn message(text: &str, error: bool) {
    let text = if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    };
    unsafe {
        let std = if error { STD_ERROR_HANDLE } else { STD_OUTPUT_HANDLE };
        if let Ok(h) = GetStdHandle(std)
            && !h.is_invalid()
            && !h.0.is_null()
            && write_handle(h, &text)
        {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok()
            && let Ok(h) = CreateFileW(
                w!("CONOUT$"),
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        {
            // Start on a fresh line: the shell prompt has usually been printed already.
            let ok = write_handle(h, &format!("\n{text}"));
            let _ = CloseHandle(h);
            if ok {
                return;
            }
        }
        let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
        MessageBoxW(None, &HSTRING::from(text.trim_end()), w!("tabex"), icon | MB_OK);
    }
}

unsafe fn write_handle(h: HANDLE, text: &str) -> bool {
    unsafe {
        let mut mode = CONSOLE_MODE::default();
        if GetConsoleMode(h, &mut mode).is_ok() {
            let wide: Vec<u16> = text.encode_utf16().collect();
            WriteConsoleW(h, &wide, None, None).is_ok()
        } else {
            let mut written = 0;
            WriteFile(h, Some(text.as_bytes()), Some(&mut written), None).is_ok()
        }
    }
}
