//! Navigation tree: quick access, drives, network and recycle bin, filled lazily.

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::fs::enumerate::{Pidl, bind_folder, display_name};
use crate::fs::{extended_path, join};
use crate::model::listing::{ATTR_DIRECTORY, ATTR_HIDDEN, Filter, is_dot_name};
use crate::model::workspace::Location;

pub const ROOTS: [&str; 4] = [
    "shell:::{679f85cb-0220-4080-b29b-5540cc05aab6}", // Quick access / Home
    "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",       // This PC
    "shell:NetworkPlacesFolder",
    "shell:RecycleBinFolder",
];

#[derive(Debug, Clone)]
pub struct NavChild {
    pub name: String,
    pub location: Location,
    pub icon: i32,
    pub hidden: bool,
    pub has_children: bool,
}

struct Node {
    item: HTREEITEM,
    location: Location,
    loaded: bool,
}

pub struct NavTree {
    pub hwnd: HWND,
    nodes: Vec<Node>,
    /// Bumped on reset so that children loaded for the old tree are ignored.
    pub generation: u64,
}

fn send(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wp)), Some(LPARAM(lp))) }
}

impl NavTree {
    pub fn create(parent: HWND, id: i32, small: HIMAGELIST) -> Result<NavTree> {
        unsafe {
            let style = WS_CHILD
                | WS_VISIBLE
                | WS_CLIPSIBLINGS
                | WS_TABSTOP
                | WINDOW_STYLE(
                    TVS_HASBUTTONS | TVS_LINESATROOT | TVS_SHOWSELALWAYS | TVS_FULLROWSELECT | TVS_TRACKSELECT,
                );
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                WC_TREEVIEWW,
                PCWSTR::null(),
                style,
                0,
                0,
                0,
                0,
                Some(parent),
                Some(HMENU(id as usize as *mut _)),
                None,
                None,
            )?;
            let _ = SetWindowTheme(hwnd, w!("Explorer"), PCWSTR::null());
            let ex = TVS_EX_DOUBLEBUFFER | TVS_EX_FADEINOUTEXPANDOS | TVS_EX_AUTOHSCROLL;
            send(hwnd, TVM_SETEXTENDEDSTYLE, ex as usize, ex as isize);
            send(hwnd, TVM_SETIMAGELIST, TVSIL_NORMAL as usize, small.0);
            Ok(NavTree {
                hwnd,
                nodes: Vec::new(),
                generation: 0,
            })
        }
    }

    fn insert(&mut self, parent: HTREEITEM, name: &str, location: Location, icon: i32, has_children: bool) -> usize {
        let mut text: Vec<u16> = super::wz(name);
        let idx = self.nodes.len();
        let ins = TVINSERTSTRUCTW {
            hParent: parent,
            hInsertAfter: TVI_LAST,
            Anonymous: TVINSERTSTRUCTW_0 {
                itemex: TVITEMEXW {
                    mask: TVIF_TEXT | TVIF_PARAM | TVIF_IMAGE | TVIF_SELECTEDIMAGE | TVIF_CHILDREN,
                    pszText: PWSTR(text.as_mut_ptr()),
                    iImage: icon,
                    iSelectedImage: icon,
                    cChildren: TVITEMEXW_CHILDREN(if has_children { 1 } else { 0 }),
                    lParam: LPARAM(idx as isize),
                    ..Default::default()
                },
            },
        };
        let item = HTREEITEM(send(self.hwnd, TVM_INSERTITEMW, 0, &ins as *const _ as isize).0);
        self.nodes.push(Node {
            item,
            location,
            loaded: false,
        });
        idx
    }

    pub fn populate_roots(&mut self) {
        if !self.nodes.is_empty() {
            return;
        }
        for r in ROOTS {
            let Ok(pidl) = Pidl::parse(r) else { continue };
            let name = shell_display_name(&pidl).unwrap_or_else(|| r.to_string());
            let icon = crate::shell::icons::pidl_icon(pidl.as_ptr());
            self.insert(HTREEITEM::default(), &name, Location::new(r), icon, true);
        }
    }

    /// Expand "This PC" so drives are visible. Returns its node for loading.
    pub fn expand_this_pc(&mut self) -> Option<usize> {
        let i = self.nodes.iter().position(|n| n.location.as_str() == ROOTS[1])?;
        Some(i)
    }

    /// Location of the item at client coordinates.
    pub fn location_at(&self, x: i32, y: i32) -> Option<Location> {
        let mut hit = TVHITTESTINFO {
            pt: POINT { x, y },
            ..Default::default()
        };
        let item = HTREEITEM(send(self.hwnd, TVM_HITTEST, 0, &mut hit as *mut _ as isize).0);
        if item.0 == 0 {
            return None;
        }
        self.nodes.iter().find(|n| n.item == item).map(|n| n.location.clone())
    }

    pub fn node_location(&self, node: usize) -> Option<&Location> {
        self.nodes.get(node).map(|n| &n.location)
    }

    pub fn node_of_item(&self, param: isize) -> Option<usize> {
        (param >= 0 && (param as usize) < self.nodes.len()).then_some(param as usize)
    }

    /// Mark a node as loading; returns its location if it needs loading.
    pub fn begin_load(&mut self, node: usize) -> Option<Location> {
        let n = self.nodes.get_mut(node)?;
        if n.loaded {
            return None;
        }
        n.loaded = true;
        Some(n.location.clone())
    }

    /// Remove every item; call `populate_roots` afterwards.
    pub fn reset(&mut self) {
        send(self.hwnd, TVM_DELETEITEM, 0, TVI_ROOT.0);
        self.nodes.clear();
        self.generation += 1;
    }

    pub fn add_children(&mut self, node: usize, generation: u64, children: Vec<NavChild>, filter: Filter) {
        if generation != self.generation {
            return;
        }
        let Some(parent) = self.nodes.get(node).map(|n| n.item) else {
            return;
        };
        let mut any = false;
        for c in children {
            let dot = is_dot_name(&crate::fs::wide(&c.name));
            if (c.hidden && !filter.show_hidden) || (dot && !filter.show_dotfiles) {
                continue;
            }
            self.insert(parent, &c.name, c.location, c.icon, c.has_children);
            any = true;
        }
        if !any {
            let item = TVITEMEXW {
                mask: TVIF_CHILDREN,
                hItem: parent,
                cChildren: TVITEMEXW_CHILDREN(0),
                ..Default::default()
            };
            send(self.hwnd, TVM_SETITEMW, 0, &item as *const _ as isize);
        } else if self.nodes[node].location.as_str() == ROOTS[1] {
            send(self.hwnd, TVM_EXPAND, TVE_EXPAND.0 as usize, parent.0);
        }
    }
}

fn shell_display_name(pidl: &Pidl) -> Option<String> {
    unsafe {
        let p = SHGetNameFromIDList(pidl.as_ptr(), SIGDN_NORMALDISPLAY).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// Subfolders of `loc`, run on a worker thread.
pub fn load_children(loc: &Location) -> Vec<NavChild> {
    if loc.is_file_system() {
        return fs_children(loc.as_str());
    }
    shell_children(loc).unwrap_or_default()
}

fn fs_children(dir: &str) -> Vec<NavChild> {
    let pattern = HSTRING::from(extended_path(&join(dir, "*")));
    let mut data = WIN32_FIND_DATAW::default();
    let mut out = Vec::new();
    let folder_icon = {
        let mut info = SHFILEINFOW::default();
        unsafe {
            SHGetFileInfoW(
                w!("folder"),
                FILE_ATTRIBUTE_DIRECTORY,
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_USEFILEATTRIBUTES | SHGFI_SYSICONINDEX | SHGFI_SMALLICON,
            )
        };
        info.iIcon
    };
    unsafe {
        let Ok(h) = FindFirstFileExW(
            &pattern,
            FindExInfoBasic,
            &mut data as *mut _ as *mut _,
            FindExSearchLimitToDirectories,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        ) else {
            return out;
        };
        loop {
            let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&data.cFileName[..len]);
            if data.dwFileAttributes & ATTR_DIRECTORY != 0 && name != "." && name != ".." {
                out.push(NavChild {
                    location: Location::new(join(dir, &name)),
                    name,
                    icon: folder_icon,
                    hidden: data.dwFileAttributes & ATTR_HIDDEN != 0,
                    has_children: true,
                });
            }
            if FindNextFileW(h, &mut data).is_err() {
                break;
            }
        }
        let _ = FindClose(h);
    }
    out.sort_by(|a, b| crate::fs::sort::compare_names(&crate::fs::wide(&a.name), &crate::fs::wide(&b.name)));
    out
}

fn shell_children(loc: &Location) -> Option<Vec<NavChild>> {
    let pidl = Pidl::parse(loc.as_str()).ok()?;
    let folder = bind_folder(&pidl).ok()?;
    let mut list: Option<IEnumIDList> = None;
    unsafe {
        folder
            .EnumObjects(HWND::default(), SHCONTF_FOLDERS.0 as u32, &mut list)
            .ok()
            .ok()?;
    }
    let list = list?;
    let mut out = Vec::new();
    loop {
        let mut child = [std::ptr::null_mut(); 1];
        let mut fetched = 0;
        if unsafe { list.Next(&mut child, Some(&mut fetched)) } != S_OK || fetched == 0 {
            break;
        }
        let child = Pidl(child[0]);
        let Some(name) = display_name(&folder, child.as_ptr(), SHGDN_INFOLDER) else {
            continue;
        };
        let mut attrs = (windows::Win32::System::SystemServices::SFGAO_HASSUBFOLDER
            | windows::Win32::System::SystemServices::SFGAO_HIDDEN
            | windows::Win32::System::SystemServices::SFGAO_STREAM)
            .0;
        unsafe {
            let _ = folder.GetAttributesOf(&[child.as_ptr()], &mut attrs);
        }
        if attrs & windows::Win32::System::SystemServices::SFGAO_STREAM.0 != 0 {
            continue; // zip files
        }
        let abs = unsafe { Pidl(ILCombine(Some(pidl.as_ptr()), Some(child.as_ptr()))) };
        let Some(location) = crate::shell::names::location_of(abs.as_ptr()) else {
            continue;
        };
        out.push(NavChild {
            name,
            location,
            icon: crate::shell::icons::pidl_icon(abs.as_ptr()),
            hidden: attrs & windows::Win32::System::SystemServices::SFGAO_HIDDEN.0 != 0,
            has_children: attrs & windows::Win32::System::SystemServices::SFGAO_HASSUBFOLDER.0 != 0,
        });
    }
    Some(out)
}
