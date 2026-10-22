//! Small, bounded file and text helpers shared by the agent readers.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Data::Json::JsonObject;
use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::core::HSTRING;

/// Collapse whitespace into single spaces and cut at `max` characters with an ellipsis.
pub fn clean(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.chars().count() <= max {
        return out;
    }
    let mut cut: String = out.chars().take(max - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// Last folder name of a Windows or POSIX path.
pub fn folder(path: &str) -> String {
    path.trim().rsplit(['\\', '/']).find(|part| !part.is_empty()).unwrap_or_default().to_string()
}

/// Epoch seconds now.
pub fn now() -> u64 {
    epoch(SystemTime::now())
}

fn epoch(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// File modified time in epoch seconds.
pub fn mtime(path: &Path) -> Option<u64> {
    fs::metadata(path).ok()?.modified().ok().map(epoch)
}

/// At most the first `limit` bytes as text (invalid UTF-8 is replaced).
pub fn read_head(path: &Path, limit: usize) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path).ok()?.take(limit as u64).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The last `limit` bytes as whole lines; a line cut by the window start is dropped.
pub fn read_tail(path: &Path, limit: u64) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(limit);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Some(text);
    }
    Some(text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default())
}

/// A whole file, only if it is a regular file no larger than `limit` bytes.
pub fn read_small(path: &Path, limit: u64) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > limit {
        return None;
    }
    Some(String::from_utf8_lossy(&fs::read(path).ok()?).into_owned())
}

/// The process exists and has not exited. Unopenable processes count as gone.
pub fn alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let read = GetExitCodeProcess(process, &mut code).is_ok();
        let _ = CloseHandle(process);
        read && code as i32 == STILL_ACTIVE.0
    }
}

/// Lowercase image names of every running process, read once per scan.
pub struct Processes(Vec<String>);

impl Processes {
    pub fn snapshot() -> Self {
        let mut names = Vec::new();
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return Self(names);
            };
            let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
            while more {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                names.push(String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase());
                more = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
        Self(names)
    }

    pub fn has(&self, image: &str) -> bool {
        self.0.iter().any(|name| name == image)
    }
}

pub fn object(text: &str) -> Option<JsonObject> {
    JsonObject::Parse(&HSTRING::from(text)).ok()
}

pub fn text_field(root: &JsonObject, key: &str) -> String {
    root.GetNamedString(&HSTRING::from(key)).map(|value| value.to_string()).unwrap_or_default()
}

pub fn number_field(root: &JsonObject, key: &str) -> Option<f64> {
    root.GetNamedNumber(&HSTRING::from(key)).ok()
}

/// Value of the first `"key":"…"` string in raw JSON text, without parsing the whole line.
/// Used on huge transcript lines where only one early field is needed.
pub fn json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let mut out = String::new();
    let mut chars = text[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let code: String = chars.by_ref().take(4).collect();
                    out.push(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32).unwrap_or('?'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// `key: value` pairs from a small YAML-like header. Block values (`|` or `>`) are joined from
/// the indented lines below them; quotes and simple escapes are removed.
pub fn yaml_fields(text: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut block: Option<(String, Vec<String>)> = None;
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, parts)) = block.as_mut()
                && !line.trim().is_empty()
            {
                parts.push(line.trim().to_string());
            }
            continue;
        }
        if let Some((key, parts)) = block.take() {
            fields.push((key, parts.join(" ")));
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        if key.is_empty() {
            continue;
        }
        if value.starts_with('|') || value.starts_with('>') {
            block = Some((key, Vec::new()));
        } else {
            fields.push((key, unquote(value)));
        }
    }
    if let Some((key, parts)) = block {
        fields.push((key, parts.join(" ")));
    }
    fields
}

pub fn field(fields: &[(String, String)], name: &str) -> String {
    fields.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()).unwrap_or_default()
}

fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|text| text.strip_suffix('"')) else {
        return value.to_string();
    };
    json_string(&format!("\"v\":\"{inner}\""), "v").unwrap_or_else(|| inner.to_string())
}

/// ISO-8601 UTC (`2026-10-18T08:46:02.370Z`) to epoch seconds; fractions are ignored.
pub fn parse_iso(text: &str) -> Option<u64> {
    let bytes = text.trim().as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    if bytes[10] != b'T' && bytes[10] != b' ' {
        return None;
    }
    let number = |from: usize, len: usize| -> Option<i64> {
        let slice = bytes.get(from..from + len)?;
        slice.iter().all(u8::is_ascii_digit).then(|| std::str::from_utf8(slice).ok()?.parse().ok())?
    };
    let (year, month, day) = (number(0, 4)?, number(5, 2)?, number(8, 2)?);
    let (hour, minute, second) = (number(11, 2)?, number(14, 2)?, number(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    u64::try_from(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_text() {
        assert_eq!(clean("  two   line\nbreak  ", 40), "two line break");
        assert_eq!(clean(" \t\r\n ", 8), "");
        assert_eq!(clean("abcdefghij", 5), "abcd…");
        assert_eq!(clean("日本語テキスト", 4), "日本語…");
        assert_eq!(clean("日本語テキスト", 0), "");
    }

    #[test]
    fn parses_iso_time() {
        assert_eq!(parse_iso("2026-10-18T08:46:02.370Z"), Some(1_792_313_162));
        assert_eq!(parse_iso("2026-10-18 08:46:02"), Some(1_792_313_162));
        assert_eq!(parse_iso("not a time"), None);
    }

    #[test]
    fn reads_json_strings_without_parsing() {
        let line = r#"{"type":"session_meta","payload":{"id":"x","cwd":"D:\\work\\crest","note":"a \"b\" \u00e9"}"#;
        assert_eq!(json_string(line, "cwd").as_deref(), Some(r"D:\work\crest"));
        assert_eq!(json_string(line, "note").as_deref(), Some("a \"b\" é"));
        assert_eq!(json_string(line, "missing"), None);
        assert_eq!(json_string(r#"{"cut":"no end"#, "cut"), None);
    }

    #[test]
    fn reads_yaml_fields() {
        let text = "id: 1\ncwd: \"d:\\\\work\\\\api\"\nsummary: |\n  first line\n  second\nbranch: main\n";
        let fields = yaml_fields(text);
        assert_eq!(field(&fields, "cwd"), r"d:\work\api");
        assert_eq!(field(&fields, "summary"), "first line second");
        assert_eq!(field(&fields, "branch"), "main");
        assert_eq!(folder(r"D:\work\acme\"), "acme");
    }

    #[test]
    fn sees_this_process() {
        assert!(alive(std::process::id()));
        assert!(!alive(0));
        assert!(!Processes::snapshot().0.is_empty());
    }
}
