//! Starting a shell (terminal) in a folder for ":sh".

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::SearchPathW;
use windows::Win32::System::Threading::*;
use windows::core::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellKind {
    WindowsTerminal,
    Pwsh,
    PowerShell,
    Cmd,
    /// Command line from `config.toml` `shell`.
    Custom(String),
}

/// Candidates tried in order when `config.toml` has no `shell`.
const AUTO: [(&str, ShellKind); 4] = [
    ("wt.exe", ShellKind::WindowsTerminal),
    ("pwsh.exe", ShellKind::Pwsh),
    ("powershell.exe", ShellKind::PowerShell),
    ("cmd.exe", ShellKind::Cmd),
];

/// The configured command if any, else the first candidate `find` reports as present.
pub fn choose(config: Option<&str>, find: impl Fn(&str) -> bool) -> ShellKind {
    if let Some(c) = config.map(str::trim).filter(|c| !c.is_empty()) {
        return ShellKind::Custom(c.to_string());
    }
    AUTO.iter()
        .find(|(exe, _)| find(exe))
        .map_or(ShellKind::Cmd, |(_, k)| k.clone())
}

/// Quote a folder for a command line; a trailing backslash would escape the closing quote.
fn quote_dir(dir: &str) -> String {
    if dir.ends_with('\\') {
        format!("\"{dir}.\"")
    } else {
        format!("\"{dir}\"")
    }
}

/// Command line to start; the folder is also passed as the current directory.
pub fn command_line(kind: &ShellKind, dir: &str) -> String {
    match kind {
        // Windows Terminal would otherwise use the profile's starting directory.
        ShellKind::WindowsTerminal => format!("wt.exe -d {}", quote_dir(dir)),
        ShellKind::Pwsh => "pwsh.exe".to_string(),
        ShellKind::PowerShell => "powershell.exe".to_string(),
        ShellKind::Cmd => "cmd.exe".to_string(),
        ShellKind::Custom(c) => c.clone(),
    }
}

/// First token of a command line (quotes removed).
fn program(cmd: &str) -> &str {
    let cmd = cmd.trim_start();
    if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else {
        cmd.split_whitespace().next().unwrap_or("")
    }
}

/// Command Prompt cannot use a UNC path as its current directory.
pub fn is_cmd(kind: &ShellKind) -> bool {
    match kind {
        ShellKind::Cmd => true,
        ShellKind::Custom(c) => {
            let p = program(c);
            let name = p.rsplit(['\\', '/']).next().unwrap_or(p);
            name.eq_ignore_ascii_case("cmd.exe") || name.eq_ignore_ascii_case("cmd")
        }
        _ => false,
    }
}

/// Whether `exe` can be found on the search path.
pub fn on_path(exe: &str) -> bool {
    let mut buf = [0u16; 1024];
    unsafe {
        SearchPathW(
            PCWSTR::null(),
            &HSTRING::from(exe),
            PCWSTR::null(),
            Some(&mut buf),
            None,
        ) > 0
    }
}

/// Start the shell in a new console window without waiting for it.
pub fn launch(kind: &ShellKind, dir: &str) -> std::result::Result<(), String> {
    let mut cmd: Vec<u16> = command_line(kind, dir)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NEW_CONSOLE,
            None,
            &HSTRING::from(dir),
            &si,
            &mut pi,
        )
        .map_err(|e| e.message())?;
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_order_and_config() {
        let only = |names: &'static [&'static str]| move |e: &str| names.contains(&e);
        assert_eq!(
            choose(None, only(&["wt.exe", "pwsh.exe", "cmd.exe"])),
            ShellKind::WindowsTerminal
        );
        assert_eq!(choose(None, only(&["pwsh.exe", "powershell.exe"])), ShellKind::Pwsh);
        assert_eq!(
            choose(None, only(&["powershell.exe", "cmd.exe"])),
            ShellKind::PowerShell
        );
        assert_eq!(choose(None, only(&[])), ShellKind::Cmd);
        assert_eq!(
            choose(Some(" nu.exe "), only(&["wt.exe"])),
            ShellKind::Custom("nu.exe".to_string())
        );
        // A blank setting means "not set".
        assert_eq!(choose(Some("  "), only(&["pwsh.exe"])), ShellKind::Pwsh);
    }

    #[test]
    fn command_lines() {
        assert_eq!(
            command_line(&ShellKind::WindowsTerminal, "C:\\src\\my app"),
            "wt.exe -d \"C:\\src\\my app\""
        );
        assert_eq!(command_line(&ShellKind::WindowsTerminal, "C:\\"), "wt.exe -d \"C:\\.\"");
        assert_eq!(command_line(&ShellKind::Pwsh, "C:\\x"), "pwsh.exe");
        assert_eq!(
            command_line(&ShellKind::Custom("pwsh.exe -NoLogo".into()), "C:\\x"),
            "pwsh.exe -NoLogo"
        );
    }

    #[test]
    fn cmd_detection() {
        assert!(is_cmd(&ShellKind::Cmd));
        assert!(is_cmd(&ShellKind::Custom("cmd.exe /k echo hi".into())));
        assert!(is_cmd(&ShellKind::Custom("CMD".into())));
        assert!(is_cmd(&ShellKind::Custom(
            "\"C:\\Windows\\System32\\cmd.exe\" /k".into()
        )));
        assert!(!is_cmd(&ShellKind::Custom("pwsh.exe -c cmd".into())));
        assert!(!is_cmd(&ShellKind::WindowsTerminal));
        assert!(!is_cmd(&ShellKind::PowerShell));
    }
}
