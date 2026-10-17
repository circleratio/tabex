//! Folder contents of a tab: an append-only entry store plus a sorted, filtered view.

use std::cmp::Ordering;
use std::collections::HashSet;

pub const ATTR_READONLY: u32 = 0x1;
pub const ATTR_HIDDEN: u32 = 0x2;
pub const ATTR_SYSTEM: u32 = 0x4;
pub const ATTR_DIRECTORY: u32 = 0x10;
pub const ATTR_ARCHIVE: u32 = 0x20;

/// `Entry::icon` value meaning "draw the cached thumbnail" (I_IMAGENONE for the list view).
pub const THUMB_ICON: i32 = -2;

#[derive(Clone, Debug, Default)]
pub struct Entry {
    /// UTF-16 file name without terminator.
    pub name: Box<[u16]>,
    pub attrs: u32,
    pub size: u64,
    /// FILETIME (100ns since 1601-01-01 UTC).
    pub modified: u64,
    pub created: u64,
    /// Relative ITEMIDLIST bytes for virtual (shell) folders.
    pub shell_id: Option<Box<[u8]>>,
    /// System image list index, -1 while unresolved.
    pub icon: i32,
    pub alive: bool,
}

impl Entry {
    pub fn new(name: &str, attrs: u32) -> Self {
        Entry {
            name: name.encode_utf16().collect(),
            attrs,
            icon: -1,
            alive: true,
            ..Default::default()
        }
    }

    pub fn is_dir(&self) -> bool {
        self.attrs & ATTR_DIRECTORY != 0
    }

    pub fn is_hidden(&self) -> bool {
        self.attrs & ATTR_HIDDEN != 0
    }

    /// Name starts with '.', hidden by UNIX convention.
    pub fn is_dotfile(&self) -> bool {
        is_dot_name(&self.name)
    }

    pub fn name_string(&self) -> String {
        String::from_utf16_lossy(&self.name)
    }

    /// Extension without the dot, empty for folders and names without one.
    pub fn extension(&self) -> &[u16] {
        if self.is_dir() {
            return &[];
        }
        match self.name.iter().rposition(|&c| c == b'.' as u16) {
            Some(0) | None => &[],
            Some(i) => &self.name[i + 1..],
        }
    }
}

pub fn is_dot_name(name: &[u16]) -> bool {
    name.first() == Some(&(b'.' as u16))
}

pub type Cmp<'a> = &'a dyn Fn(&Entry, &Entry) -> Ordering;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub show_hidden: bool,
    pub show_dotfiles: bool,
}

impl Filter {
    /// Hidden-attribute and dot-file checks are independent; both must pass.
    pub fn accepts(&self, e: &Entry) -> bool {
        e.alive && (self.show_hidden || !e.is_hidden()) && (self.show_dotfiles || !e.is_dotfile())
    }
}

/// Positions are indices into the visible `order`; ids are indices into `entries`.
#[derive(Debug, Default)]
pub struct Listing {
    pub generation: u64,
    pub complete: bool,
    /// Set when the folder may have changed while it was not watched.
    pub stale: bool,
    entries: Vec<Entry>,
    order: Vec<u32>,
    filter: Filter,
    selection: HashSet<u32>,
    dead: usize,
}

impl Listing {
    pub fn new(generation: u64, filter: Filter) -> Self {
        Listing {
            generation,
            filter,
            ..Default::default()
        }
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn get(&self, pos: usize) -> Option<&Entry> {
        self.order.get(pos).map(|&id| &self.entries[id as usize])
    }

    pub fn get_mut(&mut self, pos: usize) -> Option<&mut Entry> {
        let id = *self.order.get(pos)?;
        Some(&mut self.entries[id as usize])
    }

    pub fn id_at(&self, pos: usize) -> Option<u32> {
        self.order.get(pos).copied()
    }

    pub fn entry(&self, id: u32) -> &Entry {
        &self.entries[id as usize]
    }

    pub fn position_of(&self, id: u32) -> Option<usize> {
        self.order.iter().position(|&x| x == id)
    }

    pub fn order(&self) -> &[u32] {
        &self.order
    }

    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// Visible entries in display order.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.order.iter().map(|&id| &self.entries[id as usize])
    }

    pub fn find(&self, name: &[u16]) -> Option<u32> {
        self.entries
            .iter()
            .position(|e| e.alive && &*e.name == name)
            .map(|i| i as u32)
    }

    /// Append a chunk and merge it into the sorted view in O(n + k log k).
    pub fn append_chunk(&mut self, chunk: Vec<Entry>, cmp: Cmp) {
        let start = self.entries.len() as u32;
        self.entries.extend(chunk);
        let entries = &self.entries;
        let mut fresh: Vec<u32> = (start..entries.len() as u32)
            .filter(|&id| self.filter.accepts(&entries[id as usize]))
            .collect();
        fresh.sort_by(|&a, &b| cmp(&entries[a as usize], &entries[b as usize]));
        if self.order.is_empty() {
            self.order = fresh;
            return;
        }
        let old = std::mem::take(&mut self.order);
        let mut merged = Vec::with_capacity(old.len() + fresh.len());
        let (mut i, mut j) = (0, 0);
        while i < old.len() && j < fresh.len() {
            if cmp(&entries[fresh[j] as usize], &entries[old[i] as usize]) == Ordering::Less {
                merged.push(fresh[j]);
                j += 1;
            } else {
                merged.push(old[i]);
                i += 1;
            }
        }
        merged.extend_from_slice(&old[i..]);
        merged.extend_from_slice(&fresh[j..]);
        self.order = merged;
    }

    /// Rebuild the view from scratch (sort order or filter changed).
    pub fn resort(&mut self, cmp: Cmp) {
        let entries = &self.entries;
        let mut order: Vec<u32> = (0..entries.len() as u32)
            .filter(|&id| self.filter.accepts(&entries[id as usize]))
            .collect();
        order.sort_by(|&a, &b| cmp(&entries[a as usize], &entries[b as usize]));
        self.selection.retain(|id| self.filter.accepts(&entries[*id as usize]));
        self.order = order;
    }

    pub fn set_filter(&mut self, filter: Filter, cmp: Cmp) {
        if self.filter != filter {
            self.filter = filter;
            self.resort(cmp);
        }
    }

    fn insert_into_order(&mut self, id: u32, cmp: Cmp) -> Option<usize> {
        let e = &self.entries[id as usize];
        if !self.filter.accepts(e) {
            return None;
        }
        let entries = &self.entries;
        let pos = self
            .order
            .partition_point(|&x| cmp(&entries[x as usize], e) != Ordering::Greater);
        self.order.insert(pos, id);
        Some(pos)
    }

    /// Add one entry (file created). Returns its visible position.
    pub fn insert(&mut self, mut entry: Entry, cmp: Cmp) -> Option<usize> {
        if self.find(&entry.name).is_some() {
            return self.update(entry, cmp);
        }
        entry.alive = true;
        let id = self.entries.len() as u32;
        self.entries.push(entry);
        self.insert_into_order(id, cmp)
    }

    /// Replace the metadata of an existing entry (file modified) and reposition it.
    pub fn update(&mut self, mut entry: Entry, cmp: Cmp) -> Option<usize> {
        let Some(id) = self.find(&entry.name) else {
            return self.insert(entry, cmp);
        };
        if let Some(p) = self.position_of(id) {
            self.order.remove(p);
        }
        entry.alive = true;
        entry.icon = if entry.attrs == self.entries[id as usize].attrs {
            self.entries[id as usize].icon
        } else {
            -1
        };
        self.entries[id as usize] = entry;
        let pos = self.insert_into_order(id, cmp);
        if pos.is_none() {
            self.selection.remove(&id);
        }
        pos
    }

    /// Remove an entry by name (file deleted). Returns true if it existed.
    pub fn remove(&mut self, name: &[u16]) -> bool {
        let Some(id) = self.find(name) else {
            return false;
        };
        self.entries[id as usize].alive = false;
        if let Some(p) = self.position_of(id) {
            self.order.remove(p);
        }
        self.selection.remove(&id);
        self.dead += 1;
        if self.dead > 1024 && self.dead * 2 > self.entries.len() {
            self.compact();
        }
        true
    }

    /// Rename keeps selection of the renamed entry.
    pub fn rename(&mut self, old: &[u16], new: &[u16], cmp: Cmp) -> Option<usize> {
        let id = self.find(old)?;
        if let Some(p) = self.position_of(id) {
            self.order.remove(p);
        }
        self.entries[id as usize].name = new.into();
        self.entries[id as usize].icon = -1;
        let pos = self.insert_into_order(id, cmp);
        if pos.is_none() {
            self.selection.remove(&id);
        }
        pos
    }

    fn compact(&mut self) {
        let mut remap = vec![u32::MAX; self.entries.len()];
        let mut kept = Vec::with_capacity(self.entries.len() - self.dead);
        for (i, e) in std::mem::take(&mut self.entries).into_iter().enumerate() {
            if e.alive {
                remap[i] = kept.len() as u32;
                kept.push(e);
            }
        }
        self.entries = kept;
        for id in &mut self.order {
            *id = remap[*id as usize];
        }
        self.selection = self.selection.iter().map(|&id| remap[id as usize]).collect();
        self.dead = 0;
    }

    // ---- selection ----

    pub fn set_selected(&mut self, pos: usize, selected: bool) {
        if let Some(id) = self.id_at(pos) {
            if selected {
                self.selection.insert(id);
            } else {
                self.selection.remove(&id);
            }
        }
    }

    pub fn select_all(&mut self) {
        self.selection = self.order.iter().copied().collect();
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    pub fn select_only_name(&mut self, name: &[u16]) -> Option<usize> {
        self.selection.clear();
        let id = self.find(name)?;
        let pos = self.position_of(id)?;
        self.selection.insert(id);
        Some(pos)
    }

    pub fn selected_names(&self) -> HashSet<Box<[u16]>> {
        self.selection
            .iter()
            .map(|&id| self.entries[id as usize].name.clone())
            .collect()
    }

    /// Select every visible entry whose name is in `names` (used after a refresh).
    pub fn select_names(&mut self, names: &HashSet<Box<[u16]>>) {
        self.selection.clear();
        if names.is_empty() {
            return;
        }
        for &id in &self.order {
            if names.contains(&self.entries[id as usize].name) {
                self.selection.insert(id);
            }
        }
    }

    pub fn selection_count(&self) -> usize {
        self.selection.len()
    }

    pub fn is_selected(&self, pos: usize) -> bool {
        self.id_at(pos).is_some_and(|id| self.selection.contains(&id))
    }

    /// Visible positions of selected entries, ascending.
    pub fn selected_positions(&self) -> Vec<usize> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        self.order
            .iter()
            .enumerate()
            .filter(|(_, id)| self.selection.contains(id))
            .map(|(p, _)| p)
            .collect()
    }

    pub fn selected_entries(&self) -> Vec<&Entry> {
        self.selected_positions()
            .into_iter()
            .map(|p| self.get(p).unwrap())
            .collect()
    }

    pub fn selected_size(&self) -> u64 {
        self.selection
            .iter()
            .map(|&id| &self.entries[id as usize])
            .filter(|e| !e.is_dir())
            .map(|e| e.size)
            .sum()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Simple natural order used by tests in place of StrCmpLogicalW.
    pub fn natural(a: &[u16], b: &[u16]) -> Ordering {
        let a = String::from_utf16_lossy(a).to_lowercase();
        let b = String::from_utf16_lossy(b).to_lowercase();
        let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
        loop {
            match (x.peek().copied(), y.peek().copied()) {
                (None, None) => return Ordering::Equal,
                (None, _) => return Ordering::Less,
                (_, None) => return Ordering::Greater,
                (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                    let mut na = String::new();
                    while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                        na.push(c);
                        x.next();
                    }
                    let mut nb = String::new();
                    while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                        nb.push(d);
                        y.next();
                    }
                    let (na, nb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                    match na.len().cmp(&nb.len()).then_with(|| na.cmp(nb)) {
                        Ordering::Equal => {}
                        o => return o,
                    }
                }
                (Some(c), Some(d)) => {
                    if c != d {
                        return c.cmp(&d);
                    }
                    x.next();
                    y.next();
                }
            }
        }
    }

    pub fn by_name(a: &Entry, b: &Entry) -> Ordering {
        b.is_dir().cmp(&a.is_dir()).then_with(|| natural(&a.name, &b.name))
    }

    fn names(l: &Listing) -> Vec<String> {
        l.iter().map(Entry::name_string).collect()
    }

    fn f(name: &str) -> Entry {
        Entry::new(name, 0)
    }

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural(&w("file2"), &w("file10")), Ordering::Less);
        assert_eq!(natural(&w("File1"), &w("file1")), Ordering::Equal);
    }

    #[test]
    fn chunks_merge_sorted_with_dirs_first() {
        let mut l = Listing::new(1, Filter::default());
        l.append_chunk(vec![f("b"), f("file10"), Entry::new("zdir", ATTR_DIRECTORY)], &by_name);
        l.append_chunk(vec![f("a"), f("file2")], &by_name);
        assert_eq!(names(&l), ["zdir", "a", "b", "file2", "file10"]);
    }

    #[test]
    fn hidden_filter() {
        let mut l = Listing::new(1, Filter::default());
        l.append_chunk(vec![f("a"), Entry::new("h", ATTR_HIDDEN)], &by_name);
        assert_eq!(names(&l), ["a"]);
        l.set_filter(
            Filter {
                show_hidden: true,
                ..Default::default()
            },
            &by_name,
        );
        assert_eq!(names(&l), ["a", "h"]);
    }

    #[test]
    fn dotfile_filter_is_independent_of_hidden() {
        let all = |show_hidden, show_dotfiles| {
            let mut l = Listing::new(
                1,
                Filter {
                    show_hidden,
                    show_dotfiles,
                },
            );
            l.append_chunk(
                vec![
                    f("a"),
                    f(".gitignore"),
                    Entry::new(".ssh", ATTR_DIRECTORY),
                    Entry::new("h", ATTR_HIDDEN),
                    Entry::new(".hd", ATTR_HIDDEN),
                ],
                &by_name,
            );
            names(&l)
        };
        assert_eq!(all(false, false), ["a"]);
        assert_eq!(all(true, false), ["a", "h"]);
        assert_eq!(all(false, true), [".ssh", ".gitignore", "a"]);
        assert_eq!(all(true, true), [".ssh", ".gitignore", ".hd", "a", "h"]);
        // Toggling on an existing listing re-filters without re-enumerating.
        let mut l = Listing::new(1, Filter::default());
        l.append_chunk(vec![f("a"), f(".env")], &by_name);
        l.set_filter(
            Filter {
                show_dotfiles: true,
                ..Default::default()
            },
            &by_name,
        );
        assert_eq!(names(&l), [".env", "a"]);
    }

    #[test]
    fn insert_remove_rename() {
        let mut l = Listing::new(1, Filter::default());
        l.append_chunk(vec![f("a"), f("c")], &by_name);
        assert_eq!(l.insert(f("b"), &by_name), Some(1));
        assert!(l.remove(&w("a")));
        assert!(!l.remove(&w("zzz")));
        assert_eq!(names(&l), ["b", "c"]);
        assert_eq!(l.rename(&w("b"), &w("d"), &by_name), Some(1));
        assert_eq!(names(&l), ["c", "d"]);
        // Inserting an existing name updates it instead of duplicating.
        let mut big = f("c");
        big.size = 10;
        l.insert(big, &by_name);
        assert_eq!(names(&l), ["c", "d"]);
        assert_eq!(l.get(0).unwrap().size, 10);
    }

    #[test]
    fn selection_survives_resort_and_compaction() {
        let mut l = Listing::new(1, Filter::default());
        let chunk: Vec<Entry> = (0..3000).map(|i| f(&format!("f{i}"))).collect();
        l.append_chunk(chunk, &by_name);
        let keep = w("f2999");
        let pos = l.select_only_name(&keep).unwrap();
        assert_eq!(l.get(pos).unwrap().name_string(), "f2999");
        for i in 0..2000 {
            l.remove(&w(&format!("f{i}")));
        }
        // Compaction ran (dead > 1024 and over half): ids were remapped.
        assert_eq!(l.len(), 1000);
        assert_eq!(l.selection_count(), 1);
        let p = l.selected_positions()[0];
        assert_eq!(l.get(p).unwrap().name_string(), "f2999");
        let rev = |a: &Entry, b: &Entry| by_name(b, a);
        l.resort(&rev);
        let p = l.selected_positions()[0];
        assert_eq!(p, 0);
        assert_eq!(l.get(p).unwrap().name_string(), "f2999");
    }

    #[test]
    fn selection_by_names_across_listings() {
        let mut a = Listing::new(1, Filter::default());
        a.append_chunk(vec![f("x"), f("y"), f("z")], &by_name);
        a.set_selected(0, true);
        a.set_selected(2, true);
        let names = a.selected_names();
        let mut b = Listing::new(2, Filter::default());
        b.append_chunk(vec![f("w"), f("x"), f("z")], &by_name);
        b.select_names(&names);
        assert_eq!(b.selected_positions(), [1, 2]);
    }

    #[test]
    fn hiding_drops_selection() {
        let mut l = Listing::new(
            1,
            Filter {
                show_hidden: true,
                ..Default::default()
            },
        );
        l.append_chunk(vec![f("a"), Entry::new("h", ATTR_HIDDEN)], &by_name);
        l.select_all();
        l.set_filter(
            Filter {
                show_hidden: false,
                ..Default::default()
            },
            &by_name,
        );
        assert_eq!(l.selection_count(), 1);
    }

    #[test]
    fn extension() {
        assert_eq!(String::from_utf16_lossy(f("a.tar.gz").extension()), "gz");
        assert!(f(".gitignore").extension().is_empty());
        assert!(f("noext").extension().is_empty());
        assert!(Entry::new("d.x", ATTR_DIRECTORY).extension().is_empty());
    }

    /// Deterministic LCG so the property test needs no external crate.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    #[test]
    fn chunked_merge_equals_full_sort() {
        let mut rng = Lcg(42);
        for _case in 0..1000 {
            let n = (rng.next() % 200) as usize;
            let all: Vec<Entry> = (0..n)
                .map(|i| {
                    let attrs = match rng.next() % 5 {
                        0 => ATTR_DIRECTORY,
                        1 => ATTR_HIDDEN,
                        _ => 0,
                    };
                    let mut e = Entry::new(&format!("n{}_{i}", rng.next() % 50), attrs);
                    e.size = rng.next() % 10;
                    e
                })
                .collect();
            let by_size = |a: &Entry, b: &Entry| a.size.cmp(&b.size).then_with(|| by_name(a, b));
            let mut chunked = Listing::new(1, Filter::default());
            let mut rest = all.clone();
            while !rest.is_empty() {
                let k = ((rng.next() % 30) as usize + 1).min(rest.len());
                let chunk: Vec<Entry> = rest.drain(..k).collect();
                chunked.append_chunk(chunk, &by_size);
            }
            let mut full = Listing::new(1, Filter::default());
            full.append_chunk(all, &by_size);
            assert_eq!(names(&chunked), names(&full));
        }
    }
}
