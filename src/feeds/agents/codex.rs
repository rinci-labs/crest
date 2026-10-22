//! OpenAI Codex (CLI and IDE extension): rollout transcripts in
//! `~\.codex\sessions\<yyyy>\<mm>\<dd>\rollout-<time>-<id>.jsonl`, thread names in
//! `~\.codex\session_index.jsonl`.
//!
//! Codex keeps no per-session pid file, so rows are shown only while a `codex.exe` process runs
//! and only for rollouts written recently. The state comes from the last turn event near the
//! end of the rollout (`task_started`, `task_complete`, `turn_aborted`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::util::{Processes, clean, folder, json_string, mtime, read_head, read_tail};
use super::{DETAIL_MAX, DONE_FOR, Kind, PROJECT_MAX, Session, State, TITLE_MAX};

/// Rollouts untouched for longer than this are not shown.
const RECENT: u64 = 30 * 60;
/// A started turn with no writes for this long is treated as stalled, not running.
const STALLED: u64 = 10 * 60;
/// Bytes read from the end of a rollout to find the last turn event and model.
const TAIL: u64 = 96 * 1024;
/// Bytes read from the start to find the working folder.
const HEAD: usize = 8 * 1024;

pub fn sessions(home: &Path, now: u64, processes: &Processes) -> Vec<Session> {
    if !processes.has("codex.exe") {
        return Vec::new();
    }
    let root = home.join(".codex");
    let recent: Vec<(PathBuf, u64)> = latest_days(&root.join("sessions"))
        .iter()
        .flat_map(|day| fs::read_dir(day).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .filter_map(|path| mtime(&path).map(|modified| (path, now.saturating_sub(modified))))
        .filter(|(_, age)| *age < RECENT)
        .collect();
    if recent.is_empty() {
        return Vec::new();
    }
    let names = thread_names(&root.join("session_index.jsonl"));
    recent
        .into_iter()
        .map(|(path, age)| {
            let id = thread_id(&path);
            let cwd = read_head(&path, HEAD).and_then(|head| json_string(&head, "cwd")).unwrap_or_default();
            let (event, model) = read_tail(&path, TAIL).map(|tail| last_turn(&tail)).unwrap_or_default();
            let project = clean(&folder(&cwd), PROJECT_MAX);
            let title = names
                .get(&id)
                .map(|name| clean(name, TITLE_MAX))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| if project.is_empty() { Kind::Codex.label().to_string() } else { project.clone() });
            Session {
                id: format!("codex:{id}"),
                kind: Kind::Codex,
                title,
                project,
                state: state(event, age),
                detail: clean(&model, DETAIL_MAX),
                age_secs: age,
                pid: 0,
                folder: cwd.trim().to_string(),
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Turn {
    #[default]
    None,
    Started,
    Complete,
    Aborted,
}

fn state(turn: Turn, age: u64) -> State {
    match turn {
        Turn::Started if age < STALLED => State::Running,
        Turn::Complete if age < DONE_FOR => State::Done,
        _ => State::Idle,
    }
}

/// The newest two day folders (`yyyy\mm\dd`), so a session that crosses midnight still shows.
fn latest_days(sessions: &Path) -> Vec<PathBuf> {
    let newest = |dir: &Path| -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        dirs.sort();
        dirs.reverse();
        dirs
    };
    let mut days = Vec::new();
    'search: for year in newest(sessions) {
        for month in newest(&year) {
            for day in newest(&month) {
                days.push(day);
                if days.len() == 2 {
                    break 'search;
                }
            }
        }
    }
    days
}

/// The thread id is the uuid at the end of `rollout-<time>-<uuid>.jsonl`.
fn thread_id(path: &Path) -> String {
    let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    stem.get(stem.len().saturating_sub(36)..).unwrap_or(stem).to_string()
}

/// `id → thread_name` from the end of the index; later lines win.
fn thread_names(index: &Path) -> HashMap<String, String> {
    let Some(tail) = read_tail(index, 64 * 1024) else { return HashMap::new() };
    tail.lines()
        .filter_map(|line| Some((json_string(line, "id")?, json_string(line, "thread_name")?)))
        .collect()
}

/// Last turn event and the latest model name in a rollout tail.
fn last_turn(tail: &str) -> (Turn, String) {
    let (mut turn, mut model) = (Turn::None, String::new());
    for line in tail.lines() {
        if line.contains("\"type\":\"event_msg\"") {
            if line.contains("\"payload\":{\"type\":\"task_started\"") {
                turn = Turn::Started;
            } else if line.contains("\"payload\":{\"type\":\"task_complete\"") {
                turn = Turn::Complete;
            } else if line.contains("\"payload\":{\"type\":\"turn_aborted\"") {
                turn = Turn::Aborted;
            }
        } else if line.contains("\"type\":\"turn_context\"")
            && let Some(name) = json_string(line, "model")
        {
            model = name;
        }
    }
    (turn, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rollout_tail() {
        let tail = concat!(
            r#"{"timestamp":"t","type":"turn_context","payload":{"turn_id":"1","cwd":"D:\\work","model":"gpt-5.6"}}"#, "\n",
            r#"{"timestamp":"t","type":"event_msg","payload":{"type":"task_started","turn_id":"1"}}"#, "\n",
            r#"{"timestamp":"t","type":"response_item","payload":{"type":"message","content":"task_complete"}}"#, "\n",
        );
        assert_eq!(last_turn(tail), (Turn::Started, "gpt-5.6".to_string()));
        let done = format!("{tail}{}\n", r#"{"timestamp":"t","type":"event_msg","payload":{"type":"task_complete"}}"#);
        assert_eq!(last_turn(&done).0, Turn::Complete);
    }

    #[test]
    fn maps_turns_to_states() {
        assert_eq!(state(Turn::Started, 5), State::Running);
        assert_eq!(state(Turn::Started, STALLED + 1), State::Idle);
        assert_eq!(state(Turn::Complete, 30), State::Done);
        assert_eq!(state(Turn::Aborted, 30), State::Idle);
        assert_eq!(state(Turn::None, 30), State::Idle);
    }

    #[test]
    fn takes_thread_id_from_name() {
        let path = Path::new("rollout-2026-10-18T20-00-26-01a14f19-6282-7a71-8919-620e3c28d444.jsonl");
        assert_eq!(thread_id(path), "01a14f19-6282-7a71-8919-620e3c28d444");
    }
}
