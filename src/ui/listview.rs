//! Virtual (LVS_OWNERDATA) file list control helpers.

use windows::Win32::Foundation::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::model::layout::scale;
use crate::model::listing::Listing;
use crate::model::workspace::*;
use crate::shell::icons::ImageLists;

pub fn create(parent: HWND, id: i32) -> Result<HWND> {
    unsafe {
        let style = WS_CHILD
            | WS_VISIBLE
            | WS_CLIPSIBLINGS
            | WS_TABSTOP
            | WINDOW_STYLE(LVS_REPORT | LVS_OWNERDATA | LVS_SHOWSELALWAYS | LVS_EDITLABELS | LVS_SHAREIMAGELISTS);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WC_LISTVIEWW,
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
        let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_HEADERDRAGDROP | LVS_EX_LABELTIP;
        SendMessageW(
            hwnd,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(ex as usize)),
            Some(LPARAM(ex as isize)),
        );
        let _ = SetWindowTheme(hwnd, w!("Explorer"), PCWSTR::null());
        Ok(hwnd)
    }
}

pub fn column_title(id: ColumnId) -> &'static str {
    match id {
        ColumnId::Name => "名前",
        ColumnId::Modified => "更新日時",
        ColumnId::Created => "作成日時",
        ColumnId::Type => "種類",
        ColumnId::Size => "サイズ",
        ColumnId::Attributes => "属性",
        ColumnId::Extension => "拡張子",
    }
}

fn send(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wp)), Some(LPARAM(lp))) }
}

pub fn header(hwnd: HWND) -> HWND {
    HWND(send(hwnd, LVM_GETHEADER, 0, 0).0 as *mut _)
}

/// Rebuild all columns from `view` (widths in 96-DPI units).
pub fn set_columns(hwnd: HWND, view: &ViewState, dpi: u32) {
    unsafe {
        let _ = SendMessageW(hwnd, WM_SETREDRAW, Some(WPARAM(0)), None);
        while send(hwnd, LVM_DELETECOLUMN, 0, 0).0 != 0 {}
        for (i, c) in view.columns.iter().enumerate() {
            let mut title = super::wz(column_title(c.id));
            let fmt = if c.id == ColumnId::Size {
                LVCFMT_RIGHT
            } else {
                LVCFMT_LEFT
            };
            let col = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
                fmt,
                cx: scale(c.width as i32, dpi),
                pszText: PWSTR(title.as_mut_ptr()),
                ..Default::default()
            };
            send(hwnd, LVM_INSERTCOLUMNW, i, &col as *const _ as isize);
        }
        set_sort_arrow(hwnd, view);
        let _ = SendMessageW(hwnd, WM_SETREDRAW, Some(WPARAM(1)), None);
    }
}

pub fn set_sort_arrow(hwnd: HWND, view: &ViewState) {
    let hdr = header(hwnd);
    for (i, c) in view.columns.iter().enumerate() {
        let mut item = HDITEMW {
            mask: HDI_FORMAT,
            ..Default::default()
        };
        send(hdr, HDM_GETITEMW, i, &mut item as *mut _ as isize);
        item.fmt = HEADER_CONTROL_FORMAT_FLAGS(item.fmt.0 & !(HDF_SORTUP.0 | HDF_SORTDOWN.0));
        if c.id == view.sort.column {
            item.fmt = HEADER_CONTROL_FORMAT_FLAGS(
                item.fmt.0
                    | if view.sort.order == SortOrder::Asc {
                        HDF_SORTUP.0
                    } else {
                        HDF_SORTDOWN.0
                    },
            );
        }
        send(hdr, HDM_SETITEMW, i, &item as *const _ as isize);
    }
}

/// Read back widths and drag-reordered column order into `view`.
pub fn read_columns(hwnd: HWND, view: &mut ViewState, dpi: u32) {
    let n = view.columns.len();
    if n == 0 || send(header(hwnd), HDM_GETITEMCOUNT, 0, 0).0 as usize != n {
        return;
    }
    let mut order = vec![0i32; n];
    send(hwnd, LVM_GETCOLUMNORDERARRAY, n, order.as_mut_ptr() as isize);
    let mut cols = Vec::with_capacity(n);
    for &idx in &order {
        let Some(c) = view.columns.get(idx as usize) else {
            return;
        };
        let w = send(hwnd, LVM_GETCOLUMNWIDTH, idx as usize, 0).0 as i32;
        let w96 = ((w as i64 * 96) / dpi.max(1) as i64).max(20) as u32;
        cols.push(ColumnSpec { id: c.id, width: w96 });
    }
    view.columns = cols;
}

pub fn set_view_mode(hwnd: HWND, mode: ViewMode, lists: &ImageLists) {
    let (view, normal) = match mode {
        ViewMode::Details => (LV_VIEW_DETAILS, lists.large),
        ViewMode::List => (LV_VIEW_LIST, lists.large),
        ViewMode::IconsSmall => (LV_VIEW_SMALLICON, lists.large),
        ViewMode::IconsMedium => (LV_VIEW_ICON, lists.extra_large()),
        ViewMode::IconsLarge => (LV_VIEW_ICON, lists.jumbo()),
    };
    send(hwnd, LVM_SETIMAGELIST, LVSIL_SMALL as usize, lists.small.0);
    send(hwnd, LVM_SETIMAGELIST, LVSIL_NORMAL as usize, normal.0);
    send(hwnd, LVM_SETVIEW, view as usize, 0);
}

pub fn set_count(hwnd: HWND, n: usize) {
    send(
        hwnd,
        LVM_SETITEMCOUNT,
        n,
        (LVSICF_NOSCROLL | LVSICF_NOINVALIDATEALL) as isize,
    );
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
    }
}

fn set_state(hwnd: HWND, index: i32, state: LIST_VIEW_ITEM_STATE_FLAGS, mask: LIST_VIEW_ITEM_STATE_FLAGS) {
    let item = LVITEMW {
        stateMask: mask,
        state,
        ..Default::default()
    };
    send(hwnd, LVM_SETITEMSTATE, index as usize, &item as *const _ as isize);
}

/// Push the model selection into the control.
pub fn apply_selection(hwnd: HWND, listing: &Listing) {
    set_state(hwnd, -1, LIST_VIEW_ITEM_STATE_FLAGS(0), LVIS_SELECTED);
    for p in listing.selected_positions() {
        set_state(hwnd, p as i32, LVIS_SELECTED, LVIS_SELECTED);
    }
}

/// Select exactly the rows in `range`.
pub fn select_only(hwnd: HWND, range: std::ops::RangeInclusive<usize>) {
    set_state(hwnd, -1, LIST_VIEW_ITEM_STATE_FLAGS(0), LVIS_SELECTED);
    for p in range {
        set_state(hwnd, p as i32, LVIS_SELECTED, LVIS_SELECTED);
    }
}

pub fn focused(hwnd: HWND) -> Option<usize> {
    let r = send(hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_FOCUSED as isize).0;
    (r >= 0).then_some(r as usize)
}

pub fn set_focused(hwnd: HWND, pos: usize) {
    set_state(hwnd, pos as i32, LVIS_FOCUSED, LVIS_FOCUSED);
    send(hwnd, LVM_ENSUREVISIBLE, pos, 0);
}

/// Anchor row used for Shift+click / Shift+arrow range selection.
pub fn selection_mark(hwnd: HWND) -> Option<usize> {
    let r = send(hwnd, LVM_GETSELECTIONMARK, 0, 0).0;
    (r >= 0).then_some(r as usize)
}

pub fn set_selection_mark(hwnd: HWND, pos: usize) {
    send(hwnd, LVM_SETSELECTIONMARK, 0, pos as isize);
}

pub fn top_index(hwnd: HWND) -> usize {
    send(hwnd, LVM_GETTOPINDEX, 0, 0).0.max(0) as usize
}

/// Scroll so that `top` becomes (roughly) the first visible row.
pub fn scroll_to_top_index(hwnd: HWND, top: usize, count: usize) {
    if count == 0 {
        return;
    }
    let per_page = send(hwnd, LVM_GETCOUNTPERPAGE, 0, 0).0.max(1) as usize;
    send(hwnd, LVM_ENSUREVISIBLE, (count - 1).min(top + per_page - 1), 0);
    send(hwnd, LVM_ENSUREVISIBLE, top.min(count - 1), 0);
}

pub fn edit_label(hwnd: HWND, pos: usize) -> HWND {
    HWND(send(hwnd, LVM_EDITLABELW, pos, 0).0 as *mut _)
}

pub fn redraw_item(hwnd: HWND, pos: usize) {
    send(hwnd, LVM_REDRAWITEMS, pos, pos as isize);
}
