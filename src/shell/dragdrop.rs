//! OLE drop target for the file lists, tab bars and navigation tree.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

use super::clipboard::{DROPEFFECT_COPY_, DROPEFFECT_MOVE_, hdrop_paths};

/// What the drop target asks the application.
pub trait DropHost {
    /// Destination folder under the cursor (screen coordinates), or None if not droppable.
    fn drop_folder(&self, target: HWND, x: i32, y: i32) -> Option<String>;
    /// Called while hovering, e.g. to switch tabs after a delay.
    fn hover(&self, target: HWND, x: i32, y: i32, since: Instant);
    /// Perform the drop.
    fn drop_items(&self, data: &IDataObject, dest: &str, effect: u32);
}

#[implement(IDropTarget)]
pub struct DropTarget {
    hwnd: HWND,
    host: Box<dyn DropHost>,
    helper: Option<IDropTargetHelper>,
    data: RefCell<Option<IDataObject>>,
    source_root: RefCell<Option<String>>,
    last_pos: Cell<(i32, i32)>,
    since: Cell<Option<Instant>>,
}

fn root_of(path: &str) -> String {
    let p = path.to_ascii_lowercase();
    if let Some(unc) = p.strip_prefix("\\\\") {
        let mut it = unc.split('\\');
        format!("\\\\{}\\{}", it.next().unwrap_or(""), it.next().unwrap_or(""))
    } else {
        p.chars().take(2).collect()
    }
}

/// Explorer rules: Ctrl copies, Shift moves, otherwise move within a volume and copy across.
pub fn choose_effect(keys: MODIFIERKEYS_FLAGS, source_root: Option<&str>, dest: &str, allowed: u32) -> u32 {
    let same_volume = source_root.is_some_and(|r| r == root_of(dest));
    let want = if keys.0 & MK_CONTROL.0 == 0 && (keys.0 & MK_SHIFT.0 != 0 || same_volume) {
        DROPEFFECT_MOVE_
    } else {
        DROPEFFECT_COPY_
    };
    if allowed & want != 0 {
        want
    } else if allowed & DROPEFFECT_COPY_ != 0 {
        DROPEFFECT_COPY_
    } else {
        allowed & DROPEFFECT_MOVE_
    }
}

impl DropTarget {
    pub fn register(hwnd: HWND, host: Box<dyn DropHost>) -> Result<IDropTarget> {
        let helper: Option<IDropTargetHelper> =
            unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok() };
        let t: IDropTarget = DropTarget {
            hwnd,
            host,
            helper,
            data: RefCell::new(None),
            source_root: RefCell::new(None),
            last_pos: Cell::new((i32::MIN, i32::MIN)),
            since: Cell::new(None),
        }
        .into();
        unsafe { RegisterDragDrop(hwnd, &t)? };
        Ok(t)
    }

    fn effect_at(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, allowed: u32) -> u32 {
        match self.host.drop_folder(self.hwnd, pt.x, pt.y) {
            Some(dest) => choose_effect(keys, self.source_root.borrow().as_deref(), &dest, allowed),
            None => 0,
        }
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let data = data.ok()?.clone();
        *self.source_root.borrow_mut() = hdrop_paths(&data).first().map(|p| root_of(p));
        unsafe {
            let allowed = (*effect).0;
            (*effect).0 = self.effect_at(keys, pt, allowed);
            if let Some(h) = &self.helper {
                let _ = h.DragEnter(self.hwnd, &data, &POINT { x: pt.x, y: pt.y }, *effect);
            }
        }
        *self.data.borrow_mut() = Some(data);
        self.since.set(Some(Instant::now()));
        Ok(())
    }

    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        if self.last_pos.get() != (pt.x, pt.y) {
            self.last_pos.set((pt.x, pt.y));
        }
        if let Some(since) = self.since.get() {
            self.host.hover(self.hwnd, pt.x, pt.y, since);
        }
        unsafe {
            let allowed = (*effect).0;
            (*effect).0 = self.effect_at(keys, pt, allowed);
            if let Some(h) = &self.helper {
                let _ = h.DragOver(&POINT { x: pt.x, y: pt.y }, *effect);
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        if let Some(h) = &self.helper {
            unsafe {
                let _ = h.DragLeave();
            }
        }
        *self.data.borrow_mut() = None;
        self.since.set(None);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let data = data.ok()?.clone();
        unsafe {
            let allowed = (*effect).0;
            let chosen = self.effect_at(keys, pt, allowed);
            (*effect).0 = chosen;
            if let Some(h) = &self.helper {
                let _ = h.Drop(&data, &POINT { x: pt.x, y: pt.y }, *effect);
            }
            if chosen != 0
                && let Some(dest) = self.host.drop_folder(self.hwnd, pt.x, pt.y)
            {
                self.host.drop_items(&data, &dest, chosen);
            }
        }
        *self.data.borrow_mut() = None;
        self.since.set(None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects() {
        let all = DROPEFFECT_COPY_ | DROPEFFECT_MOVE_;
        let none = MODIFIERKEYS_FLAGS(0);
        assert_eq!(choose_effect(none, Some("c:"), "C:\\x", all), DROPEFFECT_MOVE_);
        assert_eq!(choose_effect(none, Some("d:"), "C:\\x", all), DROPEFFECT_COPY_);
        assert_eq!(choose_effect(MK_CONTROL, Some("c:"), "C:\\x", all), DROPEFFECT_COPY_);
        assert_eq!(choose_effect(MK_SHIFT, Some("d:"), "C:\\x", all), DROPEFFECT_MOVE_);
        assert_eq!(choose_effect(none, None, "C:\\x", all), DROPEFFECT_COPY_);
        assert_eq!(
            choose_effect(MK_SHIFT, Some("c:"), "C:\\x", DROPEFFECT_COPY_),
            DROPEFFECT_COPY_
        );
        assert_eq!(
            choose_effect(none, Some("\\\\srv\\share"), "\\\\SRV\\Share\\dir", all),
            DROPEFFECT_MOVE_
        );
    }
}
