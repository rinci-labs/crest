//! Local error log at `%APPDATA%\Crest\log.txt`, to track down why Crest failed or closed on
//! its own. Written only to this machine and never sent anywhere. It records only the kind of
//! failure, never message contents, song titles, file names, or folder names.

use std::fmt::Display;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const FILE: &str = "log.txt";
const OLD: &str = "log-old.txt";
/// Past this size the log is moved to `log-old.txt` and started fresh.
const MAX_BYTES: u64 = 256 * 1024;

/// Last message: a failure that repeats every frame only needs logging once.
static LAST: Mutex<Option<String>> = Mutex::new(None);

/// Log folder (same as the Crest data folder).
pub fn folder() -> Option<PathBuf> {
    crate::shell::store::dir()
}

pub fn write(message: impl Display) {
    let message = message.to_string();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_deref() == Some(message.as_str()) {
        return;
    }
    let Some(dir) = folder() else { return };
    let path = dir.join(FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, dir.join(OLD));
    }
    let t = crate::system::clock::now();
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {message}\r\n",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    let written = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(&path))
        .and_then(|mut file| file.write_all(line.as_bytes()));
    if written.is_ok() {
        *last = Some(message);
    }
}

/// Bugs (panics) are logged before the process exits, including their location in the code.
pub fn catch_panics() {
    std::panic::set_hook(Box::new(|info| {
        write(format_args!("program error: {info}"));
        if cfg!(debug_assertions) {
            eprintln!("{info}");
        }
    }));
}
