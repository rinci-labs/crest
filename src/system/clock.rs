//! Local time, formatted in English with a 24-hour clock.

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount64};
use windows::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
};

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/// One day in FILETIME units (100 ns).
const DAY_TICKS: u64 = 24 * 60 * 60 * 10_000_000;

pub fn now() -> SYSTEMTIME {
    unsafe { GetLocalTime() }
}

/// "19:21"
pub fn time(t: &SYSTEMTIME) -> String {
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

/// "Mon, 19 Oct"
pub fn date(t: &SYSTEMTIME) -> String {
    format!("{}, {} {}", DAYS[t.wDayOfWeek as usize % 7], t.wDay, MONTHS[(t.wMonth as usize + 11) % 12])
}

/// Daily storage key, e.g. "2026-10-19".
pub fn day_key(t: &SYSTEMTIME) -> String {
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

pub fn greeting(t: &SYSTEMTIME) -> &'static str {
    match t.wHour {
        4..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

/// The last `n` days, oldest first and today last: daily key and the initial of the day name.
pub fn recent_days(n: usize) -> Vec<(String, char)> {
    let today = now();
    let mut ft = FILETIME::default();
    if unsafe { SystemTimeToFileTime(&today, &mut ft) }.is_err() {
        return Vec::new();
    }
    let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    (0..n as u64)
        .rev()
        .filter_map(|back| {
            let t = ticks - back * DAY_TICKS;
            let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
            let mut day = SYSTEMTIME::default();
            unsafe { FileTimeToSystemTime(&ft, &mut day) }.ok()?;
            let initial = DAYS[day.wDayOfWeek as usize % 7].chars().next().unwrap_or(' ');
            Some((day_key(&day), initial))
        })
        .collect()
}

/// Wall-clock time `minutes` from now, e.g. "19:37".
pub fn time_in(minutes: u32) -> String {
    let t = now();
    let total = (t.wHour as u32 * 60 + t.wMinute as u32 + minutes) % (24 * 60);
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Time since the computer was last started: "3 h 12 min" or "2 d 5 h".
pub fn uptime() -> String {
    let minutes = unsafe { GetTickCount64() } / 60_000;
    let (days, hours, mins) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if days > 0 { format!("{days} d {hours} h") } else { format!("{hours} h {mins} min") }
}

/// Track duration: "1:06", or "1:02:03" when over an hour.
pub fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Milliseconds until the next minute, plus a small margin so it has surely passed.
pub fn ms_to_next_minute(t: &SYSTEMTIME) -> u32 {
    let elapsed = t.wSecond as u32 * 1000 + t.wMilliseconds as u32;
    60_000u32.saturating_sub(elapsed) + 15
}

/// A UTC instant as `SYSTEMTIME`, for time zone conversion.
pub fn utc(year: i32, month: u32, day: u32, second: u32) -> Option<SYSTEMTIME> {
    let time = SYSTEMTIME {
        wYear: year.clamp(1601, 30827) as u16,
        wMonth: month as u16,
        wDay: day as u16,
        wHour: (second / 3600) as u16,
        wMinute: ((second / 60) % 60) as u16,
        wSecond: (second % 60) as u16,
        wDayOfWeek: 0,
        wMilliseconds: 0,
    };
    let mut file = FILETIME::default();
    unsafe { SystemTimeToFileTime(&time, &mut file) }.ok()?;
    Some(time)
}

/// A UTC instant as wall-clock time in the user's zone.
pub fn to_local(utc: SYSTEMTIME) -> Option<SYSTEMTIME> {
    let mut wall = SYSTEMTIME::default();
    unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut wall) }.ok()?;
    Some(wall)
}

/// A UTC instant given as seconds since 1970-01-01, as local date (days since 1970) and minute.
pub fn local_moment(seconds: i64) -> Option<(i64, u32)> {
    let (day, in_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let date = crate::tools::agenda::Date::from_days(day);
    let wall = utc(date.year, date.month, date.day, in_day as u32)?;
    let local = to_local(wall)?;
    let day = crate::tools::agenda::Date::new(local.wYear as i32, local.wMonth as u32, local.wDay as u32).days();
    Some((day, local.wHour as u32 * 60 + local.wMinute as u32))
}
