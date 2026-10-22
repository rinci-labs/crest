//! Local storage as plain text files in `%APPDATA%\Crest`. They can be read and edited by
//! hand in Notepad; nothing is sent anywhere.

use std::path::PathBuf;

/// Crest data folder: `%APPDATA%\Crest`.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("Crest"))
}

/// Lines of file `name`; empty if the file does not exist yet.
pub fn load(name: &str) -> Vec<String> {
    dir()
        .and_then(|dir| std::fs::read_to_string(dir.join(name)).ok())
        .map(|text| text.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Rewrite file `name`. Writes to a temporary file first and then swaps it in, so the old file
/// is not corrupted if the machine dies mid-write.
pub fn save(name: &str, lines: impl IntoIterator<Item = String>) {
    let Some(dir) = dir() else { return };
    let mut text = String::new();
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    let temp = dir.join(format!("{name}.tmp"));
    let result = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&temp, text))
        .and_then(|_| std::fs::rename(&temp, dir.join(name)));
    if let Err(e) = result {
        error_log!("failed to save {name}: {e}");
    }
}
