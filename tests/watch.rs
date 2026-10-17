use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use tabex::fs::watch::{Change, WatchEvent, Watcher};

fn wait_for(rx: &std::sync::mpsc::Receiver<WatchEvent>, pred: impl Fn(&Change) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(WatchEvent::Changes { key: 7, changes }) if changes.iter().any(&pred) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

fn name_is(n: &str) -> impl Fn(&[u16]) -> bool + '_ {
    move |w| String::from_utf16_lossy(w) == n
}

#[test]
fn reports_create_rename_delete() {
    let dir = std::env::temp_dir().join(format!("tabex_test_watch_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (tx, rx) = channel();
    let w = Watcher::new(move |ev| {
        let _ = tx.send(ev);
    })
    .unwrap();
    w.watch(7, dir.to_str().unwrap());
    std::thread::sleep(Duration::from_millis(200));

    std::fs::write(dir.join("a.txt"), "hello").unwrap();
    assert!(wait_for(
        &rx,
        |c| matches!(c, Change::Upsert(e) if name_is("a.txt")(&e.name) && e.size == 5)
    ));

    std::fs::rename(dir.join("a.txt"), dir.join("b.txt")).unwrap();
    // Rename arrives as removal of the old name and upsert of the new one.
    let deadline = Instant::now() + Duration::from_secs(2);
    let (mut removed, mut added) = (false, false);
    while !(removed && added) && Instant::now() < deadline {
        if let Ok(WatchEvent::Changes { changes, .. }) = rx.recv_timeout(Duration::from_millis(500)) {
            for c in changes {
                match c {
                    Change::Removed(n) if name_is("a.txt")(&n) => removed = true,
                    Change::Upsert(e) if name_is("b.txt")(&e.name) => added = true,
                    _ => {}
                }
            }
        }
    }
    assert!(removed && added);

    std::fs::remove_file(dir.join("b.txt")).unwrap();
    assert!(wait_for(
        &rx,
        |c| matches!(c, Change::Removed(n) if name_is("b.txt")(n))
    ));

    w.unwatch(7);
    drop(w);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn missing_folder_fails() {
    let (tx, rx) = channel();
    let w = Watcher::new(move |ev| {
        let _ = tx.send(ev);
    })
    .unwrap();
    w.watch(9, "C:\\no\\such\\tabex\\dir");
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(2)),
        Ok(WatchEvent::Failed { key: 9 })
    ));
}
