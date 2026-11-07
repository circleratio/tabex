//! Applying user settings from the settings dialog or from an outside edit of config.toml.

use std::time::Duration;

use windows::Win32::Graphics::Gdi::InvalidateRect;

use super::*;
use crate::fs::watch::Change;
use crate::store::config::{self, UserSettings};

/// Home directories from `[[homes]]` (falling back to `home`), plus warnings for invalid
/// entries.
pub fn resolve_homes(specs: &[config::HomeSpec], home: &Location) -> (Vec<crate::model::homes::Home>, Vec<String>) {
    crate::model::homes::resolve(
        specs.iter().map(|h| (h.name.as_deref(), h.path.as_deref())),
        crate::shell::names::expand_env,
        home,
    )
}

/// Watcher key for the settings folder; tab ids never reach it.
pub const CONFIG_WATCH_KEY: u64 = u64::MAX;
const CONFIG_RELOAD_DELAY: Duration = Duration::from_millis(300);

impl App {
    pub fn watch_config(&self) {
        if let Some(w) = &self.watcher {
            w.watch(CONFIG_WATCH_KEY, &self.base.to_string_lossy());
        }
    }

    pub fn on_config_watch(&mut self, ev: WatchEvent) {
        let is_config = |name: &[u16]| String::from_utf16_lossy(name).eq_ignore_ascii_case("config.toml");
        let touched = match ev {
            WatchEvent::Changes { changes, .. } => changes.iter().any(|c| match c {
                Change::Upsert(e) => is_config(&e.name),
                Change::Removed(name) => is_config(name),
            }),
            WatchEvent::Overflow { .. } => true,
            WatchEvent::Failed { .. } => {
                self.show_status("設定ファイルの変更を監視できません");
                false
            }
        };
        if touched {
            // Restarting the timer reads once, after the last change of a burst.
            unsafe {
                let _ = SetTimer(
                    Some(self.hwnd),
                    TIMER_CONFIG_RELOAD,
                    CONFIG_RELOAD_DELAY.as_millis() as u32,
                    None,
                );
            }
        }
    }

    /// Read config.toml after an outside edit and apply the user settings in it.
    pub fn reload_config(&mut self) {
        if self.settings_dialog_open {
            self.config_reload_pending = true;
            return;
        }
        let path = paths::config_path(&self.base);
        let Ok(text) = std::fs::read_to_string(&path) else {
            // Deleted: the next save writes it again.
            return;
        };
        if self.config_written.as_deref() == Some(text.as_str()) {
            return;
        }
        match config::from_toml(&text, &path) {
            Ok(new) => {
                self.config_written = Some(text);
                self.apply_user_settings(new.user_settings());
            }
            Err(e) => self.show_status(&format!("設定ファイルを読み込めませんでした: {e}")),
        }
    }

    pub fn redraw_lists(&self) {
        for p in &self.panes {
            unsafe {
                let _ = InvalidateRect(Some(p.list), None, false);
            }
        }
    }

    pub fn set_nav_tree_visible(&mut self, visible: bool) {
        self.config.nav_tree_visible = visible;
        self.config_dirty = true;
        self.layout();
        // Keyboard input must not go to a hidden tree.
        let focus = unsafe { GetFocus() };
        if !visible && (focus == self.nav.hwnd || unsafe { IsChild(self.nav.hwnd, focus).as_bool() }) {
            self.focus_active_list();
        }
    }

    /// Make `u` the current settings and apply every difference right away. Does not save.
    pub fn apply_user_settings(&mut self, u: UserSettings) {
        let old = self.config.user_settings();
        if old == u {
            return;
        }
        self.config.set_user_settings(u.clone());
        if old.show_hidden != u.show_hidden || old.show_dotfiles != u.show_dotfiles {
            self.apply_filter_change("設定を反映しました");
        }
        if old.show_extensions != u.show_extensions {
            self.redraw_lists();
        }
        if old.nav_tree_visible != u.nav_tree_visible {
            self.set_nav_tree_visible(u.nav_tree_visible);
        }
        let mut warnings = Vec::new();
        if old.keys != u.keys {
            let (keymap, w) = KeyMap::new(&u.keys);
            self.keymap = keymap;
            warnings.extend(w);
        }
        if old.homes != u.homes {
            let (homes, w) = resolve_homes(&u.homes, &self.home);
            self.homes = homes;
            warnings.extend(w);
        }
        if old.migemo != u.migemo {
            if !u.migemo {
                self.migemo = narrow::MigemoState::Off;
            } else if !matches!(self.migemo, narrow::MigemoState::Ready(_)) {
                // Loaded on the next "/".
                self.migemo = narrow::MigemoState::NotLoaded;
            }
        }
        if !warnings.is_empty() {
            self.show_status(&warnings.join(" / "));
        }
        self.sync_toolbar();
    }
}
