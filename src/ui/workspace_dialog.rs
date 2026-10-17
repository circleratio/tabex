//! Workspace list popup (modeless) and a small modal name prompt.

use std::cell::RefCell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::app::App;
use crate::model::command::Command;
use crate::model::layout::scale;
use crate::store::paths;

const LIST_CLASS: PCWSTR = w!("TabexWorkspaceList");
const PROMPT_CLASS: PCWSTR = w!("TabexPrompt");
const ID_FILTER: i32 = 10;
const ID_LIST: i32 = 11;
const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;

struct ListState {
    hwnd: HWND,
    main: HWND,
    filter: HWND,
    list: HWND,
    all: Vec<String>,
    shown: Vec<String>,
}

thread_local! {
    static LIST: RefCell<Option<ListState>> = const { RefCell::new(None) };
}

pub fn owns(hwnd: HWND) -> bool {
    LIST.with(|l| {
        l.borrow()
            .as_ref()
            .is_some_and(|s| s.hwnd == hwnd || unsafe { IsChild(s.hwnd, hwnd).as_bool() })
    })
}

pub fn is_dialog_message(msg: &MSG) -> bool {
    let h = LIST.with(|l| l.borrow().as_ref().map(|s| s.hwnd));
    match h {
        Some(h) if owns(msg.hwnd) => {
            if msg.message == WM_KEYDOWN {
                let vk = VIRTUAL_KEY(msg.wParam.0 as u16);
                if vk == VK_ESCAPE {
                    close_list();
                    return true;
                }
                if vk == VK_RETURN {
                    choose_selected();
                    return true;
                }
                if (vk == VK_DOWN || vk == VK_UP)
                    && LIST.with(|l| l.borrow().as_ref().is_some_and(|s| s.filter == msg.hwnd))
                {
                    // Arrow keys in the filter box move the list selection.
                    let list = LIST.with(|l| l.borrow().as_ref().map(|s| s.list)).unwrap();
                    unsafe {
                        SendMessageW(list, msg.message, Some(msg.wParam), Some(msg.lParam));
                    }
                    return true;
                }
            }
            unsafe { IsDialogMessageW(h, msg).as_bool() }
        }
        _ => false,
    }
}

fn close_list() {
    let h = LIST.with(|l| l.borrow().as_ref().map(|s| (s.hwnd, s.main)));
    if let Some((h, main)) = h {
        unsafe {
            let _ = DestroyWindow(h);
            let _ = SetForegroundWindow(main);
        }
    }
}

fn refill() {
    LIST.with(|l| {
        let mut l = l.borrow_mut();
        let Some(s) = l.as_mut() else { return };
        let f = crate::ui::window_text(s.filter).to_lowercase();
        s.shown = s
            .all
            .iter()
            .filter(|n| n.to_lowercase().contains(&f))
            .cloned()
            .collect();
        unsafe {
            SendMessageW(s.list, LB_RESETCONTENT, None, None);
            for n in &s.shown {
                let w = super::wz(n);
                SendMessageW(s.list, LB_ADDSTRING, None, Some(LPARAM(w.as_ptr() as isize)));
            }
            SendMessageW(s.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        }
    });
}

fn selected_name() -> Option<(HWND, String)> {
    LIST.with(|l| {
        let l = l.borrow();
        let s = l.as_ref()?;
        let i = unsafe { SendMessageW(s.list, LB_GETCURSEL, None, None) }.0;
        s.shown.get(i.max(0) as usize).map(|n| (s.main, n.clone()))
    })
}

fn choose_selected() {
    if let Some((main, name)) = selected_name() {
        close_list();
        if let Some(app) = App::from_hwnd(main) {
            app.execute(Command::WorkspaceSwitch(name));
        }
    }
}

pub fn show(app: &mut App) {
    if LIST.with(|l| l.borrow().is_some()) {
        close_list();
    }
    let names = paths::list_workspaces(&app.base);
    let mut all: Vec<String> = app
        .config
        .recent_workspaces
        .iter()
        .filter(|n| names.iter().any(|m| m.eq_ignore_ascii_case(n)))
        .cloned()
        .collect();
    for n in names {
        if !all.iter().any(|a| a.eq_ignore_ascii_case(&n)) {
            all.push(n);
        }
    }
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSW {
            hInstance: instance,
            lpszClassName: LIST_CLASS,
            lpfnWndProc: Some(list_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let s = |v| scale(v, app.dpi);
        let mut rc = RECT::default();
        let _ = GetWindowRect(app.hwnd, &mut rc);
        let (w, h) = (s(360), s(320));
        let x = (rc.left + rc.right - w) / 2;
        let y = rc.top + s(80);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_DLGMODALFRAME,
            LIST_CLASS,
            w!("ワークスペース"),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            x,
            y,
            w,
            h,
            Some(app.hwnd),
            None,
            Some(instance),
            None,
        ) else {
            return;
        };
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let filter = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            s(8),
            s(8),
            client.right - s(16),
            s(26),
            Some(hwnd),
            Some(HMENU(ID_FILTER as usize as *mut _)),
            Some(instance),
            None,
        )
        .unwrap_or_default();
        let list = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("LISTBOX"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE((LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32),
            s(8),
            s(42),
            client.right - s(16),
            client.bottom - s(50),
            Some(hwnd),
            Some(HMENU(ID_LIST as usize as *mut _)),
            Some(instance),
            None,
        )
        .unwrap_or_default();
        for c in [filter, list] {
            SendMessageW(c, WM_SETFONT, Some(WPARAM(app.font.0 as usize)), Some(LPARAM(1)));
        }
        let cue = super::wz("名前で絞り込み");
        SendMessageW(
            filter,
            EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
        LIST.with(|l| {
            *l.borrow_mut() = Some(ListState {
                hwnd,
                main: app.hwnd,
                filter,
                list,
                all,
                shown: Vec::new(),
            })
        });
        refill();
        // Preselect the second entry (the previously used workspace) for quick toggling.
        if LIST.with(|l| l.borrow().as_ref().is_some_and(|s| s.shown.len() > 1)) {
            SendMessageW(list, LB_SETCURSEL, Some(WPARAM(1)), None);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(Some(filter));
    }
}

extern "system" fn list_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = super::loword(wp.0) as i32;
                let code = super::hiword(wp.0) as u32;
                if id == ID_FILTER && code == EN_CHANGE {
                    refill();
                } else if id == ID_LIST && code == LBN_DBLCLK {
                    choose_selected();
                }
                LRESULT(0)
            }
            WM_CONTEXTMENU => {
                if let Some((main, name)) = selected_name()
                    && let Some(app) = App::from_hwnd(main)
                {
                    let (x, y) = super::point_from_lparam(lp);
                    let items = vec![
                        Some(("名前の変更...".to_string(), Command::WorkspaceRename, false)),
                        Some(("削除".to_string(), Command::WorkspaceDelete, false)),
                    ];
                    let chosen = app.menu.track(hwnd, items, x, y);
                    if let Some(c) = chosen {
                        close_list();
                        match c {
                            Command::WorkspaceRename => app.workspace_rename(&name),
                            Command::WorkspaceDelete => app.workspace_delete(&name),
                            _ => {}
                        }
                    }
                }
                LRESULT(0)
            }
            WM_ACTIVATE => {
                if super::loword(wp.0) as u32 == WA_INACTIVE {
                    let other = HWND(lp.0 as *mut _);
                    // Close when focus leaves to anything but our own popups (menus).
                    if !other.0.is_null() && GetParent(other).ok() != Some(hwnd) {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                LIST.with(|l| {
                    if l.borrow().as_ref().is_some_and(|s| s.hwnd == hwnd) {
                        *l.borrow_mut() = None;
                    }
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

// ---- modal prompt ----

type Validator = Box<dyn Fn(&str) -> std::result::Result<(), String>>;

struct PromptState {
    edit: HWND,
    error: HWND,
    result: Option<Option<String>>,
    validate: Validator,
}

thread_local! {
    static PROMPT: RefCell<Option<PromptState>> = const { RefCell::new(None) };
}

/// Ask for a line of text. Returns None when cancelled.
pub fn prompt(
    owner: HWND,
    title: &str,
    label: &str,
    initial: &str,
    font: HFONT,
    validate: impl Fn(&str) -> std::result::Result<(), String> + 'static,
) -> Option<String> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        let wc = WNDCLASSW {
            hInstance: instance,
            lpszClassName: PROMPT_CLASS,
            lpfnWndProc: Some(prompt_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let dpi = super::dpi::window_dpi(owner);
        let s = |v| scale(v, dpi);
        let mut rc = RECT::default();
        let _ = GetWindowRect(owner, &mut rc);
        let (w, h) = (s(420), s(180));
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            PROMPT_CLASS,
            &HSTRING::from(title),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            (rc.left + rc.right - w) / 2,
            rc.top + s(120),
            w,
            h,
            Some(owner),
            None,
            Some(instance),
            None,
        )
        .ok()?;
        let mk = |class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, x, y, cw, ch, id: i32| {
            let c = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                cw,
                ch,
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(instance),
                None,
            )
            .unwrap_or_default();
            SendMessageW(c, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            c
        };
        mk(
            w!("STATIC"),
            label,
            WINDOW_STYLE(0),
            WINDOW_EX_STYLE(0),
            s(12),
            s(12),
            s(380),
            s(20),
            0,
        );
        let edit = mk(
            w!("EDIT"),
            initial,
            WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            WS_EX_CLIENTEDGE,
            s(12),
            s(36),
            s(380),
            s(26),
            100,
        );
        let error = mk(
            w!("STATIC"),
            "",
            WINDOW_STYLE(0),
            WINDOW_EX_STYLE(0),
            s(12),
            s(68),
            s(380),
            s(20),
            0,
        );
        mk(
            w!("BUTTON"),
            "OK",
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            WINDOW_EX_STYLE(0),
            s(212),
            s(100),
            s(86),
            s(28),
            ID_OK,
        );
        mk(
            w!("BUTTON"),
            "キャンセル",
            WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            WINDOW_EX_STYLE(0),
            s(306),
            s(100),
            s(86),
            s(28),
            ID_CANCEL,
        );
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        SHAutoComplete(edit, SHACF_AUTOSUGGEST_FORCE_OFF | SHACF_AUTOAPPEND_FORCE_OFF).ok();
        PROMPT.with(|p| {
            *p.borrow_mut() = Some(PromptState {
                edit,
                error,
                result: None,
                validate: Box::new(validate),
            })
        });

        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(Some(edit));
        let mut msg = MSG::default();
        loop {
            if PROMPT.with(|p| p.borrow().as_ref().is_none_or(|s| s.result.is_some())) {
                break;
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            if msg.message == WM_KEYDOWN && VIRTUAL_KEY(msg.wParam.0 as u16) == VK_ESCAPE {
                finish_prompt(false);
                continue;
            }
            if IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(hwnd);
        let _ = SetForegroundWindow(owner);
        PROMPT.with(|p| p.borrow_mut().take()).and_then(|s| s.result.flatten())
    }
}

fn finish_prompt(ok: bool) {
    PROMPT.with(|p| {
        let mut p = p.borrow_mut();
        let Some(s) = p.as_mut() else { return };
        if !ok {
            s.result = Some(None);
            return;
        }
        let text = super::window_text(s.edit);
        match (s.validate)(&text) {
            Ok(()) => s.result = Some(Some(text)),
            Err(e) => unsafe {
                let _ = SetWindowTextW(s.error, &HSTRING::from(e));
                windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONWARNING).ok();
                let _ = SetFocus(Some(s.edit));
            },
        }
    });
}

extern "system" fn prompt_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                match super::loword(wp.0) as i32 {
                    ID_OK => finish_prompt(true),
                    ID_CANCEL => finish_prompt(false),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                let is_error = PROMPT.with(|p| p.borrow().as_ref().is_some_and(|s| s.error.0 == lp.0 as *mut _));
                let hdc = HDC(wp.0 as *mut _);
                if is_error {
                    SetTextColor(hdc, COLORREF(0x0000_20C0));
                }
                SetBkMode(hdc, TRANSPARENT);
                LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
            }
            WM_CLOSE => {
                finish_prompt(false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
