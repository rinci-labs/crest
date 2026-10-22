//! Oh My Pi (`omp`): client files at `~\.omp\run\daemons\<hash>\clients\<pid>-<id>.json` name
//! the pid and project folder. Title and model come from the start of the newest transcript in
//! `~\.omp\agent\sessions` whose working folder matches.

use std::fs;
use std::path::{Path, PathBuf};

use super::util::{alive, clean, folder, mtime, number_field, object, read_head, read_small, text_field};
use super::{DETAIL_MAX, Kind, MAX_JSON, PROJECT_MAX, Session, State, TITLE_MAX};

/// A client file touched this recently means the agent is working.
const ACTIVE: u64 = 2 * 60;
/// Touched within this window (but not `ACTIVE`): it just finished.
const DONE: u64 = 30 * 60;

pub fn sessions(home: &Path, now: u64) -> Vec<Session> {
    let Ok(daemons) = fs::read_dir(home.join(".omp").join("run").join("daemons")) else {
        return Vec::new();
    };
    let transcripts = transcripts(home);
    let mut rows = Vec::new();
    for daemon in daemons.flatten() {
        let hash = daemon.file_name().to_string_lossy().into_owned();
        if hash == "global" {
            continue;
        }
        let Ok(clients) = fs::read_dir(daemon.path().join("clients")) else { continue };
        for client in clients.flatten() {
            let path = client.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some((pid, project_dir)) = read_small(&path, MAX_JSON).and_then(|body| parse_client(&body)) else { continue };
            let Some(modified) = mtime(&path) else { continue };
            if !alive(pid) {
                continue;
            }
            let age = now.saturating_sub(modified);
            let head = transcripts
                .iter()
                .find(|transcript| read_head(transcript, 8 * 1024).is_some_and(|head| same_folder(&head, &project_dir)))
                .and_then(|transcript| read_head(transcript, 16 * 1024))
                .unwrap_or_default();
            let model = Some(model(&head)).filter(|model| !model.is_empty()).unwrap_or_else(|| welcome_model(home, &hash));
            let project = clean(&folder(&project_dir), PROJECT_MAX);
            rows.push(Session {
                id: format!("omp:{pid}"),
                kind: Kind::Omp,
                title: Some(clean(&title(&head), TITLE_MAX)).filter(|title| !title.is_empty()).unwrap_or_else(|| project.clone()),
                project,
                state: match age {
                    age if age < ACTIVE => State::Running,
                    age if age < DONE => State::Done,
                    _ => State::Idle,
                },
                detail: clean(&model, DETAIL_MAX),
                age_secs: age,
                pid,
                folder: project_dir.trim().to_string(),
            });
        }
    }
    rows
}

fn parse_client(body: &str) -> Option<(u32, String)> {
    let root = object(body)?;
    let pid = number_field(&root, "pid").unwrap_or(0.0).max(0.0) as u32;
    let project_dir = text_field(&root, "projectDir");
    (pid != 0 && !project_dir.is_empty()).then_some((pid, project_dir))
}

/// Every transcript, newest first.
fn transcripts(home: &Path) -> Vec<PathBuf> {
    let Ok(dirs) = fs::read_dir(home.join(".omp").join("agent").join("sessions")) else { return Vec::new() };
    let mut files: Vec<(u64, PathBuf)> = dirs
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .flat_map(|entry| fs::read_dir(entry.path()).into_iter().flatten().flatten())
        .map(|file| file.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .filter_map(|path| Some((mtime(&path)?, path)))
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files.into_iter().map(|(_, path)| path).collect()
}

/// The transcript's `session` record names the same working folder (case-insensitive).
fn same_folder(head: &str, project_dir: &str) -> bool {
    head.lines().take(4).filter(|line| line.contains("\"session\"")).filter_map(|line| object(line.trim())).any(|record| {
        let cwd = text_field(&record, "cwd");
        !cwd.is_empty() && cwd.eq_ignore_ascii_case(project_dir.trim())
    })
}

/// Title from a `title` or `session` record near the start.
fn title(head: &str) -> String {
    head.lines()
        .take(4)
        .filter(|line| line.contains("\"title\""))
        .filter_map(|line| object(line.trim()))
        .filter(|record| matches!(text_field(record, "type").as_str(), "title" | "session"))
        .map(|record| text_field(&record, "title"))
        .find(|title| !title.is_empty())
        .unwrap_or_default()
}

/// Model from the first `model_change` record.
fn model(head: &str) -> String {
    head.lines()
        .filter(|line| line.contains("model_change"))
        .filter_map(|line| object(line.trim()))
        .filter(|record| text_field(record, "type") == "model_change")
        .map(|record| text_field(&record, "model"))
        .find(|model| !model.is_empty())
        .unwrap_or_default()
}

/// Starting model from `~\.omp\agent\cache\composer\<hash>\welcome.json`.
fn welcome_model(home: &Path, hash: &str) -> String {
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return String::new();
    }
    let path = home.join(".omp").join("agent").join("cache").join("composer").join(hash).join("welcome.json");
    read_small(&path, MAX_JSON).and_then(|body| object(&body)).map(|root| text_field(&root, "modelName")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_transcript_head_and_client() {
        let head = "{\"type\":\"title\",\"v\":1,\"title\":\"Build crest agents page\"}\n\
{\"type\":\"session\",\"version\":3,\"id\":\"x\",\"cwd\":\"D:\\\\work\\\\crest\",\"title\":\"Session title\"}\n\
{\"type\":\"model_change\",\"model\":\"provider/model-1\"}\n";
        assert!(same_folder(head, r"d:\WORK\crest"));
        assert!(!same_folder(head, r"D:\work\other"));
        assert_eq!(title(head), "Build crest agents page");
        assert_eq!(model(head), "provider/model-1");

        let (pid, project_dir) = parse_client(r#"{"pid":150252,"id":"150252-4c10","projectDir":"D:\\work\\crest"}"#).expect("client");
        assert_eq!((pid, folder(&project_dir).as_str()), (150252, "crest"));
        assert!(parse_client("{}").is_none());
    }
}
