//! Display formatting of sizes, dates and attributes.

use windows::Win32::Foundation::*;
use windows::Win32::Globalization::*;
use windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
use windows::Win32::System::Time::FileTimeToSystemTime;
use windows::core::PCWSTR;

use crate::model::listing::*;

/// Explorer's details-view size: whole kilobytes rounded up, with thousands separators.
pub fn size_kb(bytes: u64) -> String {
    let kb = bytes.div_ceil(1024);
    format!("{} KB", group_thousands(kb))
}

pub fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Human-readable size with three significant digits (status bar).
pub fn size_human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} バイト");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1000.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    let s = if v < 10.0 {
        format!("{:.2}", (v * 100.0).floor() / 100.0)
    } else if v < 100.0 {
        format!("{:.1}", (v * 10.0).floor() / 10.0)
    } else {
        format!("{}", v.floor())
    };
    format!("{s} {}", UNITS[u])
}

pub fn attributes(attrs: u32) -> String {
    let mut s = String::new();
    for (bit, c) in [
        (ATTR_READONLY, 'R'),
        (ATTR_HIDDEN, 'H'),
        (ATTR_SYSTEM, 'S'),
        (ATTR_ARCHIVE, 'A'),
    ] {
        if attrs & bit != 0 {
            s.push(c);
        }
    }
    s
}

/// Local short date and time without seconds, e.g. "2026/10/17 11:07".
pub fn datetime(ft: u64) -> String {
    if ft == 0 {
        return String::new();
    }
    unsafe {
        let utc = FILETIME {
            dwLowDateTime: ft as u32,
            dwHighDateTime: (ft >> 32) as u32,
        };
        let mut local = FILETIME::default();
        let mut st = SYSTEMTIME::default();
        if FileTimeToLocalFileTime(&utc, &mut local).is_err() || FileTimeToSystemTime(&local, &mut st).is_err() {
            return String::new();
        }
        let mut d = [0u16; 64];
        let mut t = [0u16; 64];
        let dn = GetDateFormatEx(
            PCWSTR::null(),
            DATE_SHORTDATE,
            Some(&st),
            PCWSTR::null(),
            Some(&mut d),
            PCWSTR::null(),
        );
        let tn = GetTimeFormatEx(PCWSTR::null(), TIME_NOSECONDS, Some(&st), PCWSTR::null(), Some(&mut t));
        let dn = (dn.max(1) - 1) as usize;
        let tn = (tn.max(1) - 1) as usize;
        format!(
            "{} {}",
            String::from_utf16_lossy(&d[..dn]),
            String::from_utf16_lossy(&t[..tn])
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kb() {
        assert_eq!(size_kb(0), "0 KB");
        assert_eq!(size_kb(1), "1 KB");
        assert_eq!(size_kb(1024), "1 KB");
        assert_eq!(size_kb(1025), "2 KB");
        assert_eq!(size_kb(1024 * 1234567), "1,234,567 KB");
    }

    #[test]
    fn human() {
        assert_eq!(size_human(10), "10 バイト");
        assert_eq!(size_human(1536), "1.50 KB");
        assert_eq!(size_human(4_404_019), "4.19 MB");
        assert_eq!(size_human(150 * 1024 * 1024), "150 MB");
        assert_eq!(size_human(1000 * 1024), "0.97 MB");
    }

    #[test]
    fn attrs() {
        assert_eq!(attributes(ATTR_READONLY | ATTR_ARCHIVE), "RA");
        assert_eq!(attributes(0), "");
    }
}
