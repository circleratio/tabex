//! Workspace persistence and management commands.

use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::*;
use crate::store::{config, paths, workspace_file};
use crate::ui::workspace_dialog::prompt;

impl App {
    /// Write the workspace (and config) if there are unsaved changes.
    pub fn save_now(&mut self) {
        if self.autosave.is_dirty() {
            for side in PaneSide::BOTH {
                self.capture_view(side);
            }
            let path = paths::workspace_path(&self.base, &self.ws.name);
            match workspace_file::save(&path, &self.ws) {
                Ok(()) => {
                    self.autosave.clear();
                }
                Err(e) => {
                    // Keep the dirty flag; the next change or timer retries.
                    self.show_status(&format!("ワークスペースを保存できませんでした: {e}"));
                }
            }
        }
        if self.config_dirty {
            self.save_config();
        }
    }

    pub fn save_config(&mut self) {
        self.config.touch_recent(&self.ws.name.clone());
        match config::save(&paths::config_path(&self.base), &self.config) {
            Ok(()) => self.config_dirty = false,
            Err(e) => self.show_status(&format!("設定を保存できませんでした: {e}")),
        }
    }

    fn existing_names(&self, except: Option<&str>) -> Vec<String> {
        paths::list_workspaces(&self.base)
            .into_iter()
            .filter(|n| except.is_none_or(|e| !n.eq_ignore_ascii_case(e)))
            .collect()
    }

    fn ask_name(&self, title: &str, initial: &str, except: Option<&str>) -> Option<String> {
        let existing = self.existing_names(except);
        prompt(
            self.hwnd,
            title,
            "ワークスペース名:",
            initial,
            self.font,
            move |s| paths::validate_name(s.trim(), &existing).map_err(|e| e.to_string()),
        )
        .map(|s| s.trim().to_string())
    }

    /// Replace the current workspace with `ws` (already saved).
    fn activate_workspace(&mut self, ws: Workspace) {
        for side in PaneSide::BOTH {
            self.capture_view(side);
            let p = &mut self.panes[side.index()];
            if let Some(id) = p.shown.take()
                && let Some(w) = &self.watcher
            {
                w.unwatch(id);
            }
            ui::listview::set_count(p.list, 0);
        }
        for rt in self.rt.values() {
            rt.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.rt.clear();
        self.ws = ws;
        self.autosave.clear();
        let name = self.ws.name.clone();
        self.config.touch_recent(&name);
        self.save_config();
        self.refresh_panes();
        self.layout();
        self.focus_active_list();
    }

    pub fn workspace_switch(&mut self, name: &str) {
        if name.eq_ignore_ascii_case(&self.ws.name) {
            return;
        }
        let path = paths::workspace_path(&self.base, name);
        let real_name = self
            .existing_names(None)
            .into_iter()
            .find(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(name.to_string());
        match workspace_file::load(&path, &real_name, &self.home) {
            Ok(ws) => {
                self.save_now();
                self.activate_workspace(ws);
            }
            Err(e) => self.error_box(&format!("ワークスペース \"{name}\" を開けませんでした。\n\n{e}")),
        }
    }

    pub fn workspace_new(&mut self, from_current: bool) {
        if from_current {
            return self.workspace_save_as();
        }
        let initial = paths::unique_name("新しいワークスペース", &self.existing_names(None));
        let Some(name) = self.ask_name("新しいワークスペース", &initial, None) else {
            return;
        };
        let ws = Workspace::new(name.clone(), self.home.clone(), self.home.clone());
        if let Err(e) = workspace_file::save(&paths::workspace_path(&self.base, &name), &ws) {
            return self.error_box(&format!("ワークスペースを作成できませんでした。\n\n{e}"));
        }
        self.save_now();
        self.activate_workspace(ws);
    }

    pub fn workspace_save_as(&mut self) {
        let initial = paths::unique_name(&self.ws.name, &self.existing_names(None));
        let Some(name) = self.ask_name("名前を付けて保存", &initial, None) else {
            return;
        };
        self.save_now();
        for side in PaneSide::BOTH {
            self.capture_view(side);
        }
        let mut ws = self.ws.clone();
        ws.name = name.clone();
        ws.closed.clear();
        if let Err(e) = workspace_file::save(&paths::workspace_path(&self.base, &name), &ws) {
            return self.error_box(&format!("ワークスペースを保存できませんでした。\n\n{e}"));
        }
        // Keep the live tabs (and their listings): only the name changes.
        self.ws.name = name.clone();
        self.autosave.clear();
        self.config.touch_recent(&name);
        self.save_config();
        self.update_title();
        self.show_status(&format!("ワークスペース \"{name}\" を保存しました"));
    }

    pub fn workspace_rename(&mut self, old: &str) {
        let Some(new) = self.ask_name("ワークスペース名の変更", old, Some(old)) else {
            return;
        };
        if new == old {
            return;
        }
        let from = paths::workspace_path(&self.base, old);
        let to = paths::workspace_path(&self.base, &new);
        if old.eq_ignore_ascii_case(&self.ws.name) {
            self.save_now();
        }
        if let Err(e) = std::fs::rename(&from, &to) {
            return self.error_box(&format!("名前を変更できませんでした。\n\n{e}"));
        }
        if old.eq_ignore_ascii_case(&self.ws.name) {
            self.ws.name = new.clone();
            // Rewrite so the name key inside the file matches.
            self.mark_dirty();
            self.save_now();
        }
        self.config.rename(old, &new);
        self.save_config();
        self.update_title();
    }

    pub fn workspace_delete(&mut self, name: &str) {
        let r = unsafe {
            MessageBoxW(
                Some(self.hwnd),
                &HSTRING::from(format!(
                    "ワークスペース \"{name}\" を削除しますか？\nこの操作は元に戻せません。"
                )),
                w!("tabex"),
                MB_ICONWARNING | MB_YESNO | MB_DEFBUTTON2,
            )
        };
        if r != IDYES {
            return;
        }
        if let Err(e) = std::fs::remove_file(paths::workspace_path(&self.base, name)) {
            return self.error_box(&format!("削除できませんでした。\n\n{e}"));
        }
        self.config.forget(name);
        if name.eq_ignore_ascii_case(&self.ws.name) {
            self.autosave.clear();
            let names = self.existing_names(None);
            let next = self
                .config
                .recent_workspaces
                .iter()
                .find(|n| names.iter().any(|m| m.eq_ignore_ascii_case(n)))
                .cloned()
                .or_else(|| names.first().cloned());
            let downloads = crate::shell::names::downloads();
            let opened = store::open_or_create(&self.base, next.as_deref(), (&self.home, &downloads));
            self.activate_workspace(opened.workspace);
        } else {
            self.save_config();
        }
    }
}
