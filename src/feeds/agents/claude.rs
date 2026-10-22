//! Claude Code: one `~\.claude\sessions\<pid>.json` per running session, with its own status
//! (`busy`, `idle`, `waiting`) and where it runs (terminal, VS Code, desktop app).

use std::fs;
use std::path::Path;

use super::util::{alive, clean, folder, mtime, number_field, object, read_small, text_field};
use super::{DETAIL_MAX, DONE_FOR, Kind, MAX_JSON, PROJECT_MAX, Session, State, TITLE_MAX};

/// A transcript written this recently means work is happening even if `status` lags behind.
const TRANSCRIPT_ACTIVE: u64 = 90;

#[derive(Debug, Default, PartialEq)]
struct Record {
    pid: u32,
    session_id: String,
    cwd: String,
    name: String,
    status: String,
    waiting_for: String,
    entrypoint: String,
    /// `statusUpdatedAt`, else `updatedAt`, in epoch milliseconds.
    stamp_ms: u64,
}

pub fn sessions(home: &Path, now: u64) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(home.join(".claude").join("sessions")) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(record) = read_small(&path, MAX_JSON).and_then(|body| parse(&body)) else { continue };
        let file_pid = path.file_stem().and_then(|stem| stem.to_str()).and_then(|stem| stem.parse().ok()).unwrap_or(0);
        let pid = if record.pid != 0 { record.pid } else { file_pid };
        if record.cwd.trim().is_empty() || !alive(pid) {
            continue;
        }
        let stamp = if record.stamp_ms > 0 { record.stamp_ms / 1000 } else { mtime(&path).unwrap_or(now) };
        let since = now.saturating_sub(stamp);
        let transcript = transcript_age(home, &record.session_id, now);
        let state = state(&record.status, since, transcript);
        let project = clean(&folder(&record.cwd), PROJECT_MAX);
        let title = [clean(&record.name, TITLE_MAX), project.clone()]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or_else(|| Kind::Claude.label().to_string());
        let detail = match state {
            State::Waiting if !record.waiting_for.is_empty() => clean(&record.waiting_for, DETAIL_MAX),
            _ => surface(&record.entrypoint).to_string(),
        };
        rows.push(Session {
            id: format!("claude:{pid}"),
            kind: Kind::Claude,
            title,
            project,
            state,
            detail,
            age_secs: transcript.map_or(since, |age| age.min(since)),
            pid,
            folder: record.cwd.trim().to_string(),
        });
    }
    rows
}

fn parse(body: &str) -> Option<Record> {
    let root = object(body)?;
    let millis = |key: &str| number_field(&root, key).unwrap_or(0.0).max(0.0) as u64;
    let status_ms = millis("statusUpdatedAt");
    Some(Record {
        pid: number_field(&root, "pid").unwrap_or(0.0).max(0.0) as u32,
        session_id: text_field(&root, "sessionId"),
        cwd: text_field(&root, "cwd"),
        name: text_field(&root, "name"),
        status: text_field(&root, "status"),
        waiting_for: text_field(&root, "waitingFor"),
        entrypoint: text_field(&root, "entrypoint"),
        stamp_ms: if status_ms > 0 { status_ms } else { millis("updatedAt") },
    })
}

/// `waiting` wins, then `busy` or a fresh transcript, then a recent finish, else idle.
fn state(status: &str, since_status: u64, transcript_age: Option<u64>) -> State {
    if status.eq_ignore_ascii_case("waiting") {
        State::Waiting
    } else if status.eq_ignore_ascii_case("busy") || transcript_age.is_some_and(|age| age < TRANSCRIPT_ACTIVE) {
        State::Running
    } else if since_status < DONE_FOR {
        State::Done
    } else {
        State::Idle
    }
}

/// Where the session runs, from `entrypoint`.
fn surface(entrypoint: &str) -> &'static str {
    match entrypoint {
        "claude-vscode" => "VS Code",
        "claude-desktop" => "Desktop app",
        "claude-jetbrains" => "JetBrains",
        "cli" | "" => "Terminal",
        _ => "",
    }
}

/// Age of `<sessionId>.jsonl` under any `~\.claude\projects\<folder>`.
fn transcript_age(home: &Path, session_id: &str, now: u64) -> Option<u64> {
    if session_id.is_empty() || !session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    fs::read_dir(home.join(".claude").join("projects"))
        .ok()?
        .flatten()
        .filter_map(|entry| mtime(&entry.path().join(&file)))
        .max()
        .map(|modified| now.saturating_sub(modified))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_session_file() {
        let body = r#"{"pid":114796,"sessionId":"2e08195e-633c-49fa-82c2-ce28c0eb5d18","cwd":"D:\\work\\acme\\billing-api","name":"Billing API backfill","status":"waiting","entrypoint":"claude-vscode","updatedAt":1792597058937,"statusUpdatedAt":1792597058937,"waitingFor":"permission prompt"}"#;
        let record = parse(body).expect("session JSON");
        assert_eq!(record.pid, 114796);
        assert_eq!(record.waiting_for, "permission prompt");
        assert_eq!(record.stamp_ms, 1_792_597_058_937);
        assert_eq!(folder(&record.cwd), "billing-api");
        assert_eq!(surface(&record.entrypoint), "VS Code");
    }

    #[test]
    fn picks_state() {
        assert_eq!(state("waiting", 9_000, Some(5)), State::Waiting);
        assert_eq!(state("busy", 9_000, None), State::Running);
        assert_eq!(state("idle", 9_000, Some(30)), State::Running);
        assert_eq!(state("idle", 60, Some(600)), State::Done);
        assert_eq!(state("idle", 9_000, None), State::Idle);
    }
}
