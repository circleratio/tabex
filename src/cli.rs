//! Command line parsing (pure).

use std::ffi::OsString;

use serde::{Deserialize, Serialize};

pub const USAGE: &str = "\
使いかた: tabex [オプション] [フォルダ...]

  フォルダ...               アクティブペインに新しいタブで開く（複数指定可）
  -w, --workspace <名前>    指定したワークスペースを開く
  -h, --help                この使いかたを表示する
  -V, --version             バージョンを表示する
";

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct RunArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run(RunArgs),
    Help,
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    #[error("不明なオプションです: {0}")]
    UnknownOption(String),
    #[error("{0} には値が必要です")]
    MissingValue(String),
    #[error("{0} が複数回指定されています")]
    Duplicate(String),
    #[error("引数に不正な文字が含まれています: {0}")]
    InvalidUnicode(String),
    #[error("フォルダが見つかりません: {0}")]
    FolderNotFound(String),
    #[error("ワークスペースが見つかりません: {0}")]
    WorkspaceNotFound(String),
}

pub fn version_string() -> String {
    format!("tabex {}", env!("CARGO_PKG_VERSION"))
}

/// Parse arguments, excluding the program name. `--help`/`--version` win over everything else.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<CliAction, CliError> {
    let mut run = RunArgs::default();
    let mut help = false;
    let mut version = false;
    let mut only_paths = false;
    let mut it = args.into_iter();
    let mut first_err: Option<CliError> = None;
    while let Some(raw) = it.next() {
        let arg = match raw.into_string() {
            Ok(s) => s,
            Err(os) => {
                first_err.get_or_insert(CliError::InvalidUnicode(os.to_string_lossy().into_owned()));
                continue;
            }
        };
        if only_paths || !arg.starts_with('-') || arg == "-" {
            run.paths.push(arg);
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        match flag.as_str() {
            "--" => only_paths = true,
            "-h" | "--help" | "/?" => help = true,
            "-V" | "--version" => version = true,
            "-w" | "--workspace" => {
                let value = match inline {
                    Some(v) => Some(v),
                    None => it.next().and_then(|v| v.into_string().ok()),
                };
                match value.filter(|v| !v.is_empty()) {
                    None => {
                        first_err.get_or_insert(CliError::MissingValue(flag.clone()));
                    }
                    Some(_) if run.workspace.is_some() => {
                        first_err.get_or_insert(CliError::Duplicate("--workspace".into()));
                    }
                    Some(v) => run.workspace = Some(v),
                }
            }
            _ => {
                first_err.get_or_insert(CliError::UnknownOption(arg.clone()));
            }
        }
    }
    if help {
        return Ok(CliAction::Help);
    }
    if version {
        return Ok(CliAction::Version);
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(CliAction::Run(run)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<CliAction, CliError> {
        parse(args.iter().map(OsString::from))
    }

    fn run(ws: Option<&str>, paths: &[&str]) -> Result<CliAction, CliError> {
        Ok(CliAction::Run(RunArgs {
            workspace: ws.map(str::to_string),
            paths: paths.iter().map(|s| s.to_string()).collect(),
        }))
    }

    #[test]
    fn no_args() {
        assert_eq!(p(&[]), run(None, &[]));
    }

    #[test]
    fn paths_and_workspace() {
        assert_eq!(
            p(&["C:\\a", "-w", "dev", "D:\\b"]),
            run(Some("dev"), &["C:\\a", "D:\\b"])
        );
        assert_eq!(p(&["--workspace=写真"]), run(Some("写真"), &[]));
        assert_eq!(p(&["--workspace", "dev"]), run(Some("dev"), &[]));
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(p(&["--", "-weird"]), run(None, &["-weird"]));
    }

    #[test]
    fn help_and_version_win() {
        assert_eq!(p(&["--help"]), Ok(CliAction::Help));
        assert_eq!(p(&["--bogus", "-h"]), Ok(CliAction::Help));
        assert_eq!(p(&["C:\\", "--version"]), Ok(CliAction::Version));
        assert_eq!(p(&["-V"]), Ok(CliAction::Version));
    }

    #[test]
    fn errors() {
        assert_eq!(p(&["--bogus"]), Err(CliError::UnknownOption("--bogus".into())));
        assert_eq!(p(&["-w"]), Err(CliError::MissingValue("-w".into())));
        assert_eq!(p(&["--workspace="]), Err(CliError::MissingValue("--workspace".into())));
        assert_eq!(
            p(&["-w", "a", "-w", "b"]),
            Err(CliError::Duplicate("--workspace".into()))
        );
    }

    #[test]
    fn run_args_round_trip_toml() {
        let a = RunArgs {
            workspace: Some("dev".into()),
            paths: vec!["C:\\a b".into()],
        };
        let s = toml::to_string(&a).unwrap();
        assert_eq!(toml::from_str::<RunArgs>(&s).unwrap(), a);
    }
}
