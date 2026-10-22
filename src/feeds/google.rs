//! Google account access (optional, off by default): unread Gmail and Google Calendar.
//!
//! Optional and off by default. Fonts aside, it is the only part of Crest that signs in anywhere,
//! so it stays strictly opt-in:
//!
//! 1. The user creates a free Google Cloud project, enables the Gmail API and the Google
//!    Calendar API, and creates an OAuth *Desktop app* client. Everything is read-only:
//!    `gmail.readonly` and `calendar.readonly`.
//! 2. The client id is pasted once (the client secret of an installed app is not a secret, but
//!    Crest asks for it anyway, since Google returns one).
//! 3. Connect opens the browser; the reply comes back to `http://127.0.0.1` on a temporary
//!    port. Crest stores the refresh token, protected with DPAPI for this Windows user only.
//!
//! No password is ever seen by Crest, and the token file can be deleted at any time
//! (tray menu > Google > Disconnect).
//!
//! Only what the panel shows is requested: message ids with subject and sender (no bodies) and
//! the next two weeks of calendar events.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use windows::Data::Json::JsonObject;
use windows::core::{HSTRING, Result};

use crate::shell::store;
use crate::system::http::{self, Request};
use crate::system::services::{Event, Notifier};

const TOKEN_FILE: &str = "google.txt";
const CLIENT_FILE: &str = "google-client.txt";
const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const GMAIL: &str = "https://gmail.googleapis.com/gmail/v1/users/me/messages";
const CALENDAR: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";
const USER_AGENT: &str = concat!("Crest/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rinci-labs/crest)");
const SCOPES: &str = "https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/calendar.readonly";
/// How long the browser may take to come back. Short on purpose: when Google refuses the sign-in
/// outright (a consent screen still in Testing, so the account is not a test user) it shows a 403
/// page and never redirects, so this is the only thing that ends the wait.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(120);

/// Shown when the browser never came back, with the reason and where to fix it.
pub const NO_REPLY: &str = "Google did not send you back. If it showed \"Error 403: access_denied\", \
    add your address under OAuth consent screen > Test users (or publish the app), then press Connect again.";
/// Unread messages listed, and the most the snapshot keeps; the panel shows fewer.
const MAX_MAIL: usize = 8;
/// Events kept from one answer, even if the server ignores the page size.
const MAX_EVENTS: usize = 200;
/// Calendar window: the next two weeks.
const CALENDAR_DAYS: i64 = 14;
/// Events requested per refresh; one page is enough for a two-week window.
const CALENDAR_MAX: u32 = 50;

/// OAuth client the user created, stored as plain text next to the other settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub id: String,
    pub secret: String,
}

impl Client {
    pub fn load() -> Option<Self> {
        Self::parse_stored(&store::load(CLIENT_FILE).join("\n"))
    }

    fn parse_stored(text: &str) -> Option<Self> {
        let id = text.lines().find_map(|l| l.strip_prefix("client-id="))?.trim().to_string();
        let secret = text.lines().find_map(|l| l.strip_prefix("client-secret=")).unwrap_or("").trim().to_string();
        (!id.is_empty()).then_some(Self { id, secret })
    }

    pub fn save(&self) {
        let clean = |text: &str| text.chars().filter(|c| !c.is_control() && !c.is_whitespace()).collect::<String>();
        store::save(CLIENT_FILE, [format!("client-id={}", clean(&self.id)), format!("client-secret={}", clean(&self.secret))]);
    }
}

/// Refresh token, encrypted with DPAPI for the current Windows user.
pub fn token() -> Option<String> {
    let text = store::load(TOKEN_FILE).into_iter().next()?;
    crate::system::secrets::unprotect(&text)
}

pub fn save_token(refresh: &str) {
    // `protect` seals straight from this string and overwrites its own DPAPI buffer before
    // freeing it, so no additional plaintext copy is made here. The caller keeps its `String`:
    // the account name lookup still needs it.
    match crate::system::secrets::protect(refresh) {
        Some(text) => store::save(TOKEN_FILE, [text]),
        None => error_log!("cannot protect the Google token"),
    }
}

pub fn forget() {
    let _ = store::dir().map(|dir| std::fs::remove_file(dir.join(TOKEN_FILE)));
}

/// Is an account connected?
pub fn connected() -> bool {
    token().is_some()
}

/// One unread message, as shown on the Mail page.
#[derive(Clone, Debug, PartialEq)]
pub struct Mail {
    pub id: String,
    /// Sender display name, falling back to the address.
    pub from: String,
    pub subject: String,
    /// Short age, e.g. "12m".
    pub age: String,
    /// Up to two letters for the avatar circle, from the sender's name.
    pub initials: String,
}

impl Mail {
    /// The first letters of the sender's first and last word: "Ana Silva" -> "AS", "GitHub" -> "G".
    fn initials(from: &str) -> String {
        let words: Vec<&str> = from.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).collect();
        let mut letters = String::with_capacity(2);
        for word in [words.first(), words.last()].into_iter().flatten() {
            if let Some(c) = word.chars().find(|c| c.is_alphanumeric()) {
                let upper = c.to_uppercase().next().unwrap_or(c);
                // Skip a repeat, so "Ana Ana" gives one letter rather than two the same.
                if !letters.contains(upper) {
                    letters.push(upper);
                }
            }
            if letters.chars().count() == 2 {
                break;
            }
        }
        letters
    }
}

/// A calendar event from Google, in local wall-clock time.
#[derive(Clone, Debug, PartialEq)]
pub struct CalendarEvent {
    pub summary: String,
    /// Days since 1970-01-01, local.
    pub day: i64,
    /// Minutes after midnight; `None` for an all-day event.
    pub minute: Option<u32>,
}

/// Work handed to a worker thread.
pub enum Step {
    /// Run the browser sign-in. The listener is bound before the browser opens, so the reply
    /// cannot be picked up by another program on the same port.
    Authorize { client: Client, verifier: String, state: String, listener: TcpListener },
    /// Refresh the access token, then read whichever parts are switched on.
    Refresh { client: Client, refresh: String, mail: bool, calendar: bool },
}

/// Every request the panel needs, in one round trip.
#[derive(Default, Debug)]
pub struct Snapshot {
    pub mail: Vec<Mail>,
    pub events: Vec<CalendarEvent>,
}

/// Result of a worker step.
pub enum Outcome {
    /// The refresh token, and the account's address if Google named it.
    Authorized { refresh: String, account: String },
    /// New data; `auth` = the stored token no longer works.
    Data { snapshot: Box<Snapshot>, auth: bool },
}

/// Start a worker step; the result arrives as [`Event::Google`].
pub fn spawn(step: Step, notify: Notifier) {
    let spawned = std::thread::Builder::new().name("google".into()).spawn(move || {
        let outcome = match step {
            Step::Authorize { client, verifier, state, listener } => authorize(&client, &verifier, &state, listener),
            Step::Refresh { client, refresh, mail, calendar } => read_all(&client, &refresh, mail, calendar),
        };
        match outcome {
            Ok(outcome) => notify.send(Event::Google(Ok(outcome))),
            Err(e) => {
                error_log!("google request failed: {e}");
                notify.send(Event::Google(Err(e.to_string())));
            }
        }
    });
    if let Err(e) = spawned {
        error_log!("failed to start google thread: {e}");
    }
}

/// A PKCE verifier and its S256 challenge, plus the state value.
pub fn pkce() -> (String, String) {
    let verifier = crate::system::secrets::random_b64(32);
    let challenge = crate::system::secrets::sha256_b64(verifier.as_bytes());
    (verifier, challenge)
}

/// Loopback port for the sign-in reply. The listener is bound before the browser opens, so the
/// port is known to be free and the reply cannot be picked up by another program.
pub fn listener() -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Build the consent URL. `access_type=offline` is what asks Google for a refresh token.
pub fn consent_url(client: &Client, challenge: &str, state: &str, port: u16) -> String {
    let query = http::form(&[
        ("client_id", &client.id),
        ("redirect_uri", &format!("http://127.0.0.1:{port}")),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ]);
    format!("{AUTH}?{query}")
}

/// Wait for Google's reply on the loopback listener and exchange the code for a refresh token.
fn authorize(client: &Client, verifier: &str, expected: &str, listener: TcpListener) -> Result<Outcome> {
    let port = listener.local_addr()?.port();
    let (code, state) = wait_for_code(listener)?;
    // A reply with no state, or the wrong one, did not come from the sign-in Crest started.
    if !state_matches(expected, &state) {
        return Err(windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "the sign-in reply was not for this request"));
    }
    let body = http::form(&[
        ("client_id", &client.id),
        ("client_secret", &client.secret),
        ("code", &code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", &format!("http://127.0.0.1:{port}")),
    ]);
    let answer = token_request(&body)?;
    let refresh = answer.GetNamedString(&HSTRING::from("refresh_token")).map(|s| s.to_string()).unwrap_or_default();
    if refresh.is_empty() {
        return Err(windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "Google sent no refresh token; remove Crest's access in your Google account and try again",
        ));
    }
    let account = account_name(&refresh).unwrap_or_default();
    Ok(Outcome::Authorized { refresh, account })
}

/// Wait for `GET /?code=…&state=…`, answer with a short page, and return the code.
fn wait_for_code(listener: TcpListener) -> Result<(String, String)> {
    wait_within(listener, CONSENT_TIMEOUT)
}

/// [`wait_for_code`] with an explicit deadline, so a test does not wait the real timeout.
fn wait_within(listener: TcpListener, within: Duration) -> Result<(String, String)> {
    // Polling instead of a blocking accept lets the loop notice its own deadline even when no
    // browser ever connects (Google refused the sign-in before redirecting).
    listener.set_nonblocking(true)?;
    // A browser that fails to load the page retries; the first request with a code wins.
    let deadline = Instant::now() + within;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        let (mut stream, _) = match listener.accept() {
            Ok(pair) => pair,
            // Nobody has connected yet: wait a little and check the deadline again.
            Err(_) => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut request = [0u8; 4096];
        let read = stream.read(&mut request).unwrap_or(0);
        let request = String::from_utf8_lossy(&request[..read]);
        let target = request.split_whitespace().nth(1).unwrap_or_default();
        let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
        let mut code = String::new();
        let mut state = String::new();
        let mut error = String::new();
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else { continue };
            let value = decode(value);
            match key {
                "code" => code = value,
                "state" => state = value,
                "error" => error = value,
                _ => {}
            }
        }
        let (status, line) = match (code.is_empty(), error.as_str()) {
            (false, _) => ("200 OK", "Crest is connected. You can close this tab."),
            (_, e) if !e.is_empty() => ("200 OK", "Crest was not connected."),
            _ => ("200 OK", "Waiting for Google…"),
        };
        let page = format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Crest</title>\
             <body style=\"font:15px 'Segoe UI',sans-serif;background:#0b0b0f;color:#fafafa;text-align:center;padding:80px\">\
             <p>{line}</p></body>"
        );
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        // The request line is one read of a fixed buffer, so a client that never sends the blank
        // line still ends here instead of looping.
        if !code.is_empty() {
            // A code is an opaque token: whitespace or a control character means the reply is
            // malformed, and it must not be put in a form body either.
            if malformed(&code) {
                return Err(windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "the sign-in reply was malformed"));
            }
            return Ok((code, state));
        }
        if !error.is_empty() {
            // The query is untrusted text: only a code-like value is passed on, so nothing
            // arbitrary (and no newline) reaches a log line.
            return Err(windows::core::Error::new(windows::Win32::Foundation::E_FAIL, reply_error(&error)));
        }
    }
    Err(windows::core::Error::new(windows::Win32::Foundation::E_FAIL, NO_REPLY))
}

/// The reply's state must be the one this sign-in generated: an empty state (a hand-written or
/// truncated request) never matches, even against an empty expectation.
fn state_matches(expected: &str, state: &str) -> bool {
    !state.is_empty() && state == expected
}

/// A code is an opaque token, so whitespace or a control character means the reply was
/// malformed (or hand-written); such a value must not reach a form body.
fn malformed(code: &str) -> bool {
    code.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Google's `error=` value, kept to the shape the OAuth spec uses (`access_denied`,
/// `invalid_scope`): anything else is replaced, so a hostile reply cannot choose the text.
fn reply_error(error: &str) -> String {
    let plausible = !error.is_empty() && error.len() <= 64 && error.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if plausible { format!("sign-in was refused: {error}") } else { "sign-in was refused".to_string() }
}

/// Percent-decoding for the reply's query string.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Exchange a token request body for the JSON answer.
///
/// Google answers a rejected request with a 400 and the reason in the body (`invalid_grant`,
/// `redirect_uri_mismatch`, `invalid_client`, …). That reason is the only useful thing on
/// screen, so it is read here instead of being flattened to "HTTP 400".
fn token_request(body: &str) -> Result<JsonObject> {
    let headers = [String::from("Content-Type: application/x-www-form-urlencoded")];
    let request = Request { method: "POST", url: TOKEN, headers: &headers, body: Some(body.as_bytes()) };
    let (status, raw) = http::exchange(&request, USER_AGENT)?;
    let text = String::from_utf8_lossy(&raw);
    let root = JsonObject::Parse(&HSTRING::from(text.as_ref()))
        .map_err(|_| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, format!("HTTP {status}, unreadable answer")))?;
    if !(200..300).contains(&status) {
        return Err(windows::core::Error::new(windows::Win32::Foundation::E_FAIL, token_error(&root, status)));
    }
    Ok(root)
}

/// Google's own explanation of a rejected token request, in one short line.
///
/// The values are server text, so they are cut down and stripped of control characters before
/// they can reach the log or the page.
fn token_error(answer: &JsonObject, status: u32) -> String {
    let field = |name: &str| {
        answer
            .GetNamedString(&HSTRING::from(name))
            .map(|value| value.to_string())
            .unwrap_or_default()
    };
    let code = field("error");
    let detail = field("error_description");
    // Kept short enough that "HTTP 400: <code> — <detail>" stays one readable line.
    let short = |text: &str| text.chars().filter(|c| !c.is_control()).take(48).collect::<String>();
    match (code.is_empty(), detail.is_empty()) {
        (false, false) => format!("HTTP {status}: {} — {}", short(&code), short(&detail)),
        (false, true) => format!("HTTP {status}: {}", short(&code)),
        // No explanation: the status is all there is.
        (true, _) => format!("HTTP {status}"),
    }
}

/// A fresh access token from the stored refresh token.
fn access_token(client: &Client, refresh: &str) -> Result<String> {
    let body = http::form(&[
        ("client_id", &client.id),
        ("client_secret", &client.secret),
        ("refresh_token", refresh),
        ("grant_type", "refresh_token"),
    ]);
    let answer = token_request(&body)?;
    answer
        .GetNamedString(&HSTRING::from("access_token"))
        .map(|s| s.to_string())
        .map_err(|_| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "no access token"))
}

/// The signed-in address, as Gmail's profile reports it. Best effort.
fn account_name(refresh: &str) -> Option<String> {
    let client = Client::load()?;
    let token = access_token(&client, refresh).ok()?;
    let headers = [format!("Authorization: Bearer {token}")];
    let url = "https://gmail.googleapis.com/gmail/v1/users/me/profile";
    let text = http::send(&Request { method: "GET", url, headers: &headers, body: None }, USER_AGENT).ok()?;
    let root = JsonObject::Parse(&HSTRING::from(&text)).ok()?;
    Some(root.GetNamedString(&HSTRING::from("emailAddress")).ok()?.to_string())
}

/// Refresh the token and read only the parts the user switched on. Nothing is fetched for a
/// page that is off, so turning Mail off stops Gmail requests entirely.
fn read_all(client: &Client, refresh: &str, mail: bool, calendar: bool) -> Result<Outcome> {
    let token = match access_token(client, refresh) {
        Ok(token) => token,
        // The stored token no longer works: say so once instead of retrying every minute.
        Err(_) => return Ok(Outcome::Data { snapshot: Box::default(), auth: true }),
    };
    let mut snapshot = Snapshot::default();
    // A failed read is not a failed sign-in; the other side still shows.
    if mail && let Err(e) = read_mail(&token, &mut snapshot) {
        error_log!("gmail read failed: {e}");
    }
    if calendar && let Err(e) = read_calendar(&token, &mut snapshot) {
        error_log!("calendar read failed: {e}");
    }
    Ok(Outcome::Data { snapshot: Box::new(snapshot), auth: false })
}

fn authorized<'a>(headers: &'a [String], url: &'a str) -> Request<'a> {
    Request { method: "GET", url, headers, body: None }
}

/// Unread messages: ids from the list call, then subject and sender per id.
fn read_mail(token: &str, snapshot: &mut Snapshot) -> Result<()> {
    let headers = [format!("Authorization: Bearer {token}")];
    let url = format!("{GMAIL}?q=is%3Aunread%20in%3Ainbox&maxResults={MAX_MAIL}");
    let text = http::send(&authorized(&headers, &url), USER_AGENT)?;
    let root = JsonObject::Parse(&HSTRING::from(&text)).ok();
    let Some(root) = root else { return Ok(()) };
    let Ok(messages) = root.GetNamedArray(&HSTRING::from("messages")) else { return Ok(()) };
    let count = messages.Size().unwrap_or(0).min(MAX_MAIL as u32);
    for i in 0..count {
        // One request per id, so the count is capped; the snapshot is capped again below in case
        // a lookup returns nothing.
        if snapshot.mail.len() >= MAX_MAIL {
            break;
        }
        let Ok(message) = messages.GetObjectAt(i) else { continue };
        let id = message.GetNamedString(&HSTRING::from("id")).map(|s| s.to_string()).unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        if let Some(mail) = read_one_mail(&headers, &id) {
            snapshot.mail.push(mail);
        }
    }
    Ok(())
}

/// One message's headers; the body is never asked for.
fn read_one_mail(headers: &[String], id: &str) -> Option<Mail> {
    let url = format!(
        "{GMAIL}/{id}?format=metadata&metadataHeaders[]=Subject&metadataHeaders[]=From&metadataHeaders[]=Date"
    );
    let text = http::send(&authorized(headers, &url), USER_AGENT).ok()?;
    let root = JsonObject::Parse(&HSTRING::from(&text)).ok()?;
    let payload = root.GetNamedObject(&HSTRING::from("payload")).ok()?;
    let headers_array = payload.GetNamedArray(&HSTRING::from("headers")).ok()?;
    let mut subject = String::new();
    let mut from = String::new();
    let mut date = String::new();
    for i in 0..headers_array.Size().unwrap_or(0) {
        let Ok(field) = headers_array.GetObjectAt(i) else { continue };
        let name = field.GetNamedString(&HSTRING::from("name")).map(|s| s.to_string()).unwrap_or_default();
        let value = field.GetNamedString(&HSTRING::from("value")).map(|s| s.to_string()).unwrap_or_default();
        match name.to_lowercase().as_str() {
            "subject" => subject = value,
            "from" => from = value,
            "date" => date = value,
            _ => {}
        }
    }
    let from = sender_name(&from);
    let initials = Mail::initials(&from);
    Some(Mail { id: id.to_string(), from, subject: clean(&subject, 60), age: age(&date), initials })
}

/// "Ana Silva <ana@example.com>" → "Ana Silva"; a bare address stays as it is.
fn sender_name(from: &str) -> String {
    let from = from.trim();
    let name = match from.rsplit_once('<') {
        Some((name, _)) => name.trim().trim_matches('"').trim(),
        None => from,
    };
    let name = if name.is_empty() { from.trim_matches(|c| c == '<' || c == '>') } else { name };
    clean(name, 24)
}

/// "12m" for a message this recent, from its RFC 5322 `Date` header.
fn age(date: &str) -> String {
    let Some(stamp) = crate::system::datetime::parse_rfc5322(date) else { return String::new() };
    let now = crate::system::datetime::now_seconds();
    let secs = (now - stamp).max(0) as u64;
    match secs {
        0..=59 => "now".to_string(),
        60..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// The next two weeks of events from the primary calendar.
fn read_calendar(token: &str, snapshot: &mut Snapshot) -> Result<()> {
    let now = crate::system::datetime::now_rfc3339();
    let later = crate::system::datetime::in_days_rfc3339(CALENDAR_DAYS);
    let headers = [format!("Authorization: Bearer {token}")];
    let url = format!(
        "{CALENDAR}?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults={CALENDAR_MAX}",
        http::encode(&now),
        http::encode(&later)
    );
    let text = http::send(&authorized(&headers, &url), USER_AGENT)?;
    let Some(root) = JsonObject::Parse(&HSTRING::from(&text)).ok() else { return Ok(()) };
    let Ok(items) = root.GetNamedArray(&HSTRING::from("items")) else { return Ok(()) };
    for i in 0..items.Size().unwrap_or(0) {
        // The page size is a request, not a promise: a server may send more, and the rest is
        // dropped rather than shown.
        if snapshot.events.len() >= MAX_EVENTS {
            break;
        }
        let Ok(item) = items.GetObjectAt(i) else { continue };
        let summary = item.GetNamedString(&HSTRING::from("summary")).map(|s| s.to_string()).unwrap_or_default();
        let Ok(start) = item.GetNamedObject(&HSTRING::from("start")) else { continue };
        // "date" for an all-day event, "dateTime" otherwise.
        if let Ok(date) = start.GetNamedString(&HSTRING::from("date"))
            && let Some(day) = crate::system::datetime::day(&date.to_string())
        {
            snapshot.events.push(CalendarEvent { summary: clean(&summary, 60), day, minute: None });
        } else if let Ok(stamp) = start.GetNamedString(&HSTRING::from("dateTime"))
            && let Some((day, minute)) = crate::system::datetime::local_moment(&stamp.to_string())
        {
            snapshot.events.push(CalendarEvent { summary: clean(&summary, 60), day, minute: Some(minute) });
        }
    }
    Ok(())
}

/// One line of readable text.
fn clean(text: &str, max: usize) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// Opening the browser for sign-in: the URL is handed to the shell.
pub fn open_browser(url: &str) {
    crate::system::dnd::open_url(url);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_consent_url() {
        let client = Client { id: "123.apps.googleusercontent.com".into(), secret: "s".into() };
        let url = consent_url(&client, "CHALLENGE", "STATE", 51234);
        assert!(url.starts_with(AUTH));
        assert!(url.contains("client_id=123.apps.googleusercontent.com"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A51234"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("gmail.readonly"));
        assert!(url.contains("calendar.readonly"));
    }

    #[test]
    fn reads_the_client_file() {
        assert!(Client::parse_stored("nothing here").is_none());
        let client = Client::parse_stored("client-id=abc\nclient-secret=def\n").unwrap();
        assert_eq!(client, Client { id: "abc".into(), secret: "def".into() });
    }

    #[test]
    fn sender_initials_are_short_and_stable() {
        assert_eq!(Mail::initials("Ana Silva"), "AS");
        assert_eq!(Mail::initials("GitHub"), "G");
        assert_eq!(Mail::initials("noreply"), "N");
        assert_eq!(Mail::initials("   "), "");
        // A repeated first letter is not doubled.
        assert_eq!(Mail::initials("Ana Ana"), "A");
        // An address in angle brackets keeps its first two letters: the first and last word are
        // the same one, so only one letter is taken from each end of it.
        assert_eq!(Mail::initials("<ana@example.com>"), "A");
    }

    #[test]
    fn splits_senders_and_ages() {
        assert_eq!(sender_name("Ana Silva <ana@example.com>"), "Ana Silva");
        assert_eq!(sender_name("\"GitHub\" <noreply@github.com>"), "GitHub");
        assert_eq!(sender_name("ana@example.com"), "ana@example.com");
        assert_eq!(sender_name("<only@example.com>"), "only@example.com");
        assert_eq!(age("not a date"), "");
    }

    #[test]
    fn decodes_the_reply_query() {
        assert_eq!(decode("4%2F0Ab%3D"), "4/0Ab=");
        assert_eq!(decode("a+b"), "a b");
        assert_eq!(decode("%zz"), "%zz");
    }

    #[test]
    fn rejects_a_code_that_cannot_be_a_token() {
        assert!(!malformed("4/0AbC"));
        // Whitespace and control characters are not part of an authorization code.
        assert!(malformed("4/0Ab C"));
        assert!(malformed("4/0Ab\nC"));
        assert!(malformed("4/0Ab\tC"));
        assert!(malformed("4/0Ab\u{7}C"));
    }

    #[test]
    fn keeps_only_a_shaped_refusal_reason() {
        assert_eq!(reply_error("access_denied"), "sign-in was refused: access_denied");
        assert_eq!(reply_error("invalid_scope"), "sign-in was refused: invalid_scope");
        // Anything else is dropped rather than echoed back into a status line.
        assert_eq!(reply_error("<script>"), "sign-in was refused");
        assert_eq!(reply_error("two words"), "sign-in was refused");
        assert_eq!(reply_error(&"a".repeat(65)), "sign-in was refused");
        assert_eq!(reply_error(""), "sign-in was refused");
    }

    #[test]
    fn an_empty_state_never_matches() {
        assert!(state_matches("STATE", "STATE"));
        assert!(!state_matches("STATE", "OTHER"));
        // A reply with no state must be rejected even if the expected value were somehow empty.
        assert!(!state_matches("", ""));
        assert!(!state_matches("STATE", ""));
    }
}


#[cfg(test)]
mod error_checks {
    use super::*;

    /// Google's reason for a rejected exchange reaches the page instead of a bare status.
    #[test]
    fn reports_googles_reason_for_a_rejected_exchange() {
        let answer = JsonObject::Parse(&HSTRING::from(
            r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
        ))
        .unwrap();
        assert_eq!(token_error(&answer, 400), "HTTP 400: invalid_grant — Bad Request");
        // Only a code.
        let answer = JsonObject::Parse(&HSTRING::from(r#"{"error":"invalid_client"}"#)).unwrap();
        assert_eq!(token_error(&answer, 401), "HTTP 401: invalid_client");
        // Nothing readable: the status is all there is, and no panic on an empty answer.
        assert_eq!(token_error(&JsonObject::new().unwrap(), 500), "HTTP 500");
        // Server text is cut down and cannot smuggle control characters into a log line.
        let answer = JsonObject::Parse(&HSTRING::from(format!(
            r#"{{"error":"e{}\n"}}"#,
            "x".repeat(300)
        )))
        .unwrap();
        let message = token_error(&answer, 400);
        assert!(message.len() <= 120, "{}", message.len());
        assert!(!message.contains('\n'));
    }

    /// `crest_agent`-style validation: an empty state never matches, even an empty expectation.
    #[test]
    fn a_reply_must_carry_the_state_of_this_sign_in() {
        assert!(state_matches("abc", "abc"));
        assert!(!state_matches("abc", "abd"));
        assert!(!state_matches("", ""));
        assert!(!state_matches("abc", ""));
    }
}

#[cfg(test)]
mod flow_checks {
    use super::*;
    use std::net::TcpStream;

    /// A refused sign-in must not hang the worker until the timeout: the loop keeps noticing its
    /// own deadline while no browser ever connects.
    #[test]
    fn waiting_ends_when_nobody_connects() {
        let (listener, _port) = listener().unwrap();
        let started = Instant::now();
        // A short stand-in deadline, so the test does not wait the real two minutes.
        let result = wait_within(listener, Duration::from_millis(400));
        assert!(result.is_err(), "no reply must end as an error, not as a code");
        assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
    }

    /// A browser that reaches the listener is still served normally.
    #[test]
    fn a_real_reply_is_still_read() {
        let (listener, port) = listener().unwrap();
        let reply = std::thread::spawn(move || wait_within(listener, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(120));
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        std::io::Write::write_all(
            &mut stream,
            b"GET /?state=S1&code=4%2F0AbC HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
        assert_eq!(reply.join().unwrap().unwrap(), ("4/0AbC".to_string(), "S1".to_string()));
    }
}
