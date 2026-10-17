//! Entry comparators. Names use CompareStringEx with SORT_DIGITSASNUMBERS, which gives the
//! same natural order as StrCmpLogicalW (Explorer) without needing NUL-terminated strings.

use std::cmp::Ordering;

use windows::Win32::Globalization::*;

use crate::model::listing::Entry;
use crate::model::workspace::{ColumnId, SortOrder, SortSpec};

pub fn compare_names(a: &[u16], b: &[u16]) -> Ordering {
    let r = unsafe {
        CompareStringEx(
            windows::core::PCWSTR::null(),
            NORM_IGNORECASE | SORT_DIGITSASNUMBERS,
            a,
            b,
            None,
            None,
            None,
        )
    };
    match r {
        COMPARESTRING_RESULT(1) => Ordering::Less,
        COMPARESTRING_RESULT(3) => Ordering::Greater,
        COMPARESTRING_RESULT(2) => Ordering::Equal,
        // Failure: fall back to ordinal comparison so sorting stays total.
        _ => a.cmp(b),
    }
}

/// Folders first regardless of direction; ties broken by ascending name.
pub fn comparator(sort: SortSpec) -> impl Fn(&Entry, &Entry) -> Ordering + Send + Sync + Copy {
    move |a: &Entry, b: &Entry| {
        let dirs = b.is_dir().cmp(&a.is_dir());
        if dirs != Ordering::Equal {
            return dirs;
        }
        let name = || compare_names(&a.name, &b.name);
        let key = match sort.column {
            ColumnId::Name => name(),
            ColumnId::Size => a.size.cmp(&b.size),
            ColumnId::Modified => a.modified.cmp(&b.modified),
            ColumnId::Created => a.created.cmp(&b.created),
            ColumnId::Type | ColumnId::Extension => compare_names(a.extension(), b.extension()),
            ColumnId::Attributes => a.attrs.cmp(&b.attrs),
        };
        let key = if sort.order == SortOrder::Desc {
            key.reverse()
        } else {
            key
        };
        key.then_with(|| {
            if sort.column == ColumnId::Name {
                Ordering::Equal
            } else {
                name()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::listing::ATTR_DIRECTORY;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn sorted(mut v: Vec<Entry>, spec: SortSpec) -> Vec<String> {
        let c = comparator(spec);
        v.sort_by(|a, b| c(a, b));
        v.iter().map(Entry::name_string).collect()
    }

    fn spec(column: ColumnId, order: SortOrder) -> SortSpec {
        SortSpec { column, order }
    }

    #[test]
    fn natural_and_case_insensitive() {
        assert_eq!(compare_names(&w("file2"), &w("file10")), Ordering::Less);
        assert_eq!(compare_names(&w("ABC"), &w("abc")), Ordering::Equal);
        assert_eq!(compare_names(&w("a"), &w("B")), Ordering::Less);
    }

    #[test]
    fn folders_first_both_directions() {
        let v = vec![
            Entry::new("b.txt", 0),
            Entry::new("zdir", ATTR_DIRECTORY),
            Entry::new("a.txt", 0),
        ];
        assert_eq!(
            sorted(v.clone(), spec(ColumnId::Name, SortOrder::Asc)),
            ["zdir", "a.txt", "b.txt"]
        );
        assert_eq!(
            sorted(v, spec(ColumnId::Name, SortOrder::Desc)),
            ["zdir", "b.txt", "a.txt"]
        );
    }

    #[test]
    fn size_ties_by_name_ascending() {
        let mk = |n: &str, s| {
            let mut e = Entry::new(n, 0);
            e.size = s;
            e
        };
        let v = vec![mk("c", 5), mk("a", 5), mk("b", 1)];
        assert_eq!(sorted(v.clone(), spec(ColumnId::Size, SortOrder::Asc)), ["b", "a", "c"]);
        assert_eq!(sorted(v, spec(ColumnId::Size, SortOrder::Desc)), ["a", "c", "b"]);
    }

    #[test]
    fn by_extension() {
        let v = vec![Entry::new("x.txt", 0), Entry::new("y.doc", 0), Entry::new("z", 0)];
        assert_eq!(sorted(v, spec(ColumnId::Type, SortOrder::Asc)), ["z", "y.doc", "x.txt"]);
    }
}
