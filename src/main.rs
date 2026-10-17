#![windows_subsystem = "windows"]

use std::process::ExitCode;

use tabex::cli::{self, CliAction, CliError, RunArgs};
use tabex::instance::{self, Acquire};
use tabex::store::paths;
use tabex::{app, report};

fn main() -> ExitCode {
    install_panic_hook();
    let action = match cli::parse(std::env::args_os().skip(1)) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    let args = match action {
        CliAction::Help => {
            report::message(cli::USAGE, false);
            return ExitCode::SUCCESS;
        }
        CliAction::Version => {
            report::message(&cli::version_string(), false);
            return ExitCode::SUCCESS;
        }
        CliAction::Run(args) => args,
    };
    let base = paths::base_dir();
    let args = match validate(args, &base) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };

    let key = instance::instance_key(&base);
    let guard = match instance::acquire(key) {
        Ok(Acquire::First(g)) => g,
        Ok(Acquire::Running) => {
            return match instance::forward(key, &args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    report::message(&e.message(), true);
                    ExitCode::from(1)
                }
            };
        }
        Err(e) => {
            report::message(&format!("初期化に失敗しました: {}", e.message()), true);
            return ExitCode::from(1);
        }
    };

    let code = match app::run(base, key, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            report::message(&format!("初期化に失敗しました: {e}"), true);
            ExitCode::from(1)
        }
    };
    drop(guard);
    code
}

/// Write panics to `%LOCALAPPDATA%\tabex\crash.log` and tell the user. Unsaved workspace
/// changes are deliberately not written: the state may be inconsistent.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let dir = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("tabex");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("crash.log");
        let text = format!("tabex {} panicked: {info}\n{bt}\n", env!("CARGO_PKG_VERSION"));
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(text.as_bytes());
        }
        report::message(
            &format!("tabex が予期しないエラーで終了します。\n詳細: {}", path.display()),
            true,
        );
    }));
}

fn usage_error(e: &CliError) -> ExitCode {
    report::message(&format!("tabex: {e}\n\n{}", cli::USAGE), true);
    ExitCode::from(2)
}

/// Resolve folders to absolute paths and check that they and the workspace exist.
fn validate(mut args: RunArgs, base: &std::path::Path) -> Result<RunArgs, CliError> {
    for p in &mut args.paths {
        if p.starts_with("shell:") || p.starts_with("::") {
            continue;
        }
        let abs = std::path::absolute(&*p).map_err(|_| CliError::FolderNotFound(p.clone()))?;
        if !abs.is_dir() {
            return Err(CliError::FolderNotFound(p.clone()));
        }
        *p = abs.to_string_lossy().into_owned();
    }
    if let Some(ws) = &args.workspace
        && !paths::workspace_path(base, ws).is_file()
    {
        return Err(CliError::WorkspaceNotFound(ws.clone()));
    }
    Ok(args)
}
