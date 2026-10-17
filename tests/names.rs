use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tabex::fs::enumerate::{EnumEvent, EnumRequest, enumerate};
use tabex::model::workspace::SortSpec;
use tabex::shell::names::resolve;

fn com() {
    unsafe {
        let _ =
            windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
}

/// Every navigation tree root must resolve to a location that can be enumerated.
/// "Home" used to resolve to `::{679F85CB-...}`, which the shell cannot parse back.
#[test]
fn tree_roots_resolve_to_enumerable_locations() {
    com();
    for root in tabex::ui::navtree::ROOTS {
        let loc = resolve(root).unwrap_or_else(|| panic!("{root} did not resolve"));
        let req = EnumRequest {
            location: loc.clone(),
            sort: SortSpec::default(),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let mut failed = None;
        let mut done = false;
        enumerate(&req, &mut |ev| match ev {
            EnumEvent::Done => done = true,
            EnumEvent::Failed(e) => failed = Some(e),
            EnumEvent::Chunk(_) => {}
        });
        assert!(done && failed.is_none(), "{root} -> {loc:?}: {failed:?}");
    }
}

#[test]
fn file_system_input_is_normalized() {
    com();
    let loc = resolve("C:\\Windows\\").unwrap();
    assert_eq!(loc.as_str(), "C:\\Windows");
    assert_eq!(resolve("C:\\").unwrap().as_str(), "C:\\");
    assert!(resolve("C:\\no\\such\\tabex").is_none());
}
