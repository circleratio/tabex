//! Main menu bar and popup menus.

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use super::*;
use crate::model::command::Command;

const STATIC_BASE: i32 = 1000;
const RECENT_BASE: i32 = 3000;
const POPUP_BASE: i32 = 5000;

#[derive(Default)]
pub struct MenuState {
    cmds: Vec<Command>,
    pub view_menu: HMENU,
    pub workspace_menu: HMENU,
    recent: Vec<String>,
    /// Commands of the popup menu currently being tracked.
    popup: Vec<Command>,
}

fn append(menu: HMENU, id: i32, text: &str) {
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, id as usize, &HSTRING::from(text));
    }
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}

impl MenuState {
    fn item(&mut self, menu: HMENU, text: &str, cmd: Command) {
        let id = STATIC_BASE + self.cmds.len() as i32;
        self.cmds.push(cmd);
        append(menu, id, text);
    }

    pub fn build(&mut self) -> HMENU {
        use Command::*;
        unsafe {
            let bar = CreateMenu().unwrap();
            let file = CreatePopupMenu().unwrap();
            self.item(file, "新しいタブ(&T)\tCtrl+T", NewTab);
            self.item(file, "タブを複製(&D)", DuplicateTab);
            self.item(file, "タブを閉じる(&C)\tCtrl+W", CloseTab);
            self.item(file, "閉じたタブを開き直す(&R)\tCtrl+Shift+T", ReopenClosedTab);
            separator(file);
            self.item(file, "新しいフォルダー(&N)\tCtrl+Shift+N", NewFolder);
            self.item(file, "プロパティ(&P)\tAlt+Enter", Properties);
            separator(file);
            self.item(file, "終了(&X)", Exit);

            let edit = CreatePopupMenu().unwrap();
            self.item(edit, "元に戻す(&U)\tCtrl+Z", Undo);
            separator(edit);
            self.item(edit, "切り取り(&T)\tCtrl+X", Cut);
            self.item(edit, "コピー(&C)\tCtrl+C", Copy);
            self.item(edit, "貼り付け(&P)\tCtrl+V", Paste);
            self.item(edit, "削除(&D)\tDelete", Delete { permanent: false });
            self.item(edit, "名前の変更(&M)\tF2", Rename);
            separator(edit);
            self.item(edit, "すべて選択(&A)\tCtrl+A", SelectAll);

            let view = CreatePopupMenu().unwrap();
            self.item(view, "大アイコン(&G)\tCtrl+Shift+1", SetViewMode(ViewMode::IconsLarge));
            self.item(view, "中アイコン(&M)\tCtrl+Shift+2", SetViewMode(ViewMode::IconsMedium));
            self.item(view, "小アイコン(&N)\tCtrl+Shift+3", SetViewMode(ViewMode::IconsSmall));
            self.item(view, "一覧(&L)\tCtrl+Shift+4", SetViewMode(ViewMode::List));
            self.item(view, "詳細(&D)\tCtrl+Shift+5", SetViewMode(ViewMode::Details));
            separator(view);
            self.item(view, "隠しファイル(&H)\tCtrl+H", ToggleHidden);
            self.item(view, "ドットファイル(&O)\tCtrl+Shift+H", ToggleDotfiles);
            self.item(view, "ファイル名拡張子(&E)", ToggleExtensions);
            self.item(view, "ナビゲーション ウィンドウ(&V)", ToggleNavTree);
            self.item(view, "2 ペイン表示(&2)", ToggleLayout);
            separator(view);
            self.item(view, "最新の情報に更新(&R)\tF5", Refresh);

            let wsm = CreatePopupMenu().unwrap();

            let _ = AppendMenuW(bar, MF_POPUP, file.0 as usize, w!("ファイル(&F)"));
            let _ = AppendMenuW(bar, MF_POPUP, edit.0 as usize, w!("編集(&E)"));
            let _ = AppendMenuW(bar, MF_POPUP, view.0 as usize, w!("表示(&V)"));
            let _ = AppendMenuW(bar, MF_POPUP, wsm.0 as usize, w!("ワークスペース(&W)"));
            self.view_menu = view;
            self.workspace_menu = wsm;
            bar
        }
    }

    fn id_of(&self, cmd: &Command) -> Option<i32> {
        self.cmds.iter().position(|c| c == cmd).map(|i| STATIC_BASE + i as i32)
    }

    pub fn command_for(&self, id: i32) -> Option<Command> {
        if (POPUP_BASE..POPUP_BASE + 1000).contains(&id) {
            return self.popup.get((id - POPUP_BASE) as usize).cloned();
        }
        if id >= RECENT_BASE {
            return self
                .recent
                .get((id - RECENT_BASE) as usize)
                .map(|n| Command::WorkspaceSwitch(n.clone()));
        }
        if id < STATIC_BASE {
            return None;
        }
        self.cmds.get((id - STATIC_BASE) as usize).cloned()
    }

    /// Show a popup menu built from `items` (None = separator, bool = checked) and return
    /// the chosen command.
    pub fn track(
        &mut self,
        owner: HWND,
        items: Vec<Option<(String, Command, bool)>>,
        x: i32,
        y: i32,
    ) -> Option<Command> {
        self.track_ex(owner, items, &[], x, y)
    }

    /// Like `track`, but items whose command is in `disabled` are grayed out.
    pub fn track_ex(
        &mut self,
        owner: HWND,
        items: Vec<Option<(String, Command, bool)>>,
        disabled: &[Command],
        x: i32,
        y: i32,
    ) -> Option<Command> {
        unsafe {
            let m = CreatePopupMenu().ok()?;
            self.popup.clear();
            for it in items {
                match it {
                    None => separator(m),
                    Some((text, cmd, checked)) => {
                        let id = POPUP_BASE + self.popup.len() as i32;
                        let mut flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                        if disabled.contains(&cmd) {
                            flags |= MF_GRAYED;
                        }
                        self.popup.push(cmd);
                        let _ = AppendMenuW(m, flags, id as usize, &HSTRING::from(text));
                    }
                }
            }
            let id = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, x, y, owner, None).0;
            let _ = DestroyMenu(m);
            self.command_for(id)
        }
    }
}

impl App {
    pub fn on_init_menu_popup(&mut self, menu: HMENU) {
        if menu == self.menu.view_menu {
            let mode = self.ws.active_tab().view.mode;
            let checks = [
                (Command::SetViewMode(ViewMode::IconsLarge), mode == ViewMode::IconsLarge),
                (
                    Command::SetViewMode(ViewMode::IconsMedium),
                    mode == ViewMode::IconsMedium,
                ),
                (Command::SetViewMode(ViewMode::IconsSmall), mode == ViewMode::IconsSmall),
                (Command::SetViewMode(ViewMode::List), mode == ViewMode::List),
                (Command::SetViewMode(ViewMode::Details), mode == ViewMode::Details),
                (Command::ToggleHidden, self.config.show_hidden),
                (Command::ToggleDotfiles, self.config.show_dotfiles),
                (Command::ToggleExtensions, self.config.show_extensions),
                (Command::ToggleNavTree, self.config.nav_tree_visible),
                (Command::ToggleLayout, self.ws.layout == Layout::Dual),
            ];
            for (cmd, on) in checks {
                if let Some(id) = self.menu.id_of(&cmd) {
                    unsafe {
                        CheckMenuItem(menu, id as u32, (if on { MF_CHECKED } else { MF_UNCHECKED }).0);
                    }
                }
            }
        } else if menu == self.menu.workspace_menu {
            unsafe {
                while GetMenuItemCount(Some(menu)) > 0 {
                    let _ = DeleteMenu(menu, 0, MF_BYPOSITION);
                }
            }
            // Rebuild the workspace menu each time so the recent list is current.
            let items: [(&str, Command); 5] = [
                ("ワークスペース一覧(&L)...\tCtrl+Alt+W", Command::WorkspaceList),
                ("新規作成(&N)", Command::WorkspaceNew { from_current: false }),
                ("名前を付けて保存(&A)...", Command::WorkspaceSaveAs),
                ("名前の変更(&R)...", Command::WorkspaceRename),
                ("削除(&D)", Command::WorkspaceDelete),
            ];
            for (text, cmd) in items {
                let id = match self.menu.id_of(&cmd) {
                    Some(id) => id,
                    None => {
                        self.menu.cmds.push(cmd);
                        STATIC_BASE + self.menu.cmds.len() as i32 - 1
                    }
                };
                append(menu, id, text);
            }
            separator(menu);
            let names = paths::list_workspaces(&self.base);
            let mut recent: Vec<String> = self
                .config
                .recent_workspaces
                .iter()
                .filter(|n| names.iter().any(|m| m.eq_ignore_ascii_case(n)))
                .cloned()
                .collect();
            for n in names {
                if !recent.iter().any(|r| r.eq_ignore_ascii_case(&n)) && recent.len() < store::config::MAX_RECENT {
                    recent.push(n);
                }
            }
            for (i, n) in recent.iter().enumerate() {
                let id = RECENT_BASE + i as i32;
                let label = if i < 9 { format!("&{} {}", i + 1, n) } else { n.clone() };
                append(menu, id, &label);
                if n.eq_ignore_ascii_case(&self.ws.name) {
                    unsafe {
                        CheckMenuItem(menu, id as u32, MF_CHECKED.0);
                    }
                }
            }
            self.menu.recent = recent;
        }
    }

    pub fn column_menu(&mut self, side: PaneSide) {
        let tab = self.ws.pane(side).active();
        let items = ColumnId::ALL
            .iter()
            .filter(|c| **c != ColumnId::Name)
            .map(|c| {
                let on = tab.view.columns.iter().any(|x| x.id == *c);
                Some((
                    ui::listview::column_title(*c).to_string(),
                    Command::ToggleColumn(*c),
                    on,
                ))
            })
            .collect();
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        if let Some(cmd) = self.menu.track(self.hwnd, items, pt.x, pt.y) {
            self.execute(cmd);
        }
    }
}
