//! Date and time strings from outside sources: RFC 3339 (Google Calendar), RFC 5322 (mail
//! headers), and Unix seconds for age labels. Everything is converted to local wall-clock time.

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::SystemTimeToFileTime;

/// Seconds since the Unix epoch, now.
pub fn now_seconds() -> i64 {
    let t = crate::system::clock::now();
    wall_seconds(&t).unwrap_or(0)
}

/// Seconds since the Unix epoch for a wall-clock `SYSTEMTIME` read as UTC.
fn wall_seconds(t: &SYSTEMTIME) -> Option<i64> {
    let mut file = windows::Win32::Foundation::FILETIME::default();
    unsafe { SystemTimeToFileTime(t, &mut file) }.ok()?;
    let ticks = ((file.dwHighDateTime as u64) << 32) | file.dwLowDateTime as u64;
    // FILETIME counts from 1601; Unix time from 1970, in 100 ns units.
    Some((ticks as i64 - 116_444_736_000_000_000) / 10_000_000)
}

/// Now, formatted as RFC 3339 in UTC, for Google's `timeMin`.
pub fn now_rfc3339() -> String {
    let seconds = now_seconds();
    rfc3339(seconds)
}

/// `days` from now, as RFC 3339 in UTC, for Google's `timeMax`.
pub fn in_days_rfc3339(days: i64) -> String {
    rfc3339(now_seconds() + days * 86_400)
}

fn rfc3339(seconds: i64) -> String {
    let date = crate::tools::agenda::Date::from_days(seconds.div_euclid(86_400));
    let secs = seconds.rem_euclid(86_400);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year,
        date.month,
        date.day,
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// A local moment from an RFC 3339 stamp (`2026-10-20T09:00:00+02:00` or `…Z`), as days since
/// 1970 and minutes after midnight.
pub fn local_moment(stamp: &str) -> Option<(i64, u32)> {
    let (date, rest) = stamp.trim().split_once('T')?;
    let day = day(date)?;
    // Split off the zone: "09:00:00+02:00" → "09:00:00" and 120 minutes.
    let (time, zone) = match rest.find(['Z', '+', '-']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    let mut parts = time.split(':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    // No zone at all: the stamp names a wall-clock time rather than an instant, so shifting it
    // through the user's zone would move the event. Some calendar APIs omit the offset.
    if zone.is_empty() {
        if hour > 23 || minute > 59 {
            return None;
        }
        return Some((day, hour as u32 * 60 + minute as u32));
    }
    let offset = match zone {
        "Z" => 0,
        zone => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let zone = &zone[1..];
            let (hours, minutes) = zone.split_once(':').unwrap_or((zone, "0"));
            sign * (hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?)
        }
    };
    // The stamp names a UTC instant; the panel works in the user's own zone.
    let utc_seconds = day * 86_400 + hour * 3600 + minute * 60 - offset * 60;
    crate::system::clock::local_moment(utc_seconds)
}

/// `2026-10-20` → days since 1970-01-01.
pub fn day(date: &str) -> Option<i64> {
    let mut parts = date.trim().split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    Some(crate::tools::agenda::Date::new(year, month, day).days())
}

/// Unix seconds from an RFC 5322 `Date` header, e.g. "Tue, 20 Oct 2026 09:12:33 +0200".
pub fn parse_rfc5322(text: &str) -> Option<i64> {
    let mut words = text.split_whitespace();
    let mut date = None;
    let mut time = None;
    let mut offset = 0i64;
    while let Some(word) = words.next() {
        // "Tue," and "20 Oct 2026" are followed by the time.
        if word.ends_with(',') && date.is_none() {
            let day = words.next()?;
            let month = words.next()?;
            let year = words.next()?;
            date = Some((day, month, year));
        } else if word.contains(':') {
            time = Some(word);
        } else if let Some(zone) = numeric_offset(word) {
            offset = zone;
        }
    }
    let ((day, month, year), time) = (date?, time?);
    let month = month_number(month)?;
    let day: u32 = day.parse().ok()?;
    let year = year_number(year)?;
    let mut parts = time.split(':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    let second: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let day = crate::tools::agenda::Date::new(year, month, day).days();
    Some(day * 86_400 + hour * 3600 + minute * 60 + second - offset * 60)
}

/// `+0200` / `-0530` → minutes east of UTC.
fn numeric_offset(word: &str) -> Option<i64> {
    if word.len() != 5 {
        return None;
    }
    let (sign, rest) = (word.as_bytes()[0], &word[1..]);
    if sign != b'+' && sign != b'-' {
        return None;
    }
    let hours: i64 = rest[0..2].parse().ok()?;
    let minutes: i64 = rest[2..4].parse().ok()?;
    Some(if sign == b'-' { -(hours * 60 + minutes) } else { hours * 60 + minutes })
}

/// RFC 5322 §4.3: a two-digit year between 00 and 49 is in the 2000s, one between 50 and 99 or
/// any three-digit year is in the 1900s. Mail clients still send "Tue, 20 Oct 26 …".
fn year_number(word: &str) -> Option<i32> {
    let year: i32 = word.parse().ok()?;
    Some(match word.len() {
        2 if year <= 49 => year + 2000,
        2 | 3 => year + 1900,
        _ => year,
    })
}

fn month_number(name: &str) -> Option<u32> {
    const NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    NAMES.iter().position(|m| m.eq_ignore_ascii_case(&name[..3.min(name.len())])).map(|i| i as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_offsets_shift_to_utc() {
        // Two stamps of the same instant agree, whatever their offset.
        assert_eq!(local_moment("2026-10-20T09:00:00+02:00"), local_moment("2026-10-20T07:00:00Z"));
        assert_eq!(local_moment("2026-10-20T09:00:00-05:00"), local_moment("2026-10-20T14:00:00Z"));
        assert!(local_moment("not a stamp").is_none());
    }

    #[test]
    fn mail_dates_parse() {
        // 20 Oct 2026 09:12:33 +0200 = 07:12:33 UTC.
        let expected = day("2026-10-20").unwrap() * 86_400 + 7 * 3600 + 12 * 60 + 33;
        assert_eq!(parse_rfc5322("Tue, 20 Oct 2026 09:12:33 +0200"), Some(expected));
        assert_eq!(parse_rfc5322("Tue, 20 Oct 2026 09:12:33 -0000"), Some(expected + 2 * 3600));
        assert_eq!(parse_rfc5322("garbage"), None);
    }

    #[test]
    fn mail_dates_with_a_two_digit_year() {
        // Mail clients still send the year as two digits; read as a plain number it would be
        // year 26, placing every message two millennia in the past.
        let full = parse_rfc5322("Tue, 20 Oct 2026 09:12:33 +0200");
        assert_eq!(parse_rfc5322("Tue, 20 Oct 26 09:12:33 +0200"), full);
        // 50 and up are the 1900s, per RFC 5322.
        let old = parse_rfc5322("Tue, 20 Oct 76 09:12:33 +0200").unwrap();
        assert_eq!(crate::tools::agenda::Date::from_days(old.div_euclid(86_400)).year, 1976);
    }

    #[test]
    fn mail_dates_without_seconds() {
        // A missing "ss" part means zero seconds, not a rejected header.
        let with = parse_rfc5322("Tue, 20 Oct 2026 09:12:33 +0200").unwrap();
        assert_eq!(parse_rfc5322("Tue, 20 Oct 2026 09:12 +0200"), Some(with - 33));
    }

    #[test]
    fn a_stamp_without_a_zone_is_local_time() {
        // A floating stamp is already wall-clock time: converting it as UTC would move the event
        // by the user's offset. The minute must come through unchanged.
        let (local_day, minute) = local_moment("2026-10-20T09:00:00").unwrap();
        assert_eq!(local_day, day("2026-10-20").unwrap());
        assert_eq!(minute, 9 * 60);
        // Midnight stays on its own day rather than sliding backwards.
        assert_eq!(local_moment("2026-10-20T00:00:00"), Some((day("2026-10-20").unwrap(), 0)));
    }

    #[test]
    fn days_and_times_format() {
        let stamp = rfc3339(day("2026-10-20").unwrap() * 86_400 + 9 * 3600);
        assert_eq!(stamp, "2026-10-20T09:00:00Z");
    }
}

