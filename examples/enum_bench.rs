//! Enumeration benchmark.
//!
//! cargo run --release --example enum_bench -- <dir> [--create N]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use tabex::fs::enumerate::{EnumEvent, EnumRequest, enumerate};
use tabex::model::listing::{Filter, Listing};
use tabex::model::workspace::{Location, SortSpec};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = args.first() else {
        eprintln!("usage: enum_bench <dir> [--create N]");
        std::process::exit(2);
    };
    if let Some(i) = args.iter().position(|a| a == "--create") {
        let n: usize = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
        std::fs::create_dir_all(dir).unwrap();
        let existing = std::fs::read_dir(dir).unwrap().count();
        let t = Instant::now();
        for k in existing..n {
            std::fs::File::create(format!("{dir}\\file{k:06}.dat")).unwrap();
        }
        println!("created {} files in {:?}", n.saturating_sub(existing), t.elapsed());
    }

    let sort = SortSpec::default();
    let cmp = tabex::fs::sort::comparator(sort);
    let req = EnumRequest {
        location: Location::new(dir.as_str()),
        sort,
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let mut listing = Listing::new(1, Filter::default());
    let t = Instant::now();
    let mut first = None;
    let mut chunks = 0;
    let mut merge_time = std::time::Duration::ZERO;
    enumerate(&req, &mut |ev| match ev {
        EnumEvent::Chunk(c) => {
            first.get_or_insert_with(|| t.elapsed());
            chunks += 1;
            let m = Instant::now();
            listing.append_chunk(c, &cmp);
            merge_time += m.elapsed();
        }
        EnumEvent::Done => {}
        EnumEvent::Failed(e) => eprintln!("failed: {e}"),
    });
    let total = t.elapsed();
    let r = Instant::now();
    listing.resort(&cmp);
    let resort = r.elapsed();
    println!("entries:      {}", listing.len());
    println!("chunks:       {chunks}");
    println!("first chunk:  {:?}", first.unwrap_or_default());
    println!("total:        {total:?} (merge on consumer side: {merge_time:?})");
    println!("full resort:  {resort:?}");
    for spec in [
        SortSpec {
            column: tabex::model::workspace::ColumnId::Name,
            order: tabex::model::workspace::SortOrder::Desc,
        },
        SortSpec {
            column: tabex::model::workspace::ColumnId::Type,
            order: tabex::model::workspace::SortOrder::Asc,
        },
        SortSpec {
            column: tabex::model::workspace::ColumnId::Name,
            order: tabex::model::workspace::SortOrder::Asc,
        },
    ] {
        let c = tabex::fs::sort::comparator(spec);
        let r = Instant::now();
        listing.resort(&c);
        println!("resort {:?} {:?}: {:?}", spec.column, spec.order, r.elapsed());
    }
    // Worst case: 100k entries in random name order.
    let mut seed = 12345u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed >> 33
    };
    let synthetic: Vec<tabex::model::listing::Entry> = (0..100_000)
        .map(|_| tabex::model::listing::Entry::new(&format!("{:x}_{}.dat", rnd(), rnd() % 1000), 0))
        .collect();
    let mut l = Listing::new(2, Filter::default());
    let r = Instant::now();
    l.append_chunk(synthetic, &cmp);
    println!("random 100k sort (append): {:?}", r.elapsed());
}
