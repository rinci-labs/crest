//! Cursor: per project in `~\.cursor\projects\<slug>\`, shell tasks under `terminals\*.txt`
//! (YAML header with pid and status) and chat transcripts under
//! `agent-transcripts\<id>\<id>.jsonl`.
//!
//! Cursor writes no chat status, so chats are judged by transcript age: freshly written means
//! running, a little older means just finished. Shell tasks are trusted while their pid lives.

use std::fs;
use std::path::Path;

use super::util::{alive, clean, field, folder, mtime, object, parse_iso, read_head, text_field, yaml_fields};
use super::{DETAIL_MAX, Kind, PROJECT_MAX, Session, State, TITLE_MAX};
use windows::core::HSTRING;

/// At most this many Cursor rows, newest first.
const MAX_ROWS: usize = 8;
/// A chat transcript written within this window is running; until `SHOWN` it counts as done.
const ACTIVE: u64 = 90;
const SHOWN: u64 = 5 * 60;

pub fn sessions(home: &Path, now: u64) -> Vec<Session> {
    let Ok(projects) = fs::read_dir(home.join(".cursor").join("projects")) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for project in projects.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())) {
        let slug = project.file_name().to_string_lossy().into_owned();
        let tasks = terminals(&project.path(), now);
        // A project with a live shell task is already represented; its chat would be a duplicate.
        if tasks.is_empty() {
            rows.extend(chats(&project.path(), &slug, now));
        } else {
            rows.extend(tasks);
        }
    }
    rows.sort_by_key(|row| row.age_secs);
    rows.truncate(MAX_ROWS);
    rows
}

#[derive(Debug, Default, PartialEq)]
struct Terminal {
    pid: u32,
    cwd: String,
    command: String,
    title: String,
    status: String,
    started_at: String,
}

fn terminals(project: &Path, now: u64) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(project.join("terminals")) else { return Vec::new() };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .filter_map(|path| {
            // Terminal output follows the header and can be long; the header fits in 1 KB.
            let terminal = parse_terminal(&read_head(&path, 1024)?)?;
            if !terminal.status.eq_ignore_ascii_case("running") || !alive(terminal.pid) {
                return None;
            }
            let started = parse_iso(&terminal.started_at)?;
            let project = clean(&folder(&terminal.cwd), PROJECT_MAX);
            let title = [clean(&terminal.title, TITLE_MAX), project.clone()]
                .into_iter()
                .find(|text| !text.is_empty())
                .unwrap_or_else(|| Kind::Cursor.label().to_string());
            Some(Session {
                id: format!("cursor:task:{}", terminal.pid),
                kind: Kind::Cursor,
                title,
                project,
                state: State::Running,
                detail: clean(&terminal.command, DETAIL_MAX),
                age_secs: now.saturating_sub(started),
                pid: terminal.pid,
                folder: terminal.cwd.trim().to_string(),
            })
        })
        .collect()
}

/// The `key: value` header between two `---` lines at the top of a terminal file.
fn parse_terminal(text: &str) -> Option<Terminal> {
    let mut parts = text.splitn(3, "---");
    parts.next()?;
    let fields = yaml_fields(parts.next()?);
    Some(Terminal {
        pid: field(&fields, "pid").trim().parse().ok()?,
        cwd: field(&fields, "cwd"),
        command: field(&fields, "command"),
        title: field(&fields, "title"),
        status: field(&fields, "status"),
        started_at: field(&fields, "started_at"),
    })
}

fn chats(project: &Path, slug: &str, now: u64) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(project.join("agent-transcripts")) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path().join(format!("{id}.jsonl"));
            let age = now.saturating_sub(mtime(&path)?);
            if age >= SHOWN {
                return None;
            }
            let title = read_head(&path, 16 * 1024)
                .and_then(|head| first_question(&head))
                .map(|title| clean(&title, TITLE_MAX))
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| clean(slug, TITLE_MAX));
            Some(Session {
                id: format!("cursor:chat:{id}"),
                kind: Kind::Cursor,
                title,
                project: clean(slug, PROJECT_MAX),
                state: if age < ACTIVE { State::Running } else { State::Done },
                detail: "Chat".to_string(),
                age_secs: age,
                pid: 0,
                folder: String::new(),
            })
        })
        .collect()
}

/// The first user question in a transcript, without Cursor's `<timestamp>` wrapper.
fn first_question(head: &str) -> Option<String> {
    head.lines().take(32).find_map(|line| {
        let root = object(line.trim())?;
        if !text_field(&root, "role").eq_ignore_ascii_case("user") {
            return None;
        }
        let content = root.GetNamedObject(&HSTRING::from("message")).ok()?.GetNamedArray(&HSTRING::from("content")).ok()?;
        let text = content.GetObjectAt(0).ok()?.GetNamedString(&HSTRING::from("text")).ok()?.to_string();
        let query = text.split_once("<user_query>")?.1;
        let query = query.split("</user_query>").next().unwrap_or(query);
        let query = without_tag(query, "timestamp");
        let query = query.trim();
        (!query.is_empty()).then(|| query.to_string())
    })
}

/// Drop every `<tag>…</tag>`; an unclosed tag drops the rest.
fn without_tag(text: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find(&close) else { return out };
        rest = &rest[start + end + close.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_terminal_header() {
        let text = "---\npid: 138008\ncwd: \"d:\\\\work\\\\web-dashboard\"\ncommand: \"bun dev\"\ntitle: \"Restart local web and API dev servers\"\nstatus: running  \nstarted_at: 2026-10-18T08:46:02.370Z\n---\n$ bun run dev\n";
        let terminal = parse_terminal(text).expect("terminal header");
        assert_eq!(terminal.pid, 138008);
        assert_eq!(terminal.cwd, r"d:\work\web-dashboard");
        assert_eq!(terminal.command, "bun dev");
        assert!(terminal.status.eq_ignore_ascii_case("running"));
        assert_eq!(parse_iso(&terminal.started_at), Some(1_792_313_162));

        let block = "---\npid: 12588\ncwd: |\n  C:\\Users\\Arch\nstatus: succeeded\n---\n";
        assert_eq!(parse_terminal(block).expect("block header").cwd, r"C:\Users\Arch");
    }

    #[test]
    fn reads_first_question() {
        let line = r#"{"role":"user","message":{"content":[{"type":"text","text":"<timestamp>Monday, Sep 28, 2026</timestamp>\n<user_query>\nMake the modal smoother\n</user_query>"}]}}"#;
        assert_eq!(first_question(line).as_deref(), Some("Make the modal smoother"));
        assert_eq!(without_tag("a<x>b</x>c<x>d", "x"), "ac");
    }
}
