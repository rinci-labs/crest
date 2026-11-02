//! Plan usage limits the agents already record locally: how much of each rate-limit window is
//! used and when it resets.
//!
//! Codex writes the account's `rate_limits` into every `token_count` event of its rollout
//! (`primary` is the 5-hour window, `secondary` the weekly one). Only the tail of the newest
//! rollout is read; nothing is asked of the network.
//!
//! Claude Code keeps no such file; it hands `rate_limits` only to its status line command. The
//! opt-in relay `crest.exe --statusline [command…]` (see [`relay`]) copies the two windows into
//! a one-line-per-window text file and passes the payload on to the user's own status line.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows::Data::Json::JsonObject;
use windows::core::HSTRING;

use super::Kind;
use super::codex::latest_days;
use super::hooks::records_dir;
use super::util::{mtime, number_field, object, read_small, read_tail};

/// Bytes read from the end of the newest rollout; `token_count` events come every turn.
const TAIL: u64 = 64 * 1024;
/// A rollout this old says nothing about the current windows.
const STALE: u64 = 7 * 24 * 60 * 60;
/// Status line payloads are a few kilobytes; anything bigger is cut.
const INPUT_MAX: u64 = 64 * 1024;
/// The Claude windows file, beside the hook records: `<window> <used> <resets_at>` per line.
const CLAUDE_FILE: &str = "limits-claude.txt";
/// omp's account database; its `usage_history` table holds the provider's usage snapshots.
const OMP_DB: &str = "agent.db";
/// A database larger than this is not omp's small account store.
const OMP_DB_MAX: u64 = 64 * 1024 * 1024;

/// One rate-limit window, e.g. Codex's 5-hour window at 42%.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Meter {
    /// Short window name: "5h", "Week", or the length in hours or days.
    pub window: String,
    /// Share of the window used, 0..=100.
    pub used: u8,
    /// When the window resets (Unix seconds).
    pub resets_at: u64,
}

/// The windows one agent reported, tightest first.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Limits {
    pub kind: Kind,
    pub meters: Vec<Meter>,
}

impl Limits {
    /// The most used window, which is the one that stops the agent first.
    pub fn tightest(&self) -> Option<&Meter> {
        self.meters.first()
    }
}

/// Every agent with a known, still-running window.
pub fn read(home: &Path, now: u64) -> Vec<Limits> {
    let claude = records_dir().and_then(|dir| read_small(&dir.join(CLAUDE_FILE), 1024)).and_then(|text| claude(&text, now));
    let mut all: Vec<Limits> = claude
        .into_iter()
        .chain(codex(&home.join(".codex").join("sessions"), now))
        .chain(omp(&home.join(".omp").join("agent").join(OMP_DB), now))
        .collect();
    // The agent closest to its limit comes first.
    all.sort_by_key(|limits| std::cmp::Reverse(limits.tightest().map_or(0, |meter| meter.used)));
    all
}

/// Claude's windows from the relay's text file.
fn claude(text: &str, now: u64) -> Option<Limits> {
    let meters = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (window, used, resets_at) = (parts.next()?, parts.next()?.parse::<u8>().ok()?, parts.next()?.parse::<u64>().ok()?);
            (resets_at > now).then(|| Meter { window: window.to_string(), used: used.min(100), resets_at })
        })
        .collect();
    let meters = sorted(meters);
    (!meters.is_empty()).then_some(Limits { kind: Kind::Claude, meters })
}

/// `crest.exe --statusline [command…]`, set as Claude Code's `statusLine` command: record the
/// payload's `rate_limits` windows, then run `command` (the user's own status line, if any)
/// with the same payload and let its output through. Only the two percentages and reset times
/// are kept; the rest of the payload (model, paths, cost) is never written.
pub fn relay(next: &[String]) {
    let mut input = Vec::new();
    let _ = std::io::stdin().take(INPUT_MAX).read_to_end(&mut input);
    let text = String::from_utf8_lossy(&input);
    if let Some(lines) = claude_lines(&text)
        && let Some(dir) = records_dir()
    {
        let path = dir.join(CLAUDE_FILE);
        // The status line refreshes often; write only when a window moved.
        if read_small(&path, 1024).as_deref() != Some(lines.as_str()) {
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(path, lines);
        }
    }
    let Some((program, args)) = next.split_first() else { return };
    let Ok(mut child) = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::inherit()).spawn() else { return };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&input);
    }
    let _ = child.wait();
}

/// The text file body for a status line payload: one `<window> <used> <resets_at>` line per
/// window present, or `None` when the payload has no `rate_limits` (API key, first response).
fn claude_lines(payload: &str) -> Option<String> {
    let limits = object(payload)?.GetNamedObject(&HSTRING::from("rate_limits")).ok()?;
    let mut out = String::new();
    for (key, name) in [("five_hour", "5h"), ("seven_day", "Week")] {
        let Ok(window) = limits.GetNamedObject(&HSTRING::from(key)) else { continue };
        let (Some(used), Some(resets_at)) = (number_field(&window, "used_percentage"), number_field(&window, "resets_at")) else {
            continue;
        };
        out.push_str(&format!("{name} {} {}\n", used.clamp(0.0, 100.0).round() as u8, resets_at.max(0.0) as u64));
    }
    (!out.is_empty()).then_some(out)
}

/// omp's windows: the newest snapshot of each limit in `usage_history` (`used_fraction` 0..1,
/// `resets_at` in ms). Read-only, through Windows' own SQLite; only those columns are read.
fn omp(db: &Path, now: u64) -> Option<Limits> {
    if fs::metadata(db).ok()?.len() > OMP_DB_MAX {
        return None;
    }
    let rows = sqlite::query(db, "SELECT limit_id, window_label, used_fraction, resets_at FROM usage_history \
         WHERE used_fraction IS NOT NULL AND resets_at IS NOT NULL ORDER BY recorded_at DESC LIMIT 32")?;
    let mut seen = Vec::new();
    let meters = rows
        .into_iter()
        .filter_map(|row| {
            let [id, label, used, resets] = row.as_slice() else { return None };
            if seen.contains(id) {
                return None;
            }
            seen.push(id.clone());
            let resets_at = resets.parse::<f64>().ok()? as u64 / 1_000;
            let used = (used.parse::<f64>().ok()? * 100.0).round().clamp(0.0, 100.0) as u8;
            (resets_at > now).then(|| Meter { window: omp_window(id, label), used, resets_at })
        })
        .collect();
    let meters = sorted(meters);
    (!meters.is_empty()).then_some(Limits { kind: Kind::Omp, meters })
}

/// A short window name from omp's limit id ("commandcode:5h") or its label ("Weekly").
fn omp_window(id: &str, label: &str) -> String {
    match id.split_once(':').map_or("", |(_, window)| window) {
        "7d" => "Week".to_string(),
        short if !short.is_empty() && short.len() <= 4 => short.to_string(),
        _ if label.eq_ignore_ascii_case("weekly") => "Week".to_string(),
        _ => label.chars().take(6).collect(),
    }
}

/// The few calls of Windows' `winsqlite3.dll` (part of Windows 10 and 11) needed to run one
/// read-only query. No third-party crate: the functions are bound with `windows-link`.
mod sqlite {
    use std::ffi::{CStr, c_void};
    use std::path::Path;

    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_open_v2(filename: *const u8, db: *mut *mut c_void, flags: i32, vfs: *const u8) -> i32);
    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_busy_timeout(db: *mut c_void, ms: i32) -> i32);
    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_prepare_v2(db: *mut c_void, sql: *const u8, bytes: i32, stmt: *mut *mut c_void, tail: *mut *const u8) -> i32);
    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_step(stmt: *mut c_void) -> i32);
    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_column_count(stmt: *mut c_void) -> i32);
    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_column_text(stmt: *mut c_void, column: i32) -> *const u8);
    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_finalize(stmt: *mut c_void) -> i32);
    windows_link::link!("winsqlite3.dll" "system" fn sqlite3_close(db: *mut c_void) -> i32);

    const READ_ONLY: i32 = 0x01;
    const ROW: i32 = 100;
    /// More rows than any query here asks for.
    const MAX_ROWS: usize = 64;

    /// Every row of `sql` on the database at `path`, each column as text (NULL as ""), or
    /// `None` when the database cannot be opened or the query fails.
    pub fn query(path: &Path, sql: &str) -> Option<Vec<Vec<String>>> {
        let name = format!("{}\0", path.to_str()?);
        let sql = format!("{sql}\0");
        let mut db = std::ptr::null_mut();
        // SAFETY: both strings are NUL-terminated and outlive the calls; the handles are closed
        // below on every path once opened.
        unsafe {
            if sqlite3_open_v2(name.as_ptr(), &mut db, READ_ONLY, std::ptr::null()) != 0 {
                if !db.is_null() {
                    sqlite3_close(db);
                }
                return None;
            }
            // omp may be writing a snapshot; wait a moment rather than fail.
            sqlite3_busy_timeout(db, 200);
            let mut stmt = std::ptr::null_mut();
            if sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) != 0 {
                sqlite3_close(db);
                return None;
            }
            let columns = sqlite3_column_count(stmt);
            let mut rows = Vec::new();
            while rows.len() < MAX_ROWS && sqlite3_step(stmt) == ROW {
                let row = (0..columns)
                    .map(|i| {
                        let text = sqlite3_column_text(stmt, i);
                        if text.is_null() { String::new() } else { CStr::from_ptr(text.cast()).to_string_lossy().into_owned() }
                    })
                    .collect();
                rows.push(row);
            }
            sqlite3_finalize(stmt);
            sqlite3_close(db);
            Some(rows)
        }
    }
}

fn codex(sessions: &Path, now: u64) -> Option<Limits> {
    let newest: PathBuf = latest_days(sessions)
        .iter()
        .flat_map(|day| fs::read_dir(day).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .filter_map(|path| mtime(&path).map(|modified| (modified, path)))
        .max_by_key(|(modified, _)| *modified)
        .filter(|(modified, _)| now.saturating_sub(*modified) < STALE)?
        .1;
    let tail = read_tail(&newest, TAIL)?;
    // The account-wide "codex" limit; other ids (e.g. "premium") carry no windows.
    let line = tail.lines().rev().find(|line| line.contains("\"limit_id\":\"codex\"") && line.contains("\"primary\":{"))?;
    let limits = object(line)?.GetNamedObject(&HSTRING::from("payload")).ok()?.GetNamedObject(&HSTRING::from("rate_limits")).ok()?;
    let meters = sorted(["primary", "secondary"].iter().filter_map(|key| meter(&limits, key, now)).collect());
    (!meters.is_empty()).then_some(Limits { kind: Kind::Codex, meters })
}

/// One window from `rate_limits.<key>`; a window that already reset is dropped, since its new
/// usage is not known until the agent runs again.
fn meter(limits: &JsonObject, key: &str, now: u64) -> Option<Meter> {
    let window = limits.GetNamedObject(&HSTRING::from(key)).ok()?;
    let used = number_field(&window, "used_percent")?;
    let minutes = number_field(&window, "window_minutes")? as u64;
    let resets_at = number_field(&window, "resets_at")? as u64;
    (resets_at > now).then(|| Meter { window: window_name(minutes), used: used.clamp(0.0, 100.0).round() as u8, resets_at })
}

/// Most used first, so the first meter is the one that blocks.
fn sorted(mut meters: Vec<Meter>) -> Vec<Meter> {
    meters.sort_by_key(|meter| std::cmp::Reverse(meter.used));
    meters
}

fn window_name(minutes: u64) -> String {
    match minutes {
        10_080 => "Week".to_string(),
        m if m >= 1_440 && m % 1_440 == 0 => format!("{}d", m / 1_440),
        m if m >= 60 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

/// Time until `resets_at` in one short unit, as narrow as a percentage: "45m", "2h", "3d".
pub fn until(resets_at: u64, now: u64) -> String {
    let secs = resets_at.saturating_sub(now);
    match secs {
        0..=3_599 => format!("{}m", secs.div_ceil(60).max(1)),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_omp_windows() {
        assert_eq!(omp_window("commandcode:5h", "5-hour limit"), "5h");
        assert_eq!(omp_window("commandcode:7d", "Weekly"), "Week");
        assert_eq!(omp_window("provider:monthly", "Weekly"), "Week");
        assert_eq!(omp_window("odd", "Monthly cap"), "Monthl");
    }

    /// Against omp's real database on this PC: `cargo test omp_database -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn omp_database() {
        let home = PathBuf::from(std::env::var("USERPROFILE").expect("USERPROFILE"));
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        println!("{:?}", omp(&home.join(".omp").join("agent").join(OMP_DB), now));
    }

    #[test]
    fn reads_claude_windows() {
        let limits = claude("5h 23 2000\nWeek 81 3000\nbad line\n", 1_000).unwrap();
        assert_eq!(limits.kind, Kind::Claude);
        assert_eq!(limits.tightest().unwrap().window, "Week");
        assert!(claude("5h 23 900\n", 1_000).is_none(), "a reset window is dropped");
    }

    #[test]
    fn relay_keeps_only_the_windows() {
        let payload = r#"{"model":{"id":"x"},"cwd":"D:\\secret","rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":2000},"seven_day":{"used_percentage":41.2,"resets_at":3000}}}"#;
        assert_eq!(claude_lines(payload).as_deref(), Some("5h 24 2000\nWeek 41 3000\n"));
        assert_eq!(claude_lines(r#"{"model":{"id":"x"}}"#), None, "no limits for API-key sessions");
    }

    #[test]
    fn reads_codex_rollout_line() {
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":100.0,"window_minutes":300,"resets_at":2000},"secondary":{"used_percent":18.0,"window_minutes":10080,"resets_at":900}}}}"#;
        let limits = object(line).unwrap().GetNamedObject(&HSTRING::from("payload")).unwrap().GetNamedObject(&HSTRING::from("rate_limits")).unwrap();
        let five = meter(&limits, "primary", 1_000).unwrap();
        assert_eq!((five.window.as_str(), five.used), ("5h", 100));
        assert!(meter(&limits, "secondary", 1_000).is_none(), "a reset window is dropped");
    }

    #[test]
    fn names_windows() {
        assert_eq!(window_name(300), "5h");
        assert_eq!(window_name(10_080), "Week");
        assert_eq!(window_name(2_880), "2d");
        assert_eq!(window_name(45), "45m");
    }

    #[test]
    fn short_reset_times() {
        assert_eq!(until(1_000, 1_000), "1m");
        assert_eq!(until(1_000 + 45 * 60, 1_000), "45m");
        assert_eq!(until(2 * 3_600 + 14 * 60, 0), "2h");
        assert_eq!(until(3 * 3_600, 0), "3h");
        assert_eq!(until(3 * 86_400 + 5, 0), "3d");
    }

    #[test]
    fn tightest_window_first() {
        let meters = sorted(vec![
            Meter { window: "5h".into(), used: 12, resets_at: 1 },
            Meter { window: "Week".into(), used: 80, resets_at: 2 },
        ]);
        assert_eq!(meters[0].window, "Week");
    }
}
