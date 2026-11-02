//! Tako (tako.id) overlay: donations and the creator's overlay widgets, read the way the browser
//! source in OBS reads them.
//!
//! The creator pastes the overlay link from the Tako dashboard; only its `overlay_key` is kept,
//! encrypted for this Windows user (`tako.txt`, DPAPI), and never logged. Two parts, both the
//! ones the overlay itself uses:
//!
//! - **Realtime:** a Pusher-protocol relay at `wss://relay.tako.id/app/tako`, public channel
//!   `overlay.<key>`. Its `messages` event only says "something changed" (the payload is `true`);
//!   `command` carries the dashboard's controls, of which only "alert.test" matters here.
//! - **Data:** `GET https://tako.id/api/v1/overlay/<key>` with `X-Path` naming the overlay. The
//!   alert queue answers one unplayed donation at a time, skipping the ids sent in
//!   `X-Queued-Gift-Ids`, or 206 when there is none; leaderboard and milestone answer their state.
//!
//! Crest only reads. It never sends the `PUT` that marks a donation as played, so the alert in
//! OBS still plays every donation; Crest walks the queue by remembering the ids it has seen.
//! Donations already waiting when Crest connects are recorded without a notice.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Data::Json::{JsonObject, JsonValue, JsonValueType};
use windows::core::HSTRING;

use crate::shell::store;
use crate::system::http::{self, Request, WebSocket};
use crate::system::services::{Event, Notifier};

const HOST: &str = "https://tako.id";
const RELAY_HOST: &str = "relay.tako.id";
/// Pusher protocol 7, as pusher-js 8 speaks it; "tako" is the relay's public app key.
const RELAY_PATH: &str = "/app/tako?protocol=7&client=crest&version=0.1.0&flash=false";
const KEY_FILE: &str = "tako.txt";
const USER_AGENT: &str = concat!("Crest/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rinci-labs/crest)");
/// The overlay paths read here. "alert-mediashare" answers alerts and media shares in one queue;
/// soundboard is a queue of its own. The rest answer their current state.
///
/// Gacha is left out on purpose: reading it spends the creator's gacha balance into queued spins
/// (the overlay's read is what rolls the wheel), so it is not a read at all.
const ALERTS: &str = "alert-mediashare";
const SOUNDBOARD: &str = "soundboard";
const LEADERBOARD: &str = "leaderboard";
const MILESTONE: &str = "milestone";
const SONGSHARE: &str = "songshare";
const POLLING: &str = "polling";
const TIMER: &str = "timer";
const VIP_QUEUE: &str = "vipqueue";
/// The queue header must not be empty; the overlay sends this id when it has none.
const NO_IDS: &str = "00000000-0000-0000-0000-000000000000";
/// The relay asks for activity every 30 s; a ping a little earlier keeps the socket open.
const PING_EVERY: Duration = Duration::from_secs(25);
/// How often the pinger looks at the stop flag.
const STOP_POLL: Duration = Duration::from_millis(250);
/// Widgets are read again this often even without a realtime event, as the overlay does.
const REFRESH_EVERY: Duration = Duration::from_secs(5 * 60);
/// Wait before reconnecting, doubling up to the maximum.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);
/// Ids remembered for the queue header: 37 bytes each, so the header stays near 5 KB.
const MAX_SEEN: usize = 150;
/// Queue reads per wake; at start more, to step over donations that were already waiting.
const DRAIN_WAKE: usize = 10;
const DRAIN_START: usize = 40;
/// Rows kept per list: leaderboard, song queue, poll options, VIP queue.
pub const BOARD_ROWS: usize = 10;
const MAX_NAME: usize = 40;
const MAX_MESSAGE: usize = 160;

/// The overlay key from a pasted link (`…/overlay/alert?overlay_key=<key>` or the path form
/// `…/overlay/alert/<key>`) or a bare key. Keys are cuid2: lowercase letters and digits.
pub fn parse_key(input: &str) -> Option<String> {
    let input = input.trim();
    let candidate = match input.split_once("overlay_key=") {
        Some((_, rest)) => rest.split(['&', '#']).next().unwrap_or(""),
        None if input.contains('/') => input.trim_end_matches('/').rsplit('/').next().unwrap_or(""),
        None => input,
    };
    let valid = (20..=32).contains(&candidate.len()) && candidate.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    valid.then(|| candidate.to_string())
}

/// The saved overlay key.
pub fn key() -> Option<String> {
    let text = store::load(KEY_FILE).into_iter().next()?;
    crate::system::secrets::unprotect(&text).filter(|key| parse_key(key).is_some())
}

pub fn save_key(key: &str) {
    match crate::system::secrets::protect(key) {
        Some(text) => store::save(KEY_FILE, [text]),
        None => error_log!("cannot protect the Tako key"),
    }
}

pub fn forget() {
    let _ = store::dir().map(|dir| std::fs::remove_file(dir.join(KEY_FILE)));
}

/// One donation (alert or media share).
#[derive(Clone, Debug, PartialEq)]
pub struct Gift {
    pub id: String,
    pub name: String,
    /// Rupiah.
    pub amount: u64,
    pub message: String,
    /// A gift unit replaces the amount on the alert, e.g. ("Mawar", 10).
    pub unit: Option<(String, u32)>,
    /// The video of a media share.
    pub media: Option<Media>,
    /// The sound a soundboard donation plays.
    pub sound: Option<String>,
    /// From the dashboard's Test button; never stored by Tako.
    pub test: bool,
}

/// The video a media share asks the creator to play.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    /// "YouTube", "TikTok", or "Reels".
    pub platform: &'static str,
    /// Video title as Tako looked it up; may be empty.
    pub title: String,
    /// Where the video plays, `https` only; empty when none could be made.
    pub url: String,
    /// Tako's filter flagged it: the title is not shown.
    pub flagged: bool,
}

impl Media {
    /// One line for a notice or a row: "YouTube · Title", or just the platform.
    pub fn line(&self) -> String {
        match (self.flagged, self.title.is_empty()) {
            (true, _) => format!("{} · flagged by Tako's filter", self.platform),
            (false, true) => format!("{} video", self.platform),
            (false, false) => format!("{} · {}", self.platform, self.title),
        }
    }
}

impl Gift {
    /// What was given: "Rp 10.000", or "10× Mawar" for a gift unit.
    pub fn value(&self) -> String {
        match &self.unit {
            Some((name, quantity)) if !name.is_empty() => format!("{quantity}× {name}"),
            _ => rupiah(self.amount),
        }
    }

    /// One or two letters for the avatar, from the first and last word ("@" dropped).
    pub fn initials(&self) -> String {
        let words: Vec<&str> = self.name.trim_start_matches('@').split_whitespace().collect();
        let first = |word: &str| word.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
        match words.as_slice() {
            [] => "?".to_string(),
            [one] => first(one),
            [head, .., tail] => first(head) + &first(tail),
        }
    }
}

/// "Rp 1.250.000": Indonesian grouping with dots.
pub fn rupiah(amount: u64) -> String {
    let digits = amount.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    format!("Rp {out}")
}

/// One leaderboard row.
#[derive(Clone, Debug, PartialEq)]
pub struct Rank {
    pub name: String,
    pub amount: u64,
}

/// The milestone step being worked towards.
#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    /// Step title from the dashboard; may be empty.
    pub title: String,
    pub current: u64,
    pub target: u64,
}

impl Goal {
    /// Share reached, 0..1.
    pub fn fraction(&self) -> f32 {
        if self.target == 0 { 1.0 } else { (self.current as f64 / self.target as f64).clamp(0.0, 1.0) as f32 }
    }
}

/// One song request waiting in the song share queue, in play order.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub title: String,
    pub artist: String,
    /// Who asked for it.
    pub sender: String,
    /// Seconds; 0 when unknown.
    pub duration: u32,
    /// The YouTube video it plays, `https` only; may be empty.
    pub url: String,
}

/// One option of the running poll.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub name: String,
    /// Rupiah given for it.
    pub amount: u64,
    /// Share of all votes, 0..100, as Tako rounds it.
    pub percent: u32,
}

/// The overlay countdown, as Tako stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Countdown {
    /// When it runs out, Unix milliseconds; 0 when it is not running.
    pub ends_at: u64,
    /// Seconds set for it, shown while it is not running.
    pub duration: u64,
}

impl Countdown {
    /// Seconds left at `now` (Unix milliseconds); the set duration while it is not running.
    pub fn left(&self, now: u64) -> u64 {
        if self.ends_at == 0 { self.duration } else { self.ends_at.saturating_sub(now).div_ceil(1000) }
    }

    pub fn running(&self, now: u64) -> bool {
        self.ends_at > now
    }
}

/// One entry in the VIP queue.
#[derive(Clone, Debug, PartialEq)]
pub struct Vip {
    pub name: String,
    pub amount: u64,
    /// What the supporter filled in ("Game ID: 1234"), joined into one line.
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    /// Subscribed to the channel: donations arrive as they happen.
    Live,
    /// Tako or the relay cannot be reached; retrying.
    Offline,
    /// Tako does not know the key (reset in the dashboard, or mistyped). Not retried.
    Rejected,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Connecting => "Connecting…",
            Status::Live => "Live",
            Status::Offline => "Offline · retrying",
            Status::Rejected => "Key not accepted",
        }
    }
}

/// What the listener reports, through [`Event::Tako`].
pub enum Update {
    Status(Status),
    Gift(Gift),
    Board(Vec<Rank>),
    Goal(Option<Goal>),
    Songs(Vec<Song>),
    Poll(Vec<Choice>),
    Countdown(Option<Countdown>),
    Vips(Vec<Vip>),
}

/// Background connection to the overlay; dropping it stops the thread.
pub struct Listener {
    stop: Arc<AtomicBool>,
}

impl Listener {
    pub fn start(key: String, notify: Notifier) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let spawned = std::thread::Builder::new().name("tako".into()).spawn(move || run(&key, &flag, &notify));
        if let Err(e) = spawned {
            error_log!("failed to start tako thread: {e}");
        }
        Self { stop }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Why a read failed.
enum Failure {
    /// 404: the key is unknown.
    Rejected,
    Network,
}

/// What the thread remembers between reads.
struct Reader<'a> {
    key: &'a str,
    /// Queue ids already seen, oldest first.
    seen: VecDeque<String>,
    /// `_sharedDataVersion` per overlay: echoed back, so Tako skips resending ~50 KB of settings.
    versions: Vec<(&'static str, String)>,
    refreshed: Instant,
}

fn run(key: &str, stop: &Arc<AtomicBool>, notify: &Notifier) {
    let send = |update| notify.send(Event::Tako(update));
    let mut reader = Reader { key, seen: VecDeque::new(), versions: Vec::new(), refreshed: Instant::now() };
    let mut retry = RETRY_FIRST;
    let mut started = false;
    while !stop.load(Ordering::Relaxed) {
        send(Update::Status(Status::Connecting));
        // The widgets and the queue are read before the socket opens, which also checks the key.
        let first = reader.refresh(if started { DRAIN_WAKE } else { DRAIN_START }, !started, &send);
        match first {
            Err(Failure::Rejected) => {
                send(Update::Status(Status::Rejected));
                return;
            }
            Err(Failure::Network) => {}
            Ok(()) => {
                started = true;
                if let Ok(socket) = WebSocket::connect(RELAY_HOST, RELAY_PATH, USER_AGENT) {
                    retry = RETRY_FIRST;
                    listen(socket, &mut reader, stop, &send);
                }
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        send(Update::Status(Status::Offline));
        sleep(retry, stop);
        retry = (retry * 2).min(RETRY_MAX);
    }
}

/// Wait, but wake up early when asked to stop.
fn sleep(duration: Duration, stop: &AtomicBool) {
    let until = Instant::now() + duration;
    while Instant::now() < until && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(STOP_POLL);
    }
}

/// Read relay messages until the socket closes or the listener is stopped.
fn listen(socket: WebSocket, reader: &mut Reader, stop: &Arc<AtomicBool>, send: &dyn Fn(Update)) {
    let socket = Arc::new(socket);
    let done = Arc::new(AtomicBool::new(false));
    // `receive` blocks; this thread keeps the line alive and closes it to end the wait.
    let pinger = {
        let (socket, done, stop) = (socket.clone(), done.clone(), stop.clone());
        std::thread::Builder::new().name("tako-ping".into()).spawn(move || {
            let mut last = Instant::now();
            loop {
                std::thread::sleep(STOP_POLL);
                if stop.load(Ordering::Relaxed) || done.load(Ordering::Relaxed) {
                    socket.close();
                    return;
                }
                if last.elapsed() >= PING_EVERY {
                    last = Instant::now();
                    if socket.send(r#"{"event":"pusher:ping","data":{}}"#).is_err() {
                        socket.close();
                        return;
                    }
                }
            }
        })
    };
    let channel = format!("overlay.{}", reader.key);
    while let Some(text) = socket.receive() {
        let Some((event, data)) = frame(&text) else { continue };
        match event.as_str() {
            "pusher:connection_established" => {
                let subscribe = format!(r#"{{"event":"pusher:subscribe","data":{{"auth":"","channel":"{channel}"}}}}"#);
                if socket.send(&subscribe).is_err() {
                    break;
                }
            }
            "pusher_internal:subscription_succeeded" => send(Update::Status(Status::Live)),
            "pusher:ping" => {
                let _ = socket.send(r#"{"event":"pusher:pong","data":{}}"#);
            }
            "pusher:error" => break,
            "messages" => {
                if let Err(Failure::Rejected) = reader.refresh(DRAIN_WAKE, false, send) {
                    send(Update::Status(Status::Rejected));
                    break;
                }
            }
            "command" => {
                if let Some(gift) = test_gift(&data) {
                    send(Update::Gift(gift));
                }
            }
            _ => {}
        }
        if reader.refreshed.elapsed() >= REFRESH_EVERY {
            let _ = reader.refresh(DRAIN_WAKE, false, send);
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
    done.store(true, Ordering::Relaxed);
    if let Ok(pinger) = pinger {
        let _ = pinger.join();
    }
}

/// Event name and data of one Pusher frame. Channel events carry their data JSON-encoded inside
/// a string ("true", "\"alert.test\""); a string inside is unwrapped once more.
fn frame(text: &str) -> Option<(String, String)> {
    let root = JsonObject::Parse(&HSTRING::from(text)).ok()?;
    let event = root.GetNamedString(&HSTRING::from("event")).ok()?.to_string();
    let data = match root.GetNamedValue(&HSTRING::from("data")) {
        Ok(value) if value.ValueType().ok() == Some(JsonValueType::String) => value.GetString().map(|s| s.to_string()).unwrap_or_default(),
        Ok(value) => value.Stringify().map(|s| s.to_string()).unwrap_or_default(),
        Err(_) => String::new(),
    };
    let inner = JsonValue::Parse(&HSTRING::from(data.as_str()))
        .ok()
        .filter(|value| value.ValueType().ok() == Some(JsonValueType::String))
        .and_then(|value| value.GetString().ok())
        .map(|s| s.to_string());
    Some((event, inner.unwrap_or(data)))
}

/// The dashboard's Test button for the alert or media share: "<overlay>.test[.variant]". The
/// overlay builds the test donation itself, so Crest does too, with the same sender, amount,
/// and (for a media share) the same sample video.
fn test_gift(command: &str) -> Option<Gift> {
    let mut parts = command.split('.');
    let (target, action) = (parts.next()?, parts.next()?);
    (action == "test" && matches!(target, "alert" | "mediashare")).then(|| Gift {
        id: String::new(),
        name: "Tako".to_string(),
        amount: 10_000,
        message: "Test notification from the Tako dashboard".to_string(),
        unit: None,
        media: (target == "mediashare").then(|| Media {
            platform: "YouTube",
            title: "Test video".to_string(),
            url: video_url("youtube", "mm0YkIJZJ4w", 0),
            flagged: false,
        }),
        sound: None,
        test: true,
    })
}

/// Where a media share plays: Tako's own video URL when it gave one, otherwise built from the
/// platform and id. Only `https` addresses on the platform's own host are returned.
fn video_url(kind: &str, id: &str, start: u64) -> String {
    let safe = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    match kind {
        "youtube" if safe => {
            let at = if start > 0 { format!("&t={start}s") } else { String::new() };
            format!("https://www.youtube.com/watch?v={id}{at}")
        }
        "reels" if safe => format!("https://www.instagram.com/reel/{id}/"),
        _ => String::new(),
    }
}

/// The video of a media share result, from its `media` object.
fn media(result: &JsonObject) -> Option<Media> {
    let media = object(result, "media")?;
    let kind = string(&media, "type");
    let platform = match kind.as_str() {
        "youtube" => "YouTube",
        "tiktok" => "TikTok",
        "reels" => "Reels",
        _ => "Video",
    };
    let data = object(&media, "data");
    let title = data.as_ref().map(|d| clean(&string(d, "title"), MAX_MESSAGE)).unwrap_or_default();
    // Tako's link wins when it is a plain https address; anything else is rebuilt from the id.
    let given = data.as_ref().map(|d| string(d, "videoUrl")).unwrap_or_default();
    let url = if given.starts_with("https://") && !given.contains(char::is_whitespace) {
        given
    } else {
        video_url(&kind, &string(&media, "id"), number(&media, "startTime"))
    };
    let flagged = media.GetNamedBoolean(&HSTRING::from("isNSFW")).unwrap_or(false);
    Some(Media { platform, title, url, flagged })
}

impl Reader<'_> {
    /// Walk the alert queue (up to `drain` reads) and read the leaderboard and milestone. New
    /// donations are reported unless `quiet` (the reads at start, which only record ids).
    fn refresh(&mut self, drain: usize, quiet: bool, send: &dyn Fn(Update)) -> Result<(), Failure> {
        self.refreshed = Instant::now();
        self.drain(ALERTS, drain, quiet, send)?;
        self.drain(SOUNDBOARD, drain, quiet, send)?;
        if let Some(result) = self.read(LEADERBOARD)? {
            send(Update::Board(board(&result)));
        }
        if let Some(result) = self.read(MILESTONE)? {
            send(Update::Goal(goal(&result)));
        }
        if let Some(result) = self.read(SONGSHARE)? {
            send(Update::Songs(songs(&result)));
        }
        if let Some(result) = self.read(POLLING)? {
            send(Update::Poll(poll(&result)));
        }
        if let Some(result) = self.read(TIMER)? {
            send(Update::Countdown(countdown(&result)));
        }
        if let Some(result) = self.read(VIP_QUEUE)? {
            send(Update::Vips(vips(&result)));
        }
        Ok(())
    }

    /// Walk one donation queue (alerts or soundboard), up to `limit` reads.
    fn drain(&mut self, overlay: &'static str, limit: usize, quiet: bool, send: &dyn Fn(Update)) -> Result<(), Failure> {
        for _ in 0..limit {
            let Some(result) = self.read(overlay)? else { break };
            let Some(gift) = gift(&result) else { break };
            // An id seen before means the queue is not moving past it; reading again would loop.
            // Ids go back to Tako in a request header, so only a well-formed uuid is kept.
            if !is_uuid(&gift.id) || self.seen.contains(&gift.id) {
                break;
            }
            self.seen.push_back(gift.id.clone());
            if self.seen.len() > MAX_SEEN {
                self.seen.pop_front();
            }
            if !quiet {
                send(Update::Gift(gift));
            }
        }
        Ok(())
    }

    /// One overlay read: `Some(result)` on 200, `None` on 206 (nothing queued).
    fn read(&mut self, overlay: &'static str) -> Result<Option<JsonObject>, Failure> {
        let ids = if self.seen.is_empty() { NO_IDS.to_string() } else { self.seen.iter().map(String::as_str).collect::<Vec<_>>().join(",") };
        let version = self.versions.iter().find(|(name, _)| *name == overlay).map(|(_, v)| v.clone()).unwrap_or_default();
        let headers = [
            format!("X-Overlay-Key: {}", self.key),
            format!("X-Path: /overlay/{overlay}"),
            format!("X-Queued-Gift-Ids: {ids}"),
            format!("X-Shared-Data-Version: {version}"),
            // Tako refuses overlay reads without a referrer; this is the page the overlay runs on.
            format!("Referer: {HOST}/overlay/{overlay}"),
        ];
        let url = format!("{HOST}/api/v1/overlay/{}", self.key);
        let request = Request { method: "GET", url: &url, headers: &headers, body: None };
        let (status, body) = http::exchange(&request, USER_AGENT).map_err(|_| Failure::Network)?;
        match status {
            404 => return Err(Failure::Rejected),
            200 | 206 => {}
            _ => return Err(Failure::Network),
        }
        let text = String::from_utf8_lossy(&body);
        let root = JsonObject::Parse(&HSTRING::from(text.as_ref())).map_err(|_| Failure::Network)?;
        let result = root.GetNamedObject(&HSTRING::from("result")).map_err(|_| Failure::Network)?;
        // Echoed back in a header: only a short base64url token is taken.
        let fresh = string(&result, "_sharedDataVersion");
        let token = fresh.len() <= 64 && fresh.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !fresh.is_empty() && token {
            self.versions.retain(|(name, _)| *name != overlay);
            self.versions.push((overlay, fresh));
        }
        Ok((status == 200).then_some(result))
    }
}

/// A queue id as Tako's database stores it: 8-4-4-4-12 hex digits.
fn is_uuid(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.len() == 5
        && parts.iter().zip([8, 4, 4, 4, 12]).all(|(part, len)| part.len() == len && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn string(object: &JsonObject, name: &str) -> String {
    object.GetNamedString(&HSTRING::from(name)).map(|s| s.to_string()).unwrap_or_default()
}

fn number(object: &JsonObject, name: &str) -> u64 {
    object.GetNamedNumber(&HSTRING::from(name)).map(|n| n.max(0.0) as u64).unwrap_or(0)
}

fn object(parent: &JsonObject, name: &str) -> Option<JsonObject> {
    parent.GetNamedObject(&HSTRING::from(name)).ok()
}

/// Text for one line: control characters removed, whitespace collapsed, cut at `max` characters.
fn clean(text: &str, max: usize) -> String {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = text.chars().filter(|c| !c.is_control()).take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// Custom stickers are written `[@<id>]` in the message; Crest has no image for them.
fn strip_stickers(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find("[@") {
        out.push_str(&rest[..start]);
        match rest[start..].find(']') {
            Some(end) => {
                out.push_str("[sticker]");
                rest = &rest[start + end + 1..];
            }
            None => {
                rest = &rest[start..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A donation from an alert-queue result.
fn gift(result: &JsonObject) -> Option<Gift> {
    let meta = object(result, "$")?;
    let sender = object(result, "sender");
    let name = sender.as_ref().map(|s| string(s, "name")).unwrap_or_default();
    let unit = object(result, "unit").map(|unit| (clean(&string(&unit, "name"), MAX_NAME), number(&unit, "quantity").max(1) as u32));
    Some(Gift {
        id: string(&meta, "id"),
        name: if name.trim().is_empty() { "Someone".to_string() } else { clean(&name, MAX_NAME) },
        amount: number(result, "amount"),
        message: clean(&strip_stickers(&string(result, "message")), MAX_MESSAGE),
        unit,
        media: media(result),
        sound: object(result, "sound").map(|sound| clean(&string(&sound, "name"), MAX_NAME)),
        test: false,
    })
}

/// The sender's name of a queue or list entry.
fn sender(row: &JsonObject, field: &str) -> String {
    let name = object(row, field).map(|s| string(&s, "name")).unwrap_or_default();
    if name.trim().is_empty() { "Someone".to_string() } else { clean(&name, MAX_NAME) }
}

/// Objects of the array `name`, at most `BOARD_ROWS`.
fn rows(result: &JsonObject, name: &str) -> Vec<JsonObject> {
    let Ok(array) = result.GetNamedArray(&HSTRING::from(name)) else { return Vec::new() };
    (0..array.Size().unwrap_or(0)).filter_map(|i| array.GetObjectAt(i).ok()).take(BOARD_ROWS).collect()
}

/// The song share queue, in play order.
fn songs(result: &JsonObject) -> Vec<Song> {
    rows(result, "queues")
        .iter()
        .map(|song| Song {
            title: clean(&string(song, "title"), MAX_MESSAGE),
            artist: clean(&string(song, "artist"), MAX_NAME),
            sender: sender(song, "sender"),
            duration: number(song, "duration").min(u32::MAX as u64) as u32,
            url: video_url("youtube", &string(song, "mediaId"), 0),
        })
        .collect()
}

/// The poll's options with their share.
fn poll(result: &JsonObject) -> Vec<Choice> {
    rows(result, "options")
        .iter()
        .map(|option| Choice {
            name: clean(&string(option, "name"), MAX_NAME),
            amount: number(option, "totalAmount"),
            percent: number(option, "percentage").min(100) as u32,
        })
        .collect()
}

/// The countdown; `None` when it has neither an end nor a duration.
fn countdown(result: &JsonObject) -> Option<Countdown> {
    let timer = Countdown { ends_at: number(result, "timerEndAt"), duration: number(result, "timerDuration") };
    (timer.ends_at > 0 || timer.duration > 0).then_some(timer)
}

/// The VIP queue, in the order the creator serves it.
fn vips(result: &JsonObject) -> Vec<Vip> {
    rows(result, "entries")
        .iter()
        .map(|entry| {
            let fields = entry.GetNamedArray(&HSTRING::from("fields")).ok();
            let detail = fields
                .map(|fields| {
                    (0..fields.Size().unwrap_or(0))
                        .filter_map(|i| fields.GetObjectAt(i).ok())
                        .map(|f| format!("{}: {}", string(&f, "label"), string(&f, "value")))
                        .collect::<Vec<_>>()
                        .join(" · ")
                })
                .unwrap_or_default();
            Vip { name: sender(entry, "sender"), amount: number(entry, "amount"), detail: clean(&detail, MAX_MESSAGE) }
        })
        .collect()
}

/// Leaderboard rows from `rankings`.
fn board(result: &JsonObject) -> Vec<Rank> {
    let Ok(rankings) = result.GetNamedArray(&HSTRING::from("rankings")) else { return Vec::new() };
    (0..rankings.Size().unwrap_or(0))
        .filter_map(|i| rankings.GetObjectAt(i).ok())
        .map(|row| {
            let name = object(&row, "person").map(|p| string(&p, "name")).unwrap_or_default();
            Rank { name: clean(&name, MAX_NAME), amount: number(&row, "amount") }
        })
        .take(BOARD_ROWS)
        .collect()
}

/// The first milestone step not reached yet, or the last one once all are.
fn goal(result: &JsonObject) -> Option<Goal> {
    let current = number(result, "currentAmount");
    let steps = result.GetNamedArray(&HSTRING::from("steps")).ok()?;
    let steps: Vec<(String, u64)> = (0..steps.Size().unwrap_or(0))
        .filter_map(|i| steps.GetObjectAt(i).ok())
        .map(|step| (clean(&string(&step, "title"), MAX_NAME), number(&step, "target")))
        .collect();
    let (title, target) = steps.iter().find(|(_, target)| *target > current).or(steps.last())?.clone();
    Some(Goal { title, current, target })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_come_from_links_or_alone() {
        let key = "cloes99hp0001l908lcc62g6a";
        assert_eq!(parse_key(&format!("https://tako.id/overlay/alert?overlay_key={key}")).as_deref(), Some(key));
        assert_eq!(parse_key(&format!("https://tako.id/overlay/alert?overlay_key={key}&x=1")).as_deref(), Some(key));
        assert_eq!(parse_key(&format!("https://tako.id/overlay/alert/{key}/")).as_deref(), Some(key));
        assert_eq!(parse_key(&format!("  {key} ")).as_deref(), Some(key));
        // Not a key: too short, upper case, or punctuation that would change the channel name.
        assert_eq!(parse_key("abc"), None);
        assert_eq!(parse_key("CLOES99HP0001L908LCC62G6A"), None);
        assert_eq!(parse_key("cloes99hp0001l908lcc6.g6a"), None);
        assert_eq!(parse_key("https://tako.id/overlay/alert"), None);
    }

    #[test]
    fn only_uuids_go_back_into_headers() {
        assert!(is_uuid("0b6f7a52-3c1e-4c55-9d1a-2f2b8a9c1d11"));
        assert!(!is_uuid(""));
        assert!(!is_uuid("0b6f7a52-3c1e-4c55-9d1a-2f2b8a9c1d11\r\nX-Evil: 1"));
        assert!(!is_uuid("test-1789368605683"));
    }

    #[test]
    fn amounts_read_as_rupiah() {
        assert_eq!(rupiah(0), "Rp 0");
        assert_eq!(rupiah(500), "Rp 500");
        assert_eq!(rupiah(10_000), "Rp 10.000");
        assert_eq!(rupiah(12_522_500), "Rp 12.522.500");
    }

    #[test]
    fn alert_results_become_gifts() {
        let json = r#"{"$":{"id":"0b6f7a52-3c1e-4c55-9d1a-2f2b8a9c1d11","isTest":false},
            "sender":{"name":"Ana  Putri"},"amount":25000,"message":"semangat [@abc] ya\nkak"}"#;
        let gift = gift(&JsonObject::Parse(&HSTRING::from(json)).unwrap()).unwrap();
        assert_eq!(gift.name, "Ana Putri");
        assert_eq!(gift.initials(), "AP");
        assert_eq!(gift.value(), "Rp 25.000");
        assert_eq!(gift.message, "semangat [sticker] ya kak");
        assert!(gift.media.is_none() && !gift.test);

        let json = r#"{"$":{"id":"x"},"sender":{"name":"@rafaar"},"amount":50000,"message":"","unit":{"name":"Mawar","quantity":5}}"#;
        let gift = super::gift(&JsonObject::Parse(&HSTRING::from(json)).unwrap()).unwrap();
        assert_eq!((gift.value().as_str(), gift.initials().as_str()), ("5× Mawar", "R"));
    }

    #[test]
    fn widgets_read_their_state() {
        let json = r#"{"rankings":[{"person":{"name":"@rafaar"},"amount":12522500},{"person":{"name":"Mikel"},"amount":350000}]}"#;
        let rows = board(&JsonObject::Parse(&HSTRING::from(json)).unwrap());
        assert_eq!(rows, vec![Rank { name: "@rafaar".into(), amount: 12_522_500 }, Rank { name: "Mikel".into(), amount: 350_000 }]);

        let json = r#"{"currentAmount":60000,"steps":[{"id":"a","target":50000},{"id":"b","title":"Mic","target":1200000}]}"#;
        let goal = goal(&JsonObject::Parse(&HSTRING::from(json)).unwrap()).unwrap();
        assert_eq!((goal.title.as_str(), goal.current, goal.target), ("Mic", 60_000, 1_200_000));
        assert!((goal.fraction() - 0.05).abs() < 1e-6);
        assert!(super::goal(&JsonObject::Parse(&HSTRING::from(r#"{"currentAmount":1,"steps":[]}"#)).unwrap()).is_none());
    }

    /// Against the real service: `CREST_TAKO_KEY=<key> cargo test tako::tests::live -- --ignored`.
    /// Reads the overlay once (never marking anything played) and subscribes on the relay.
    #[test]
    #[ignore]
    fn live() {
        let key = std::env::var("CREST_TAKO_KEY").expect("CREST_TAKO_KEY");
        let mut reader = Reader { key: &key, seen: VecDeque::new(), versions: Vec::new(), refreshed: Instant::now() };
        let updates = std::cell::RefCell::new(Vec::new());
        let collect = |update: Update| {
            updates.borrow_mut().push(match update {
                Update::Board(rows) => format!("board: {} rows", rows.len()),
                Update::Goal(goal) => format!("goal: {:?}", goal.map(|g| (g.current, g.target))),
                Update::Gift(_) => "gift".to_string(),
                Update::Status(status) => format!("{status:?}"),
                Update::Songs(songs) => format!("songs: {}", songs.len()),
                Update::Poll(poll) => format!("poll: {} options", poll.len()),
                Update::Countdown(timer) => format!("timer: {timer:?}"),
                Update::Vips(vips) => format!("vip: {}", vips.len()),
            })
        };
        assert!(reader.refresh(DRAIN_START, true, &collect).is_ok(), "overlay read failed");
        println!("{:?} · {} queued ids", updates.borrow(), reader.seen.len());

        let socket = WebSocket::connect(RELAY_HOST, RELAY_PATH, USER_AGENT).expect("relay");
        let mut subscribed = false;
        for _ in 0..4 {
            let Some((event, _)) = socket.receive().as_deref().and_then(frame) else { break };
            println!("relay: {event}");
            if event == "pusher:connection_established" {
                let subscribe = format!(r#"{{"event":"pusher:subscribe","data":{{"auth":"","channel":"overlay.{key}"}}}}"#);
                socket.send(&subscribe).unwrap();
            }
            if event == "pusher_internal:subscription_succeeded" {
                subscribed = true;
                break;
            }
        }
        socket.close();
        assert!(subscribed);
    }

    #[test]
    fn relay_frames_unwrap_their_data() {
        let (event, data) = frame(r#"{"event":"command","channel":"overlay.k","data":"\"alert.test.gif\""}"#).unwrap();
        assert_eq!((event.as_str(), data.as_str()), ("command", "alert.test.gif"));
        assert!(test_gift(&data).is_some_and(|g| g.test && g.amount == 10_000));
        assert!(test_gift("songshare.test").is_none());
        assert!(test_gift("alert.skip").is_none());
        let (event, data) = frame(r#"{"event":"messages","channel":"overlay.k","data":"true"}"#).unwrap();
        assert_eq!((event.as_str(), data.as_str()), ("messages", "true"));
        let (event, _) = frame(r#"{"event":"pusher:connection_established","data":"{\"socket_id\":\"1.2\",\"activity_timeout\":30}"}"#).unwrap();
        assert_eq!(event, "pusher:connection_established");
    }
}
