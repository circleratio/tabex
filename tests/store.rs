use std::path::PathBuf;

use tabex::model::workspace::{Location, PaneSide};
use tabex::store::{self, config, paths, workspace_file};

/// Fresh temporary base directory per test.
fn temp_base(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tabex_test_store_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn defaults() -> (Location, Location) {
    (
        Location::new("C:\\Users\\me"),
        Location::new("C:\\Users\\me\\Downloads"),
    )
}

#[test]
fn first_run_creates_default() {
    let base = temp_base("first");
    let (l, r) = defaults();
    let opened = store::open_or_create(&base, None, (&l, &r));
    assert!(opened.error.is_none());
    assert_eq!(opened.workspace.name, "default");
    assert_eq!(opened.workspace.pane(PaneSide::Left).active().location, l);
    assert_eq!(opened.workspace.pane(PaneSide::Right).active().location, r);
    assert!(paths::workspace_path(&base, "default").is_file());
    assert_eq!(paths::list_workspaces(&base), ["default"]);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn save_leaves_no_tmp_and_round_trips() {
    let base = temp_base("save");
    let (l, r) = defaults();
    let mut ws = store::open_or_create(&base, Some("dev"), (&l, &r)).workspace;
    ws.open_tab(PaneSide::Left, Location::new("D:\\日本語フォルダ"), None, true);
    let path = paths::workspace_path(&base, "dev");
    workspace_file::save(&path, &ws).unwrap();
    workspace_file::save(&path, &ws).unwrap();
    let files: Vec<_> = std::fs::read_dir(paths::workspaces_dir(&base))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(files, ["dev.toml"]);
    let again = workspace_file::load(&path, "dev", &l).unwrap();
    assert_eq!(again.pane(PaneSide::Left).tabs.len(), 2);
    assert_eq!(
        again.pane(PaneSide::Left).active().location.as_str(),
        "D:\\日本語フォルダ"
    );
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn broken_file_is_not_overwritten() {
    let base = temp_base("broken");
    let path = paths::workspace_path(&base, "photo");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "xxx").unwrap();
    std::fs::write(paths::workspace_path(&base, "untitled"), "version = 1\n").unwrap();
    let (l, r) = defaults();
    let opened = store::open_or_create(&base, Some("photo"), (&l, &r));
    assert!(matches!(opened.error, Some(store::StoreError::Parse { .. })));
    assert_eq!(opened.workspace.name, "untitled-2");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xxx");
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn config_missing_then_saved() {
    let base = temp_base("config");
    let path = paths::config_path(&base);
    let mut c = config::load(&path).unwrap();
    assert_eq!(c, config::Config::default());
    c.touch_recent("dev");
    config::save(&path, &c).unwrap();
    assert_eq!(config::load(&path).unwrap().last_workspace.as_deref(), Some("dev"));
    std::fs::write(&path, "not toml [").unwrap();
    assert!(config::load(&path).is_err());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn tabex_home_overrides_base_dir() {
    // Only this test touches the variable in this binary.
    unsafe { std::env::set_var("TABEX_HOME", "C:\\tabex_home_test") };
    assert_eq!(paths::base_dir(), PathBuf::from("C:\\tabex_home_test"));
    unsafe { std::env::remove_var("TABEX_HOME") };
    assert!(paths::base_dir().ends_with("tabex"));
}
