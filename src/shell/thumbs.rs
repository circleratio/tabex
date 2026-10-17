//! Thumbnail fetching (worker thread) and an LRU cache of HBITMAPs (UI thread).

use std::collections::{HashMap, HashSet, VecDeque};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::core::*;

pub const CACHE_BYTES: usize = 32 * 1024 * 1024;

/// Thumbnail of a file, or None if it has none (THUMBNAILONLY: no icon fallback).
pub fn fetch(path: &str, px: i32) -> Option<HBITMAP> {
    unsafe {
        let item: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        item.GetImage(SIZE { cx: px, cy: px }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK)
            .ok()
    }
}

fn bitmap_size(b: HBITMAP) -> (i32, i32) {
    let mut bm = BITMAP::default();
    unsafe {
        GetObjectW(
            b.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
    }
    (bm.bmWidth, bm.bmHeight.abs())
}

#[derive(Default)]
pub struct ThumbCache {
    map: HashMap<String, HBITMAP>,
    order: VecDeque<String>,
    bytes: usize,
    pending: HashSet<String>,
    none: HashSet<String>,
}

pub enum Lookup {
    Hit(HBITMAP),
    /// Not cached: the caller should request it (now marked pending).
    Request,
    /// Pending or known to have no thumbnail.
    Skip,
}

impl ThumbCache {
    pub fn lookup(&mut self, key: &str) -> Lookup {
        if let Some(&b) = self.map.get(key) {
            return Lookup::Hit(b);
        }
        if self.pending.contains(key) || self.none.contains(key) {
            return Lookup::Skip;
        }
        self.pending.insert(key.to_string());
        Lookup::Request
    }

    pub fn get(&self, key: &str) -> Option<HBITMAP> {
        self.map.get(key).copied()
    }

    pub fn insert(&mut self, key: String, bmp: Option<HBITMAP>) {
        self.pending.remove(&key);
        let Some(b) = bmp else {
            self.none.insert(key);
            return;
        };
        let (w, h) = bitmap_size(b);
        self.bytes += (w * h * 4) as usize;
        self.order.push_back(key.clone());
        if let Some(old) = self.map.insert(key, b) {
            unsafe {
                let _ = DeleteObject(old.into());
            }
        }
        while self.bytes > CACHE_BYTES {
            let Some(k) = self.order.pop_front() else { break };
            if let Some(b) = self.map.remove(&k) {
                let (w, h) = bitmap_size(b);
                self.bytes = self.bytes.saturating_sub((w * h * 4) as usize);
                unsafe {
                    let _ = DeleteObject(b.into());
                }
            }
        }
    }

    /// Forget cached state for a changed file.
    pub fn invalidate(&mut self, key: &str) {
        self.none.remove(key);
        if let Some(b) = self.map.remove(key) {
            let (w, h) = bitmap_size(b);
            self.bytes = self.bytes.saturating_sub((w * h * 4) as usize);
            self.order.retain(|k| k != key);
            unsafe {
                let _ = DeleteObject(b.into());
            }
        }
    }
}

/// Draw `bmp` centered and scaled to fit in `rc`, honouring its alpha channel.
pub fn draw(hdc: HDC, bmp: HBITMAP, rc: RECT) {
    let (w, h) = bitmap_size(bmp);
    if w <= 0 || h <= 0 {
        return;
    }
    let bw = rc.right - rc.left;
    let bh = rc.bottom - rc.top;
    let scale = (bw as f32 / w as f32).min(bh as f32 / h as f32).min(1.0);
    let (dw, dh) = ((w as f32 * scale) as i32, (h as f32 * scale) as i32);
    let x = rc.left + (bw - dw) / 2;
    let y = rc.top + (bh - dh) / 2;
    unsafe {
        let mem = CreateCompatibleDC(Some(hdc));
        let old = SelectObject(mem, bmp.into());
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = SetStretchBltMode(hdc, HALFTONE);
        let _ = AlphaBlend(hdc, x, y, dw, dh, mem, 0, 0, w, h, blend);
        SelectObject(mem, old);
        let _ = DeleteDC(mem);
    }
}
