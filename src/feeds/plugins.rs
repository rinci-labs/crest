//! Plugins: small local commands whose output becomes rows on the Plugins page.
//!
//! One file per plugin in `%APPDATA%\Crest\plugins`, named `<name>.plugin`, plain text:
//!
//! ```text
//! command = powershell -NoProfile -File "%APPDATA%\Crest\plugins\gmail.ps1"
//! every = 15
//! ```
//!
//! The command prints rows, one per line:
//!
//! ```text
//! Inbox 3 | unread from Ana | https://mail.google.com
//! CI green | all jobs passed
//! ```
//!
//! `title | detail | url`: only the title is required, and a row with a URL opens in the
//! browser when clicked. Output is read while the Plugins page is open, at most every `every`
//! minutes (minimum 1, default 15), and nothing runs while the feature is off. Crest never
//! passes data to the command; it may use the internet on its own if the user wrote it that way.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::shell::store;
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, BELOW_NORMAL_PRIORITY_CLASS};

/// Folder inside `%APPDATA%\Crest`.
const FOLDER: &str = "plugins";
/// Rows kept per plugin, and the longest row fields.
const MAX_ROWS: usize = 6;
const MAX_TITLE: usize = 60;
const MAX_DETAIL: usize = 90;
/// A definition file is a few lines; anything past this is not read. The file sits in the user's
/// own folder, but nothing stops a stray copy of something large from being saved as `.plugin`.
const MAX_DEFINITION: u64 = 64 * 1024;
/// How long the output reader may take after the process is gone.
const READER_GRACE: Duration = Duration::from_secs(5);
/// Output beyond this is dropped: a plugin is a widget, not a log.
const MAX_OUTPUT: usize = 16 * 1024;
/// A plugin that has not finished by then is killed.
const TIMEOUT_SECS: u64 = 20;
/// Default and shortest interval between runs.
const DEFAULT_MINUTES: u32 = 15;

/// One plugin definition, as read from its file.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    /// File stem, e.g. "gmail".
    pub name: String,
    pub command: String,
    /// Minutes between runs.
    pub every: u32,
}

#[allow(unused_imports)]
use std::os::windows::process::CommandExt as _;

impl Plugin {
    /// Read `<name>.plugin`; `None` when it has no command.
    pub fn parse(name: &str, text: &str) -> Option<Self> {
        let mut command = None;
        let mut every = DEFAULT_MINUTES;
        for line in text.lines() {
            let line = line.trim();
            // Comments and blank lines are ignored, so a plugin file can explain itself.
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim().to_lowercase().as_str() {
                "command" | "exec" => command = Some(value.trim_matches('"').to_string()).filter(|c| !c.is_empty()),
                "every" | "minutes" => every = value.parse::<u32>().unwrap_or(DEFAULT_MINUTES).max(1),
                _ => {}
            }
        }
        Some(Self { name: clean_name(name), command: command?, every })
    }

    /// Commands in the plugins folder, in file name order.
    pub fn load_all() -> Vec<Self> {
        let Some(dir) = folder() else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut plugins: Vec<Self> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("plugin")))
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_stem()?.to_string_lossy().into_owned();
                let text = read_definition(&path)?;
                Self::parse(&name, &text)
            })
            .collect();
        plugins.sort_by_key(|plugin| plugin.name.to_lowercase());
        plugins
    }
}

/// A row a plugin printed.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub plugin: String,
    pub title: String,
    pub detail: String,
    /// Opened in the browser when the row is clicked; may be empty.
    pub url: String,
}

/// What one plugin printed, or why it did not.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub rows: Vec<Row>,
    /// Set when the plugin failed: the reason, shown in place of its rows.
    pub error: Option<String>,
    /// When it last ran (seconds since the app started), for the "run again" rule.
    pub ran: Option<f64>,
}

/// Parse a plugin's output into rows. Blank lines are skipped; unrecognised text is a title.
pub fn parse_rows(plugin: &str, output: &str) -> Vec<Row> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut parts = line.split('|').map(str::trim);
            let title = clean(parts.next()?, MAX_TITLE);
            if title.is_empty() {
                return None;
            }
            let detail = parts.next().map(|d| clean(d, MAX_DETAIL)).unwrap_or_default();
            let url = parts.next().map(str::to_string).filter(|u| is_url(u)).unwrap_or_default();
            Some(Row { plugin: clean_name(plugin), title, detail, url })
        })
        .take(MAX_ROWS)
        .collect()
}

/// Plugin rows, keyed by plugin name, plus the order the plugins are shown in.
#[derive(Default)]
pub struct Plugins {
    pub list: Vec<Plugin>,
    pub output: HashMap<String, Output>,
}

impl Plugins {
    /// The plugins in the folder, or nothing when the feature is off.
    pub fn load(on: bool) -> Self {
        Self { list: if on { Plugin::load_all() } else { Vec::new() }, output: HashMap::new() }
    }

    /// Plugins due for a run, with the interval their own file asks for.
    pub fn due(&self, now: f64) -> Vec<Plugin> {
        self.list
            .iter()
            .filter(|plugin| match self.output.get(&plugin.name).and_then(|o| o.ran) {
                Some(ran) => now - ran >= plugin.every as f64 * 60.0,
                None => true,
            })
            .cloned()
            .collect()
    }

    /// All rows in display order: one row per plugin, plugins with more rows showing one each.
    /// Kept small on purpose; the page shows what fits.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut depth = 0;
        loop {
            let mut added = false;
            for plugin in &self.list {
                if let Some(row) = self.output.get(&plugin.name).and_then(|o| o.rows.get(depth)) {
                    rows.push(row.clone());
                    added = true;
                }
            }
            depth += 1;
            if !added || rows.len() >= MAX_ROWS * 3 {
                break;
            }
        }
        rows
    }

    /// Errors as rows, so a failing plugin explains itself in place.
    pub fn problems(&self) -> Vec<Row> {
        self.list
            .iter()
            .filter_map(|plugin| {
                let error = self.output.get(&plugin.name)?.error.as_ref()?;
                Some(Row { plugin: plugin.name.clone(), title: clean(error, MAX_TITLE), detail: plugin.command.clone(), url: String::new() })
            })
            .collect()
    }
}

/// Run `plugin` on a worker thread; the result arrives through `done`.
pub fn run(plugin: Plugin, done: impl FnOnce(Output) + Send + 'static) {
    let spawned = std::thread::Builder::new().name(format!("plugin {}", plugin.name)).spawn(move || {
        done(execute(&plugin));
    });
    if let Err(e) = spawned {
        error_log!("failed to run plugin: {e}");
    }
}

/// Read `<name>.plugin`, but not more than [`MAX_DEFINITION`]; `None` when it cannot be opened.
/// The cut may land inside a multi-byte character, so the bytes are read lossily: a truncated
/// tail must not make a valid definition look unreadable.
fn read_definition(path: &std::path::Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_DEFINITION).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Run the plugin's command and parse what it printed.
fn execute(plugin: &Plugin) -> Output {
    let mut command = Command::new("cmd");
    command
        .args(["/C", &plugin.command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // The rows are stdout only, so stderr is not piped: a plugin that writes a lot to it
        // would otherwise fill the pipe and block until the timeout kills it.
        .stderr(Stdio::null())
        // No console window may flash on screen for a background plugin.
        .creation_flags(CREATE_NO_WINDOW.0 | BELOW_NORMAL_PRIORITY_CLASS.0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return Output { error: Some(format!("cannot start: {e}")), ..Output::default() },
    };
    // Output is read on its own thread so a plugin that never closes its pipe still times out.
    // The buffer is handed back over a channel, so the wait below is bounded too: a grandchild
    // that inherited the pipe cannot keep this thread parked forever.
    let (sender, receiver) = std::sync::mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::thread::Builder::new().name("plugin output".into()).spawn(move || {
            let mut buffer = Vec::new();
            let _ = stdout.by_ref().take(MAX_OUTPUT as u64).read_to_end(&mut buffer);
            let _ = sender.send(buffer);
        });
    }
    let deadline = Instant::now() + Duration::from_secs(TIMEOUT_SECS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Output { error: Some("took too long".to_string()), ..Output::default() };
            }
            Err(e) => return Output { error: Some(format!("failed: {e}")), ..Output::default() },
        }
    };
    let text = receiver
        .recv_timeout(READER_GRACE)
        .map(|buffer| String::from_utf8_lossy(&buffer).into_owned())
        .unwrap_or_default();
    if !status.success() && text.trim().is_empty() {
        let code = status.code().map_or_else(|| "no exit code".to_string(), |c| format!("exit code {c}"));
        return Output { error: Some(format!("failed, {code}")), ..Output::default() };
    }
    let rows = parse_rows(&plugin.name, &text);
    if rows.is_empty() {
        return Output { error: Some("printed nothing".to_string()), ..Output::default() };
    }
    Output { rows, error: None, ran: None }
}

/// Folder holding the plugin files; created on first use so the path can be shown to the user.
pub fn folder() -> Option<PathBuf> {
    let dir = store::dir()?.join(FOLDER);
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

/// Open the plugins folder, or create it first.
pub fn open_folder(hwnd: windows::Win32::Foundation::HWND) {
    if let Some(dir) = folder() {
        crate::system::dnd::open(hwnd, &dir);
    }
}

/// Write a commented example plugin, once, so the folder explains the format.
pub fn write_example() {
    let Some(dir) = folder() else { return };
    let path = dir.join("example.plugin");
    if path.exists() {
        return;
    }
    let text = "\
# A Crest plugin: one file per plugin, saved as <name>.plugin in this folder.
# Turn plugins on in the tray menu (Settings > Plugins) or on the Plugins page.
#
# command runs while the Plugins page is open; every = minutes between runs.
# Print rows, one per line:  title | detail | url
# The detail and the url are optional; a row with a url opens in the browser.
#
# command = echo Hello | Crest plugins are on | https://github.com/rinci-labs/crest
# every = 15
";
    let _ = std::fs::write(path, text);
}

/// A row is worth showing only when its title has letters or digits.
fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(max).collect()
}

fn clean_name(name: &str) -> String {
    clean(name, 24)
}

/// Only http(s) links are opened, so a plugin cannot launch a local program this way.
fn is_url(text: &str) -> bool {
    text.starts_with("https://") || text.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_plugin_file() {
        let plugin = Plugin::parse("gmail", "# my inbox\ncommand = \"C:\\tools\\gmail.exe --unread\"\nevery = 5\n").unwrap();
        assert_eq!(plugin.name, "gmail");
        assert_eq!(plugin.command, "C:\\tools\\gmail.exe --unread");
        assert_eq!(plugin.every, 5);
        // No command means it is not a plugin.
        assert!(Plugin::parse("empty", "# nothing\n").is_none());
        // A missing interval falls back to the default.
        assert_eq!(Plugin::parse("x", "command=echo hi").unwrap().every, DEFAULT_MINUTES);
    }

    #[test]
    fn parses_output_rows() {
        let rows = parse_rows("inbox", "Inbox 3 | unread from Ana | https://mail.google.com\n\nCI green\n# comment\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], Row {
            plugin: "inbox".into(),
            title: "Inbox 3".into(),
            detail: "unread from Ana".into(),
            url: "https://mail.google.com".into()
        });
        assert_eq!(rows[1].detail, "");
        assert_eq!(rows[1].url, "");
        // Only web links are kept.
        assert_eq!(parse_rows("x", "Open | | file:///C:/Windows")[0].url, "");
        // Output is capped so a chatty plugin cannot flood the page.
        let many: String = (0..20).map(|i| format!("row {i}\n")).collect();
        assert_eq!(parse_rows("x", &many).len(), MAX_ROWS);
    }

    #[test]
    fn rows_are_interleaved_and_failures_are_reported() {
        let mut plugins = Plugins {
            list: vec![
                Plugin { name: "a".into(), command: "x".into(), every: 15 },
                Plugin { name: "b".into(), command: "y".into(), every: 15 },
            ],
            output: HashMap::new(),
        };
        assert_eq!(plugins.due(0.0).len(), 2, "everything runs the first time");
        plugins.output.insert(
            "a".into(),
            Output {
                rows: parse_rows("a", "a1\na2"),
                error: None,
                ran: Some(10.0),
            },
        );
        plugins.output.insert("b".into(), Output { rows: parse_rows("b", "b1"), error: None, ran: Some(10.0) });
        let rows = plugins.rows();
        assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["a1", "b1", "a2"]);
        // "a" ran 10 seconds ago, so nothing is due yet.
        assert!(plugins.due(20.0).is_empty());
        // After its interval it is due again.
        assert_eq!(plugins.due(10.0 + 15.0 * 60.0).len(), 2);

        plugins.output.insert("b".into(), Output { rows: Vec::new(), error: Some("printed nothing".into()), ran: Some(1.0) });
        assert_eq!(plugins.problems().len(), 1);
        assert_eq!(plugins.problems()[0].title, "printed nothing");
    }

    #[test]
    fn moves_pending_output_into_rows_once() {
        // The table keeps the last result only; a second result replaces the first.
        let mut plugins = Plugins { list: vec![Plugin { name: "p".into(), command: "x".into(), every: 1 }], output: HashMap::new() };
        plugins.output.insert("p".into(), Output { rows: parse_rows("p", "first"), error: None, ran: Some(0.0) });
        assert_eq!(plugins.rows()[0].title, "first");
        plugins.output.insert("p".into(), Output { rows: parse_rows("p", "second"), error: None, ran: Some(60.0) });
        assert_eq!(plugins.rows()[0].title, "second");
    }
}

