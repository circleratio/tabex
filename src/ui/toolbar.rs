//! Toolbar: a ToolbarWindow32 whose button images are icon font glyphs drawn at the window DPI.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::model::layout::scale;
use crate::model::toolbar::{
    BUTTONS, ButtonId, ButtonKind, Glyph, ToolbarState, VIEW_MODES, checked, enabled, view_mode_index,
};

/// Command id of the first button; the others follow in `BUTTONS` order.
const TOOLBAR_BASE: i32 = 6000;
/// Sizes in 96-DPI pixels.
const ICON_SIZE: i32 = 16;
const BUTTON_SIZE: i32 = 28;

pub struct Toolbar {
    pub hwnd: HWND,
    images: HIMAGELIST,
    disabled: HIMAGELIST,
    dpi: u32,
    /// Skip updates when nothing changed.
    last: Option<ToolbarState>,
    /// TTN_GETDISPINFOW text must outlive the notification.
    tip: Vec<u16>,
}

fn send(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wp)), Some(LPARAM(lp))) }
}

fn makelong(lo: i32, hi: i32) -> isize {
    ((lo as u32 & 0xFFFF) | ((hi as u32 & 0xFFFF) << 16)) as isize
}

/// Image index of the view mode glyphs, which follow the per-button glyphs.
fn view_mode_image(st: &ToolbarState) -> i32 {
    (BUTTONS.len() + view_mode_index(st.view_mode)) as i32
}

/// Segoe Fluent Icons (Windows 11) when installed, otherwise Segoe MDL2 Assets (Windows 10).
fn icon_face() -> &'static str {
    static FACE: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FACE.get_or_init(|| {
        const FLUENT: &str = "Segoe Fluent Icons";
        let font = create_font(FLUENT, 16, FW_NORMAL.0 as i32);
        let mut buf = [0u16; 64];
        let n = unsafe {
            let hdc = CreateCompatibleDC(None);
            let old = SelectObject(hdc, font.into());
            let n = GetTextFaceW(hdc, Some(&mut buf));
            SelectObject(hdc, old);
            let _ = DeleteDC(hdc);
            let _ = DeleteObject(font.into());
            n
        };
        let got = String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize]);
        if got.eq_ignore_ascii_case(FLUENT) {
            FLUENT
        } else {
            "Segoe MDL2 Assets"
        }
    })
}

fn create_font(face: &str, height: i32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            0,
            &HSTRING::from(face),
        )
    }
}

/// Draw `glyph` white on black into a 32bpp DIB, then turn brightness into alpha of `color`
/// (premultiplied), so the image list blends it over any background.
fn render(glyph: Glyph, size: i32, color: COLORREF) -> Option<HBITMAP> {
    unsafe {
        let hdc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(hdc);
            return None;
        };
        let old_bmp = SelectObject(hdc, bmp.into());
        let (text, font) = match glyph {
            Glyph::Icon(c) => (c.to_string(), create_font(icon_face(), size, FW_NORMAL.0 as i32)),
            Glyph::Text(s) => (s.to_string(), fit_text_font(hdc, s, size)),
        };
        let old_font = SelectObject(hdc, font.into());
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00FF_FFFF));
        let mut w: Vec<u16> = text.encode_utf16().collect();
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: size,
            bottom: size,
        };
        DrawTextW(
            hdc,
            &mut w,
            &mut rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_NOCLIP,
        );
        let _ = GdiFlush();
        let (r, g, b) = (color.0 & 0xFF, (color.0 >> 8) & 0xFF, (color.0 >> 16) & 0xFF);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        for p in px.iter_mut() {
            let a = (*p & 0xFF).max((*p >> 8) & 0xFF).max((*p >> 16) & 0xFF);
            *p = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
        }
        SelectObject(hdc, old_font);
        SelectObject(hdc, old_bmp);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(hdc);
        Some(bmp)
    }
}

/// Bold UI font, shrunk until `text` fits in `size` pixels.
fn fit_text_font(hdc: HDC, text: &str, size: i32) -> HFONT {
    let w: Vec<u16> = text.encode_utf16().collect();
    let mut height = size;
    loop {
        let font = create_font("Segoe UI", height, FW_BOLD.0 as i32);
        let mut ext = SIZE::default();
        unsafe {
            let old = SelectObject(hdc, font.into());
            let _ = GetTextExtentPoint32W(hdc, &w, &mut ext);
            SelectObject(hdc, old);
        }
        if ext.cx <= size || height <= 6 {
            return font;
        }
        unsafe {
            let _ = DeleteObject(font.into());
        }
        height -= 1;
    }
}

/// Per-button glyphs in `BUTTONS` order (ViewMode gets the details glyph), then `VIEW_MODES`.
fn image_list(size: i32, color: COLORREF) -> HIMAGELIST {
    let glyphs = BUTTONS.iter().map(|b| b.glyph).chain(VIEW_MODES.iter().map(|v| v.2));
    unsafe {
        let himl = ImageList_Create(size, size, ILC_COLOR32, (BUTTONS.len() + VIEW_MODES.len()) as i32, 0);
        for g in glyphs {
            if let Some(bmp) = render(g, size, color) {
                ImageList_Add(himl, bmp, None);
                let _ = DeleteObject(bmp.into());
            }
        }
        himl
    }
}

impl Toolbar {
    pub fn create(parent: HWND, id: i32, dpi: u32) -> Result<Toolbar> {
        let style =
            TBSTYLE_FLAT | TBSTYLE_TOOLTIPS | CCS_NODIVIDER as u32 | CCS_NOPARENTALIGN as u32 | CCS_NORESIZE as u32;
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                TOOLBARCLASSNAMEW,
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                0,
                0,
                0,
                0,
                Some(parent),
                Some(HMENU(id as usize as *mut _)),
                None,
                None,
            )?
        };
        send(hwnd, TB_BUTTONSTRUCTSIZE, std::mem::size_of::<TBBUTTON>(), 0);
        let mut tb = Toolbar {
            hwnd,
            images: HIMAGELIST::default(),
            disabled: HIMAGELIST::default(),
            dpi,
            last: None,
            tip: Vec::new(),
        };
        tb.rebuild_images();
        let mut buttons = Vec::new();
        for (i, b) in BUTTONS.iter().enumerate() {
            if i > 0 && BUTTONS[i - 1].group != b.group {
                buttons.push(TBBUTTON {
                    fsStyle: BTNS_SEP as u8,
                    ..Default::default()
                });
            }
            let style = match b.kind {
                ButtonKind::Push => BTNS_BUTTON,
                ButtonKind::Check => BTNS_CHECK,
                ButtonKind::Dropdown => BTNS_WHOLEDROPDOWN,
            };
            buttons.push(TBBUTTON {
                iBitmap: i as i32,
                idCommand: TOOLBAR_BASE + i as i32,
                fsState: TBSTATE_ENABLED as u8,
                fsStyle: style as u8,
                ..Default::default()
            });
        }
        send(hwnd, TB_ADDBUTTONSW, buttons.len(), buttons.as_ptr() as isize);
        tb.set_button_size();
        Ok(tb)
    }

    fn set_button_size(&self) {
        let s = scale(BUTTON_SIZE, self.dpi);
        send(self.hwnd, TB_SETBUTTONSIZE, 0, makelong(s, s));
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        self.dpi = dpi;
        self.rebuild_images();
        self.set_button_size();
    }

    /// Redraw the glyphs, e.g. after the system colors changed.
    pub fn rebuild_images(&mut self) {
        let size = scale(ICON_SIZE, self.dpi);
        let (normal, gray) = unsafe { (GetSysColor(COLOR_BTNTEXT), GetSysColor(COLOR_GRAYTEXT)) };
        let images = image_list(size, COLORREF(normal));
        let disabled = image_list(size, COLORREF(gray));
        send(self.hwnd, TB_SETIMAGELIST, 0, images.0);
        send(self.hwnd, TB_SETDISABLEDIMAGELIST, 0, disabled.0);
        unsafe {
            if !self.images.is_invalid() {
                let _ = ImageList_Destroy(Some(self.images));
                let _ = ImageList_Destroy(Some(self.disabled));
            }
        }
        self.images = images;
        self.disabled = disabled;
        // The ViewMode image index has to be sent again.
        if let Some(st) = self.last.take() {
            self.apply(&st);
        }
    }

    pub fn apply(&mut self, st: &ToolbarState) {
        if self.last.as_ref() == Some(st) {
            return;
        }
        for b in &BUTTONS {
            let id = Self::button_id(b.id) as usize;
            if b.kind == ButtonKind::Check {
                send(self.hwnd, TB_CHECKBUTTON, id, checked(b.id, st) as isize);
            }
            send(self.hwnd, TB_ENABLEBUTTON, id, enabled(b.id, st) as isize);
            if b.id == ButtonId::ViewMode {
                send(self.hwnd, TB_CHANGEBITMAP, id, view_mode_image(st) as isize);
            }
        }
        self.last = Some(*st);
    }

    pub fn button_id(id: ButtonId) -> i32 {
        TOOLBAR_BASE + BUTTONS.iter().position(|b| b.id == id).unwrap_or(0) as i32
    }

    pub fn button_of(cmd_id: i32) -> Option<ButtonId> {
        let i = usize::try_from(cmd_id.checked_sub(TOOLBAR_BASE)?).ok()?;
        BUTTONS.get(i).map(|b| b.id)
    }

    /// Client rectangle of a button, for placing the dropdown menu.
    pub fn button_rect(&self, id: ButtonId) -> RECT {
        let mut rc = RECT::default();
        send(
            self.hwnd,
            TB_GETRECT,
            Self::button_id(id) as usize,
            &mut rc as *mut RECT as isize,
        );
        rc
    }

    pub fn set_tip(&mut self, text: &str) -> PWSTR {
        self.tip = text.encode_utf16().chain(std::iter::once(0)).collect();
        PWSTR(self.tip.as_mut_ptr())
    }
}
