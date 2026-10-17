use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tabex::fs::FsError;
use tabex::fs::enumerate::{EnumEvent, EnumRequest, enumerate};
use tabex::model::listing::Entry;
use tabex::model::workspace::{Location, SortSpec};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tabex_test_enum_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(loc: &str) -> (Vec<Vec<Entry>>, Option<FsError>, bool) {
    let req = EnumRequest {
        location: Location::new(loc),
        sort: SortSpec::default(),
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let mut chunks = Vec::new();
    let mut err = None;
    let mut done = false;
    enumerate(&req, &mut |ev| match ev {
        EnumEvent::Chunk(c) => chunks.push(c),
        EnumEvent::Done => done = true,
        EnumEvent::Failed(e) => err = Some(e),
    });
    (chunks, err, done)
}

fn hide(path: &std::path::Path) {
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
    unsafe { SetFileAttributesW(&windows::core::HSTRING::from(path), FILE_ATTRIBUTE_HIDDEN).unwrap() };
}

#[test]
fn lists_japanese_hidden_and_long_paths() {
    let dir = temp_dir("mixed");
    std::fs::write(dir.join("日本語ファイル.txt"), "abc").unwrap();
    std::fs::write(dir.join("hidden.txt"), "").unwrap();
    hide(&dir.join("hidden.txt"));
    std::fs::create_dir(dir.join("sub")).unwrap();
    // Build a path longer than MAX_PATH through nested folders.
    let seg = "x".repeat(100);
    let mut deep = String::from(r"\\?\") + dir.to_str().unwrap();
    for _ in 0..3 {
        deep = format!("{deep}\\{seg}");
        std::fs::create_dir(&deep).unwrap();
    }
    std::fs::write(format!("{deep}\\long.txt"), "12345").unwrap();

    let (chunks, err, done) = run(dir.to_str().unwrap());
    assert!(err.is_none() && done);
    let all: Vec<Entry> = chunks.into_iter().flatten().collect();
    let names: Vec<String> = all.iter().map(Entry::name_string).collect();
    assert!(names.contains(&"日本語ファイル.txt".to_string()));
    let hidden = all.iter().find(|e| e.name_string() == "hidden.txt").unwrap();
    assert!(hidden.is_hidden());
    assert!(all.iter().find(|e| e.name_string() == "sub").unwrap().is_dir());
    assert_eq!(
        all.iter()
            .find(|e| e.name_string() == "日本語ファイル.txt")
            .unwrap()
            .size,
        3
    );
    assert!(!names.iter().any(|n| n == "." || n == ".."));

    let long_dir = deep.trim_start_matches(r"\\?\").to_string();
    assert!(long_dir.len() > 300);
    let (chunks, err, done) = run(&long_dir);
    assert!(err.is_none() && done, "{err:?}");
    let all: Vec<Entry> = chunks.into_iter().flatten().collect();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].size, 5);

    std::fs::remove_dir_all(format!(r"\\?\{}", dir.to_str().unwrap())).unwrap();
}

#[test]
fn ten_thousand_files_arrive_in_chunks() {
    let dir = temp_dir("many");
    for i in 0..10_000 {
        std::fs::File::create(dir.join(format!("f{i}.dat"))).unwrap();
    }
    let (chunks, err, done) = run(dir.to_str().unwrap());
    assert!(err.is_none() && done);
    assert!(chunks.len() >= 2, "got {} chunks", chunks.len());
    assert_eq!(chunks[0].len(), 1000);
    assert_eq!(chunks.iter().map(Vec::len).sum::<usize>(), 10_000);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn missing_folder_is_not_found() {
    let (_, err, done) = run("C:\\definitely\\not\\here\\tabex");
    assert_eq!(err, Some(FsError::NotFound));
    assert!(!done);
    let (_, err, _) = run("\\\\nonexistent-host-tabex\\share");
    assert_eq!(err, Some(FsError::NotFound), "UNC error: {err:?}");
}

#[test]
fn shell_folder_enumerates() {
    let (_, err, done) = run("::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"); // This PC
    assert!(err.is_none() && done, "{err:?}");
}

#[test]
fn cancelled_request_sends_nothing_after() {
    let dir = temp_dir("cancel");
    for i in 0..3000 {
        std::fs::File::create(dir.join(format!("f{i}"))).unwrap();
    }
    let cancelled = Arc::new(AtomicBool::new(true));
    let req = EnumRequest {
        location: Location::new(dir.to_str().unwrap()),
        sort: SortSpec::default(),
        cancelled,
    };
    let mut events = 0;
    enumerate(&req, &mut |_| events += 1);
    assert_eq!(events, 0);
    std::fs::remove_dir_all(&dir).unwrap();
}
