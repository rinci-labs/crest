//! Coding agents working on this PC: Claude Code, Codex, Cursor, GitHub Copilot CLI, and omp.
//!
//! Each reader looks only at small files the agent already writes in the user folder: session
//! records, the head or tail of a transcript, and file times. No databases, no full
//! conversations, no network. Rows backed by a pid must have a live process; readers without a
//! pid limit themselves to recently written sessions.
//!
//! [`Listener`] watches those folders and rescans on change, so the page and notices follow
//! the agents within a fraction of a second.

mod claude;
mod codex;
mod copilot;
mod cursor;
mod hooks;
mod listener;
mod omp;
mod util;
mod window;

use std::path::PathBuf;

pub use listener::Listener;
pub use window::focus;
pub use hooks::{install as install_hooks, install_provider, installed_hooks, record as record_hook};

/// A provider whose hook configuration Crest can write. Third-party tools install their own
/// one-line hook instead (see [`hooks`]), so they are not listed here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Claude,
    Cursor,
    Copilot,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Claude, Provider::Cursor, Provider::Copilot];

    /// The agent this provider installs a hook for.
    pub fn kind(self) -> Kind {
        match self {
            Provider::Claude => Kind::Claude,
            Provider::Cursor => Kind::Cursor,
            Provider::Copilot => Kind::Copilot,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Cursor => "Cursor",
            Provider::Copilot => "Copilot",
        }
    }
}

/// Optional lifecycle relays already present in provider configuration.
#[derive(Clone, Copy, Default)]
pub struct HookStatus {
    pub claude: bool,
    pub cursor: bool,
    pub copilot: bool,
}

impl HookStatus {
    /// Is the hook for every provider listed on the Settings page installed?
    pub fn all(self) -> bool {
        Provider::ALL.iter().all(|provider| self.get(&provider.kind()))
    }

    pub fn get(self, kind: &Kind) -> bool {
        match kind {
            Kind::Claude => self.claude,
            Kind::Cursor => self.cursor,
            Kind::Copilot => self.copilot,
            Kind::Codex | Kind::Omp => true,
            // A tool that writes its own hook is, by definition, already connected.
            Kind::Other(_) => true,
        }
    }

}

/// Row text limits.
const TITLE_MAX: usize = 48;
const PROJECT_MAX: usize = 28;
const DETAIL_MAX: usize = 40;
/// Rows kept after sorting.
const MAX_ROWS: usize = 12;
/// Rows that fit on the Agents page at once; the rest scroll with the mouse wheel.
pub const VISIBLE: usize = 2;
/// Session records are tiny; anything bigger is not one.
const MAX_JSON: u64 = 16 * 1024;
/// A finished session reads as "Done" for this long, then "Idle".
const DONE_FOR: u64 = 10 * 60;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Claude,
    Codex,
    Cursor,
    Copilot,
    Omp,
    /// Any other tool that reports through a hook and names itself (see [`hooks`]). The name is
    /// the lowercase id the tool sends, e.g. "aider" or "gemini".
    Other(String),
}

impl Kind {
    pub fn label(&self) -> &str {
        match self {
            Kind::Claude => "Claude",
            Kind::Codex => "Codex",
            Kind::Cursor => "Cursor",
            Kind::Copilot => "Copilot",
            Kind::Omp => "omp",
            Kind::Other(name) => name,
        }
    }

    /// Brand-neutral tint for the agent name on the Agents page. Other tools get a colour
    /// from their name, so each tool keeps the same tint between runs.
    pub fn tint(&self) -> u32 {
        match self {
            Kind::Claude => 0xE8A07A,
            Kind::Codex => 0x9CA3AF,
            Kind::Cursor => 0x7DD3FC,
            Kind::Copilot => 0xA78BFA,
            Kind::Omp => 0x86EFAC,
            Kind::Other(name) => {
                // The same handful of calm tints the built-in agents use.
                const TINTS: [u32; 6] = [0xFB923C, 0x34D399, 0x60A5FA, 0xF472B6, 0xFBBF24, 0x8B7CFF];
                let seed = name.bytes().fold(0u32, |sum, b| sum.wrapping_mul(31).wrapping_add(b as u32));
                TINTS[(seed % TINTS.len() as u32) as usize]
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Needs the user: a permission prompt or a question.
    Waiting,
    Running,
    /// Finished a turn in the last few minutes.
    Done,
    Idle,
}

impl State {
    /// Sort order: what needs attention first.
    fn rank(self) -> u8 {
        match self {
            State::Waiting => 0,
            State::Running => 1,
            State::Done => 2,
            State::Idle => 3,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Session {
    /// Stable per session (pid or session id), used to spot state changes between scans.
    pub id: String,
    pub kind: Kind,
    pub title: String,
    /// Folder name the agent works in.
    pub project: String,
    pub state: State,
    /// Model, waiting reason, surface, or running command; may be empty.
    pub detail: String,
    /// Seconds since the last activity.
    pub age_secs: u64,
    /// Agent process, whose window a click brings forward; 0 when unknown.
    pub pid: u32,
    /// Full path of the folder the agent works in, for "open project"; empty when unknown.
    pub folder: String,
}

impl Session {
    /// Same row on screen. Age only counts per minute, otherwise every scan would differ.
    fn looks_same(&self, other: &Session) -> bool {
        self.id == other.id
            && self.title == other.title
            && self.project == other.project
            && self.state == other.state
            && self.detail == other.detail
            && self.age_secs / 60 == other.age_secs / 60
    }
}

/// A state change worth a notice.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Change {
    NeedsInput(Session),
    Finished(Session),
}

/// Changes from `before` to `after`: a session that starts waiting, or one that was running
/// and is now done.
pub fn changes(before: &[Session], after: &[Session]) -> Vec<Change> {
    after
        .iter()
        .filter_map(|now| {
            let was = before.iter().find(|old| old.id == now.id).map(|old| old.state);
            match (was, now.state) {
                (Some(State::Waiting), State::Waiting) => None,
                (_, State::Waiting) => Some(Change::NeedsInput(now.clone())),
                (Some(State::Running), State::Done) => Some(Change::Finished(now.clone())),
                _ => None,
            }
        })
        .collect()
}

/// Rescans every reader and remembers the last rows.
#[derive(Default)]
struct Scanner {
    last: Option<Vec<Session>>,
}

impl Scanner {
    /// Fresh rows, or `None` when nothing visible changed.
    fn scan(&mut self) -> Option<Vec<Session>> {
        let mut rows = collect();
        rows.sort_by_key(|row| (row.state.rank(), row.age_secs));
        rows.truncate(MAX_ROWS);
        let same = self.last.as_ref().is_some_and(|last| {
            last.len() == rows.len() && last.iter().zip(&rows).all(|(old, new)| old.looks_same(new))
        });
        if same {
            return None;
        }
        self.last = Some(rows.clone());
        Some(rows)
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

fn collect() -> Vec<Session> {
    let Some(home) = home() else { return Vec::new() };
    let now = util::now();
    let processes = util::Processes::snapshot();
    let mut rows = claude::sessions(&home, now);
    rows.extend(codex::sessions(&home, now, &processes));
    rows.extend(cursor::sessions(&home, now));
    rows.extend(copilot::sessions(&home, now));
    rows.extend(omp::sessions(&home, now));
    let hooked = hooks::sessions(now);
    rows.retain(|row| {
        !hooked.iter().any(|hook| hook.kind == row.kind && !hook.project.is_empty() && hook.project == row.project)
    });
    rows.extend(hooked);
    rows
}

/// Folders whose changes should trigger a rescan, with whether to watch their subtree.
fn watched() -> Vec<(PathBuf, bool)> {
    let Some(home) = home() else { return Vec::new() };
    let mut paths = vec![
        (home.join(".claude").join("sessions"), false),
        (home.join(".claude").join("projects"), true),
        (home.join(".codex").join("sessions"), true),
        (home.join(".cursor").join("projects"), true),
        (home.join(".copilot").join("session-state"), true),
        (home.join(".omp").join("run"), true),
    ];
    if let Some(path) = hooks::records_dir() {
        paths.push((path, false));
    }
    paths.into_iter().filter(|(path, _)| path.is_dir()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, state: State) -> Session {
        Session {
            id: id.into(),
            kind: Kind::Claude,
            title: "t".into(),
            project: "p".into(),
            state,
            detail: String::new(),
            age_secs: 5,
            pid: 0,
            folder: String::new(),
        }
    }

    #[test]
    fn spots_changes_worth_a_notice() {
        let before = [row("a", State::Running), row("b", State::Waiting), row("c", State::Idle)];
        let after = [row("a", State::Done), row("b", State::Waiting), row("c", State::Waiting), row("d", State::Waiting)];
        let found = changes(&before, &after);
        assert_eq!(found.len(), 3);
        assert!(matches!(&found[0], Change::Finished(s) if s.id == "a"));
        assert!(matches!(&found[1], Change::NeedsInput(s) if s.id == "c"));
        assert!(matches!(&found[2], Change::NeedsInput(s) if s.id == "d"));
        assert!(changes(&after, &after).is_empty());
    }

    #[test]
    fn unchanged_rows_skip_the_redraw() {
        let mut first = row("a", State::Running);
        let mut second = first.clone();
        second.age_secs += 30;
        assert!(first.looks_same(&second), "age within the same minute");
        second.age_secs += 60;
        assert!(!first.looks_same(&second), "age moved to the next minute");
        first.state = State::Waiting;
        assert!(!first.looks_same(&row("a", State::Running)));
    }

    /// Real scan on this machine: only invariants that always hold.
    #[test]
    fn live_scan_is_consistent() {
        let mut scanner = Scanner::default();
        let Some(rows) = scanner.scan() else { panic!("the first scan always returns rows") };
        for row in &rows {
            assert!(!row.id.is_empty() && !row.title.is_empty());
            assert!(row.title.chars().count() <= TITLE_MAX && row.project.chars().count() <= PROJECT_MAX);
        }
        assert!(rows.windows(2).all(|pair| pair[0].state.rank() <= pair[1].state.rank()));
    }
}
