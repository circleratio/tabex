//! Navigation tree notifications.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::*;
use crate::model::command::Command;
use crate::ui::navtree::NavTarget;

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
        self.refresh_nav_workspaces();
        self.nav.expand_workspaces(self.config.nav_workspaces_expanded);
        if let Some(node) = self.nav.expand_this_pc() {
            self.load_nav_children(node);
        }
    }

    /// Re-read the workspace list into the tree and mark the open one in bold.
    pub fn refresh_nav_workspaces(&mut self) {
        let names = store::tree_workspaces(&self.base, &self.home);
        self.nav.set_workspaces(&names, &self.ws.name);
    }

    fn on_tree_rclick(&mut self) -> bool {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let screen = pt;
        unsafe {
            let _ = ScreenToClient(self.nav.hwnd, &mut pt);
        }
        let Some(target) = self.nav.target_at(pt.x, pt.y).cloned() else {
            return false;
        };
        match target {
            NavTarget::Workspace(name) => {
                let items = vec![
                    Some(("名前の変更(&M)...".to_string(), Command::WorkspaceRename, false)),
                    Some(("削除(&D)".to_string(), Command::WorkspaceDelete, false)),
                ];
                // The open workspace cannot be deleted from the tree.
                let disabled: &[Command] = if name.eq_ignore_ascii_case(&self.ws.name) {
                    &[Command::WorkspaceDelete]
                } else {
                    &[]
                };
                match self.menu.track_ex(self.hwnd, items, disabled, screen.x, screen.y) {
                    Some(Command::WorkspaceRename) => self.workspace_rename(&name),
                    Some(Command::WorkspaceDelete) => self.workspace_delete(&name),
                    _ => {}
                }
                true
            }
            NavTarget::WorkspaceRoot => {
                let items = vec![
                    Some((
                        "新しいワークスペース(&N)...".to_string(),
                        Command::WorkspaceNew { from_current: false },
                        false,
                    )),
                    Some((
                        "今の構成を元に新規作成(&C)...".to_string(),
                        Command::WorkspaceNew { from_current: true },
                        false,
                    )),
                ];
                if let Some(cmd) = self.menu.track(self.hwnd, items, screen.x, screen.y) {
                    self.execute(cmd);
                }
                true
            }
            _ => false,
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
                TVN_ITEMEXPANDEDW => {
                    let nm = &*(lp.0 as *const NMTREEVIEWW);
                    if let Some(NavTarget::WorkspaceRoot) = self
                        .nav
                        .node_of_item(nm.itemNew.lParam.0)
                        .and_then(|n| self.nav.node_target(n))
                    {
                        let expanded = nm.action == TVE_EXPAND;
                        if self.config.nav_workspaces_expanded != expanded {
                            self.config.nav_workspaces_expanded = expanded;
                            self.config_dirty = true;
                        }
                    }
                }
                TVN_SELCHANGEDW => {
                    let nm = &*(lp.0 as *const NMTREEVIEWW);
                    if nm.action.0 != TVC_BYMOUSE.0 && nm.action.0 != TVC_BYKEYBOARD.0 {
                        return LRESULT(0);
                    }
                    let Some(target) = self
                        .nav
                        .node_of_item(nm.itemNew.lParam.0)
                        .and_then(|n| self.nav.node_target(n))
                        .cloned()
                    else {
                        return LRESULT(0);
                    };
                    match target {
                        NavTarget::Folder(loc) => {
                            let loc = crate::shell::names::resolve(loc.as_str()).unwrap_or(loc);
                            self.execute(Command::Navigate(loc));
                        }
                        NavTarget::Workspace(name) => {
                            self.execute(Command::WorkspaceSwitch(name));
                            // Switching focuses the file list; stay in the tree for arrow keys.
                            let _ = SetFocus(Some(self.nav.hwnd));
                        }
                        NavTarget::WorkspaceRoot | NavTarget::Removed => {}
                    }
                }
                NM_RCLICK if self.on_tree_rclick() => return LRESULT(1),
                _ => {}
            }
        }
        LRESULT(0)
    }
}
