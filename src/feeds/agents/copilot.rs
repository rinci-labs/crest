//! GitHub Copilot CLI: one folder per session in `~\.copilot\session-state\<id>\` with
//! `workspace.yaml` (working folder, summary) and `events.jsonl` (turn events).
//!
//! The CLI runs on Node, so there is no dedicated process to check. Only recently written
//! sessions are shown; the state comes from the last `assistant.turn_*` or `abort` event.

use std::fs;
use std::path::Path;

use super::util::{clean, field, folder, mtime, read_small, read_tail, yaml_fields};
use super::{DONE_FOR, Kind, PROJECT_MAX, Session, State, TITLE_MAX};

/// Sessions untouched for longer than this are not shown.
const RECENT: u64 = 15 * 60;
/// A turn that started this long ago without writes is treated as stalled.
const STALLED: u64 = 5 * 60;

pub fn sessions(home: &Path, now: u64) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(home.join(".copilot").join("session-state")) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let events = dir.join("events.jsonl");
        let Some(age) = mtime(&events).map(|modified| now.saturating_sub(modified)) else { continue };
        if age >= RECENT {
            continue;
        }
        let workspace = read_small(&dir.join("workspace.yaml"), 16 * 1024).map(|text| yaml_fields(&text)).unwrap_or_default();
        let cwd = field(&workspace, "cwd");
        let project = clean(&folder(&cwd), PROJECT_MAX);
        let title = [clean(&field(&workspace, "summary"), TITLE_MAX), project.clone()]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or_else(|| Kind::Copilot.label().to_string());
        let turn = read_tail(&events, 32 * 1024).map(|tail| last_turn(&tail)).unwrap_or(Turn::None);
        rows.push(Session {
            id: format!("copilot:{}", entry.file_name().to_string_lossy()),
            kind: Kind::Copilot,
            title,
            project,
            state: state(turn, age),
            detail: String::new(),
            age_secs: age,
            pid: 0,
            folder: cwd.trim().to_string(),
        });
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Turn {
    None,
    Started,
    Ended,
    Aborted,
}

fn state(turn: Turn, age: u64) -> State {
    match turn {
        Turn::Started if age < STALLED => State::Running,
        Turn::Ended if age < DONE_FOR => State::Done,
        _ => State::Idle,
    }
}

/// Events are one JSON object per line with `type` as the first key.
fn last_turn(tail: &str) -> Turn {
    tail.lines().fold(Turn::None, |turn, line| {
        if line.starts_with(r#"{"type":"assistant.turn_start""#) || line.starts_with(r#"{"type":"user.message""#) {
            Turn::Started
        } else if line.starts_with(r#"{"type":"assistant.turn_end""#) {
            Turn::Ended
        } else if line.starts_with(r#"{"type":"abort""#) {
            Turn::Aborted
        } else {
            turn
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_turn_events() {
        let tail = "{\"type\":\"user.message\",\"data\":{}}\n{\"type\":\"assistant.turn_start\",\"data\":{}}\n{\"type\":\"tool.execution_start\",\"data\":{}}\n";
        assert_eq!(last_turn(tail), Turn::Started);
        assert_eq!(last_turn(&format!("{tail}{{\"type\":\"assistant.turn_end\",\"data\":{{}}}}\n")), Turn::Ended);
        assert_eq!(last_turn("{\"type\":\"abort\",\"data\":{}}\n"), Turn::Aborted);
        assert_eq!(state(Turn::Started, 10), State::Running);
        assert_eq!(state(Turn::Ended, 10), State::Done);
        assert_eq!(state(Turn::Aborted, 10), State::Idle);
    }
}
