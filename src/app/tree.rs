//! Navigation tree notifications.

use windows::Win32::Foundation::*;
use windows::Win32::UI::Controls::*;

use super::*;
use crate::model::command::Command;

impl App {
    pub fn load_nav_children(&mut self, node: usize) {
        let Some(loc) = self.nav.begin_load(node) else { return };
        let generation = self.nav.generation;
        let n = self.notifier.clone();
        self.enum_pool.spawn(move || {
            let children = crate::ui::navtree::load_children(&loc);
            n.send(WorkerMsg::NavChildren {
                node,
                generation,
                children,
            });
        });
    }

    /// Fill the tree from the roots and expand "This PC".
    pub fn build_nav_tree(&mut self) {
        self.nav.populate_roots();
        if let Some(node) = self.nav.expand_this_pc() {
            self.load_nav_children(node);
        }
    }

    pub fn on_tree_notify(&mut self, hdr: &NMHDR, lp: LPARAM) -> LRESULT {
        unsafe {
            match hdr.code {
                TVN_ITEMEXPANDINGW => {
                    let nm = &*(lp.0 as *const NMTREEVIEWW);
                    if nm.action == TVE_EXPAND
                        && let Some(node) = self.nav.node_of_item(nm.itemNew.lParam.0)
                    {
                        self.load_nav_children(node);
                    }
                }
                TVN_SELCHANGEDW => {
                    let nm = &*(lp.0 as *const NMTREEVIEWW);
                    if (nm.action.0 == TVC_BYMOUSE.0 || nm.action.0 == TVC_BYKEYBOARD.0)
                        && let Some(loc) = self
                            .nav
                            .node_of_item(nm.itemNew.lParam.0)
                            .and_then(|n| self.nav.node_location(n))
                            .cloned()
                        && let Some(loc) = crate::shell::names::resolve(loc.as_str()).or(Some(loc))
                    {
                        self.execute(Command::Navigate(loc));
                    }
                }
                _ => {}
            }
        }
        LRESULT(0)
    }
}
