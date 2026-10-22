//! Optional agent hooks. Installation is always explicit from the Agents page; hook processes
//! only write a small, bounded status record under `%APPDATA%\Crest\agent-hooks` and never make
//! permission decisions. Existing config fields and third-party hooks are preserved.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Data::Json::{JsonArray, JsonObject, JsonValue};
use windows::core::HSTRING;

use super::util::{clean, folder, number_field, object, read_small, text_field};
use super::{DETAIL_MAX, DONE_FOR, HookStatus, Kind, MAX_JSON, PROJECT_MAX, Provider, Session, State, TITLE_MAX};

const ACTIVE_FOR: u64 = 5 * 60;
const KEEP_FOR: u64 = 30 * 60;
const INPUT_MAX: u64 = 16 * 1024;
const MARKER: &str = "--agent-hook";

/// The hook payload field a third-party tool uses to name itself. When present it overrides the
/// name in the installed command, so one tool can report several agents (e.g. "gemini" and
/// "antigravity"). `coucou_agent` is understood too, so a tool already wired for Coucou works
/// with Crest without changes.
const AGENT_FIELDS: [&str; 2] = ["crest_agent", "coucou_agent"];

/// Read the payload, work out which tool and event it describes, and write the status record.
///
/// The payload is whatever the agent puts on stdin; anything unreadable is treated as an empty
/// object. When `event` is empty the payload's own `hook_event_name` is used, so a tool can send
/// just `{...}` with the command `crest.exe --agent-hook <tool>`.
pub fn record(kind: &str, event: &str) {
    let mut input = String::new();
    let _ = std::io::stdin().take(INPUT_MAX).read_to_string(&mut input);
    let root = object(&input);
    let field = |names: &[&str]| {
        root.as_ref()
            .and_then(|root| names.iter().find_map(|name| Some(text_field(root, name)).filter(|value| !value.is_empty())))
            .unwrap_or_default()
    };
    // A tool may name itself in the payload; that wins over the installed command.
    let named = field(&AGENT_FIELDS);
    let kind = parse_kind(if named.is_empty() { kind } else { &named });
    let Some(kind) = kind else { return };
    let payload_event = field(&["hook_event_name"]);
    let raw_event = if event.is_empty() { payload_event.as_str() } else { event };
    let event = normalise_event(raw_event);
    if event.is_empty() {
        return;
    }
    // A permission request from a tool Crest cannot show a card for must never be answered
    // "allow": Copilot CLI and Cursor expect JSON on stdout and would read silence as refusal or
    // hang, so the safe answer is to ask again in their own terminal.
    if event.eq_ignore_ascii_case("PermissionRequest") {
        answer_ask(&kind);
        return;
    }
    let session = clean(&field(&["session_id", "conversation_id", "sessionId"]), 64);
    if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
        return;
    }
    let Some(dir) = records_dir() else { return };
    let path = dir.join(format!("{}-{session}.json", kind_key(&kind)));
    if event.eq_ignore_ascii_case("SessionEnd") {
        let _ = fs::remove_file(path);
        return;
    }
    let _ = fs::create_dir_all(&dir);
    let cwd = field(&["cwd", "workspace", "workspace_path"]);
    // Prompts and command arguments may contain private conversation or source text. The hook
    // record only needs a terse activity label; the project folder remains the row fallback.
    let title = clean(&field(&["tool_name"]), TITLE_MAX);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs());
    let Some(record) = make_record(&kind, &event, &session, &cwd, &title, stamp) else { return };
    let Ok(text) = record.Stringify() else { return };
    let _ = fs::write(path, text.to_string());
}

/// Tell an agent that Crest made no decision, so it asks in its own terminal instead of
/// proceeding. Nothing is ever approved from here: Crest has no approval card.
fn answer_ask(kind: &Kind) {
    // Copilot CLI reads `permissionDecision`; Cursor reads `permission`. Both are told to ask.
    let body = match kind {
        Kind::Copilot => "{\"permissionDecision\":\"ask\"}",
        Kind::Cursor => "{\"permission\":\"ask\"}",
        _ => return,
    };
    use std::io::Write as _;
    let _ = std::io::stdout().write_all(body.as_bytes());
    let _ = std::io::stdout().flush();
}

pub fn sessions(now: u64) -> Vec<Session> {
    let Some(dir) = records_dir() else { return Vec::new() };
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let body = read_small(&path, MAX_JSON)?;
            let root = object(&body)?;
            let stamp = number_field(&root, "stamp")?.max(0.0) as u64;
            let age = now.saturating_sub(stamp);
            if age >= KEEP_FOR {
                let _ = fs::remove_file(path);
                return None;
            }
            let kind = parse_kind(&text_field(&root, "kind"))?;
            let event = text_field(&root, "event");
            let cwd = text_field(&root, "cwd");
            let project = clean(&folder(&cwd), PROJECT_MAX);
            let detail = clean(&event_label(&event), DETAIL_MAX);
            let title = clean(&text_field(&root, "title"), TITLE_MAX);
            Some(Session {
                id: format!("hook:{}", text_field(&root, "session")),
                kind,
                title: if title.is_empty() { project.clone() } else { title },
                project,
                state: event_state(&event, age),
                detail,
                age_secs: age,
                pid: 0,
                folder: cwd,
            })
        })
        .collect()
}

pub fn install() -> Result<usize, String> {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).ok_or("User folder is unavailable")?;
    let exe = std::env::current_exe().map_err(|_| "Crest executable is unavailable")?;
    let command = |kind: &str, event: &str| format!("\"{}\" {MARKER} {kind} {event}", exe.display());
    let mut installed = 0;
    installed += usize::from(install_cursor(&home.join(".cursor").join("hooks.json"), &command).is_ok());
    installed += usize::from(install_claude(&home.join(".claude").join("settings.json"), &command).is_ok());
    installed += usize::from(install_copilot(&home.join(".copilot").join("hooks").join("crest.json"), &command).is_ok());
    (installed > 0).then_some(installed).ok_or_else(|| "No hook configuration could be written".to_string())
}

/// Install the lifecycle relay for one provider. Native readers do not need hooks.
pub fn install_provider(provider: Provider) -> Result<(), String> {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).ok_or("User folder is unavailable")?;
    let exe = std::env::current_exe().map_err(|_| "Crest executable is unavailable")?;
    let command = |name: &str, event: &str| format!("\"{}\" {MARKER} {name} {event}", exe.display());
    match provider {
        Provider::Claude => install_claude(&home.join(".claude").join("settings.json"), &command),
        Provider::Cursor => install_cursor(&home.join(".cursor").join("hooks.json"), &command),
        Provider::Copilot => install_copilot(&home.join(".copilot").join("hooks").join("crest.json"), &command),
    }
}

pub fn installed_hooks() -> HookStatus {
    let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
        return HookStatus::default();
    };
    HookStatus {
        claude: has_marker(&home.join(".claude").join("settings.json")),
        cursor: has_marker(&home.join(".cursor").join("hooks.json")),
        copilot: has_marker(&home.join(".copilot").join("hooks").join("crest.json")),
    }
}

fn has_marker(path: &Path) -> bool {
    read_small(path, 64 * 1024).is_some_and(|text| text.contains(MARKER))
}

pub fn records_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from).map(|path| path.join("Crest").join("agent-hooks"))
}

fn install_cursor(path: &Path, command: &impl Fn(&str, &str) -> String) -> Result<(), String> {
    let root = load(path)?;
    set_number(&root, "version", 1.0)?;
    let hooks = named_object(&root, "hooks")?;
    for event in ["sessionStart", "beforeSubmitPrompt", "preToolUse", "postToolUse", "stop", "sessionEnd"] {
        add_command(&hooks, event, &command("cursor", event), false)?;
    }
    save(path, &root)
}

fn install_claude(path: &Path, command: &impl Fn(&str, &str) -> String) -> Result<(), String> {
    let root = load(path)?;
    let hooks = named_object(&root, "hooks")?;
    for event in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SessionEnd"] {
        add_command(&hooks, event, &command("claude", event), true)?;
    }
    save(path, &root)
}

fn install_copilot(path: &Path, command: &impl Fn(&str, &str) -> String) -> Result<(), String> {
    let root = JsonObject::new().map_err(|_| "Could not create Copilot hooks")?;
    set_number(&root, "version", 1.0)?;
    let hooks = named_object(&root, "hooks")?;
    for event in ["sessionStart", "userPromptSubmitted", "preToolUse", "postToolUse", "agentStop", "sessionEnd"] {
        let array = JsonArray::new().map_err(|_| "Could not create Copilot hook list")?;
        let item = JsonObject::new().map_err(|_| "Could not create Copilot hook")?;
        set_string(&item, "bash", &command("copilot", event))?;
        set_number(&item, "timeoutSec", 2.0)?;
        array.Append(&item).map_err(|_| "Could not append Copilot hook")?;
        hooks.SetNamedValue(&HSTRING::from(event), &array).map_err(|_| "Could not set Copilot hook")?;
    }
    save(path, &root)
}

fn add_command(hooks: &JsonObject, event: &str, command: &str, claude: bool) -> Result<(), String> {
    let array = match hooks.GetNamedArray(&HSTRING::from(event)) {
        Ok(array) => array,
        Err(_) => JsonArray::new().map_err(|_| "Could not create hook list")?,
    };
    if array.Stringify().is_ok_and(|text| text.to_string().contains(MARKER)) {
        return Ok(());
    }
    let item = JsonObject::new().map_err(|_| "Could not create hook")?;
    if claude {
        let commands = JsonArray::new().map_err(|_| "Could not create hook list")?;
        let nested = JsonObject::new().map_err(|_| "Could not create hook command")?;
        set_string(&nested, "type", "command")?;
        set_string(&nested, "command", command)?;
        commands.Append(&nested).map_err(|_| "Could not append hook command")?;
        item.SetNamedValue(&HSTRING::from("hooks"), &commands).map_err(|_| "Could not set hook command")?;
    } else {
        set_string(&item, "command", command)?;
    }
    array.Append(&item).map_err(|_| "Could not append hook")?;
    hooks.SetNamedValue(&HSTRING::from(event), &array).map_err(|_| "Could not set hook".to_string())
}

fn load(path: &Path) -> Result<JsonObject, String> {
    match fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => JsonObject::Parse(&HSTRING::from(text)).map_err(|_| "Existing hook settings are not valid JSON".to_string()),
        _ => JsonObject::new().map_err(|_| "Could not create hook settings".to_string()),
    }
}

fn save(path: &Path, root: &JsonObject) -> Result<(), String> {
    let parent = path.parent().ok_or("Hook folder is unavailable")?;
    fs::create_dir_all(parent).map_err(|_| "Could not create hook folder")?;
    let text = root.Stringify().map_err(|_| "Could not serialize hook settings")?;
    fs::write(path, text.to_string()).map_err(|_| "Could not write hook settings".to_string())
}

fn named_object(parent: &JsonObject, name: &str) -> Result<JsonObject, String> {
    if let Ok(found) = parent.GetNamedObject(&HSTRING::from(name)) {
        return Ok(found);
    }
    let value = JsonObject::new().map_err(|_| "Could not create settings section")?;
    parent.SetNamedValue(&HSTRING::from(name), &value).map_err(|_| "Could not set settings section")?;
    Ok(value)
}

fn set_string(object: &JsonObject, name: &str, value: &str) -> Result<(), String> {
    let value = JsonValue::CreateStringValue(&HSTRING::from(value)).map_err(|_| "Could not create setting")?;
    object.SetNamedValue(&HSTRING::from(name), &value).map_err(|_| "Could not set setting".to_string())
}

fn set_number(object: &JsonObject, name: &str, value: f64) -> Result<(), String> {
    let value = JsonValue::CreateNumberValue(value).map_err(|_| "Could not create setting")?;
    object.SetNamedValue(&HSTRING::from(name), &value).map_err(|_| "Could not set setting".to_string())
}

fn make_record(kind: &Kind, event: &str, session: &str, cwd: &str, title: &str, stamp: u64) -> Option<JsonObject> {
    let root = JsonObject::new().ok()?;
    set_string(&root, "kind", &kind_key(kind)).ok()?;
    set_string(&root, "event", event).ok()?;
    set_string(&root, "session", session).ok()?;
    set_string(&root, "cwd", cwd).ok()?;
    set_string(&root, "title", title).ok()?;
    set_number(&root, "stamp", stamp as f64).ok()?;
    Some(root)
}

/// Translate a tool's own event names to the canonical ones the page understands, so a new tool
/// only has to say what happened. Unknown names are kept as they are.
///
/// The mappings are the ones Coucou documents for the same tools (see its `docs/AGENTS.md`), so a
/// tool already wired for one notch companion works with Crest.
fn normalise_event(event: &str) -> String {
    let lower = event.trim().to_ascii_lowercase();
    let canonical = match lower.as_str() {
        // Gemini CLI and Antigravity
        "beforetool" | "pretooluse" => "PreToolUse",
        "aftertool" | "posttooluse" | "posttoolusefailure" | "postinvocation" => "PostToolUse",
        "beforeagent" | "userpromptsubmit" | "beforesubmitprompt" | "preinvocation" | "agent.start" => "UserPromptSubmit",
        "afteragent" | "stop" | "agentstop" | "agent.end" | "session.idle" | "post_llm_call" => "Stop",
        "stopfailure" | "session.error" => "StopFailure",
        "sessionstart" | "sessionstarted" | "session.created" | "on_session_start" => "SessionStart",
        "sessionend" | "session.ended" | "session.deleted" => "SessionEnd",
        "notification" => "Notification",
        "permissionrequest" | "permission.asked" | "pre_approval_request" => "PermissionRequest",
        "subagentstart" => "SubagentStart",
        "subagentstop" => "SubagentStop",
        _ => return event.trim().to_string(),
    };
    canonical.to_string()
}

fn event_state(event: &str, age: u64) -> State {
    let lower = event.to_ascii_lowercase();
    if lower.contains("permission") || lower == "notification" {
        State::Waiting
    } else if matches!(lower.as_str(), "stop" | "agentstop") && age < DONE_FOR {
        State::Done
    } else if lower == "stopfailure" {
        // A failed turn is finished, not working, whatever its age.
        State::Done
    } else if age < ACTIVE_FOR {
        State::Running
    } else {
        State::Idle
    }
}

fn event_label(event: &str) -> String {
    match event.to_ascii_lowercase().as_str() {
        "sessionstart" => "Session started".to_string(),
        "userpromptsubmit" | "beforesubmitprompt" => "Thinking".to_string(),
        "pretooluse" => "Using a tool".to_string(),
        "posttooluse" => "Working".to_string(),
        "stop" | "agentstop" => "Finished".to_string(),
        _ => event.to_string(),
    }
}

/// The tool that sent the event: one of the built-in agents, or any other tool that names
/// itself with `crest_agent` (e.g. `crest.exe --agent-hook tool <event>`, or the name in the
/// hook payload). The name is what the Agents page shows.
fn parse_kind(kind: &str) -> Option<Kind> {
    match kind.to_ascii_lowercase().as_str() {
        "claude" => Some(Kind::Claude),
        "codex" => Some(Kind::Codex),
        "cursor" => Some(Kind::Cursor),
        "copilot" => Some(Kind::Copilot),
        "omp" => Some(Kind::Omp),
        name => {
            // Validation keeps the name safe to use in a file name and on screen.
            let name = clean(name, 24);
            let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
            valid.then_some(Kind::Other(name))
        }
    }
}

fn kind_key(kind: &Kind) -> String {
    match kind {
        Kind::Claude => "claude".to_string(),
        Kind::Codex => "codex".to_string(),
        Kind::Cursor => "cursor".to_string(),
        Kind::Copilot => "copilot".to_string(),
        Kind::Omp => "omp".to_string(),
        Kind::Other(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_events_map_to_readable_states() {
        assert_eq!(event_state("permissionRequest", 1), State::Waiting);
        assert_eq!(event_state("stop", 1), State::Done);
        assert_eq!(event_state("postToolUse", 1), State::Running);
        assert_eq!(event_state("postToolUse", ACTIVE_FOR), State::Idle);
    }

    #[test]
    fn third_party_tools_name_themselves() {
        // A name from the payload becomes the tool shown on the page.
        assert_eq!(parse_kind("gemini"), Some(Kind::Other("gemini".to_string())));
        // Names that could not be a file name or a label are refused.
        assert_eq!(parse_kind(""), None);
        assert_eq!(parse_kind("../evil"), None);
        assert_eq!(parse_kind("a b"), None);
        // Built-in agents keep their own kind.
        assert_eq!(parse_kind("Claude"), Some(Kind::Claude));
        // Each tool keeps one tint between runs.
        let tint = |name: &str| Kind::Other(name.to_string()).tint();
        assert_eq!(tint("gemini"), tint("gemini"));
        assert_ne!(tint("gemini"), tint("aider"));
    }

    #[test]
    fn vendor_events_become_the_ones_the_page_understands() {
        // Gemini CLI and Antigravity names, as Coucou documents them.
        assert_eq!(normalise_event("BeforeTool"), "PreToolUse");
        assert_eq!(normalise_event("AfterTool"), "PostToolUse");
        assert_eq!(normalise_event("PreInvocation"), "UserPromptSubmit");
        assert_eq!(normalise_event("PostInvocation"), "PostToolUse");
        assert_eq!(normalise_event("AfterAgent"), "Stop");
        // OpenCode and Hermes.
        assert_eq!(normalise_event("session.idle"), "Stop");
        assert_eq!(normalise_event("session.error"), "StopFailure");
        assert_eq!(normalise_event("pre_approval_request"), "PermissionRequest");
        // An event Crest does not know is kept, so a new tool still shows something.
        assert_eq!(normalise_event("MyOwnEvent"), "MyOwnEvent");
        // A failed turn reads as finished, not as still working.
        assert_eq!(event_state("StopFailure", 2), State::Done);
        // A plain notification is the agent asking for the user.
        assert_eq!(event_state("Notification", 2), State::Waiting);
    }

    #[test]
    fn lifecycle_labels_cover_each_adapter() {
        assert_eq!(event_label("beforeSubmitPrompt"), "Thinking");
        assert_eq!(event_label("userPromptSubmit"), "Thinking");
        assert_eq!(event_label("agentStop"), "Finished");
    }
}

