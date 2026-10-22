//! Calendar feeds the user subscribed to: Google Calendar's "secret address in iCal format",
//! and the same for Outlook, iCloud, and Fastmail. Read-only, no account.
//!
//! The addresses live in `calendars.txt`, one per line as `name|https://…`; the address is a
//! secret (anyone who has it can read the calendar), so the file is plain text like the rest of
//! the data but never logged. Feeds are fetched only while the calendar is on Home and the
//! computer is online, at most every 30 minutes.

use std::thread;
use std::time::{Duration, Instant};

use crate::shell::store;
use crate::system::http;
use crate::system::services::{Event, Notifier};
use super::calfeed::Feed;

const FILE: &str = "calendars.txt";
/// A feed older than this is fetched again.
pub const REFRESH: Duration = Duration::from_secs(30 * 60);
/// Addresses per feed line, and feeds in total.
const MAX_NAME: usize = 24;
pub const MAX_FEEDS: usize = 6;

const USER_AGENT: &str = concat!("Crest/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rinci-labs/crest)");

/// One subscribed feed.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub name: String,
    /// `webcal://` is accepted and read as `https://`.
    pub url: String,
}

impl Source {
    /// `name|address` → source. A bare address gets a name from its host.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (name, url) = match line.split_once('|') {
            Some((name, url)) => (name.trim(), url.trim()),
            None => ("", line),
        };
        let url = normalize(url)?;
        let name = if name.is_empty() { host_name(&url) } else { name.to_string() };
        Some(Self { name: name.chars().take(MAX_NAME).collect(), url })
    }
}

/// `webcal://x` and an address without a scheme all become an `https` URL.
fn normalize(url: &str) -> Option<String> {
    let url = url.trim();
    let url = match url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    };
    // Only https is fetched: a plain http feed would send the secret address in the clear.
    (url.starts_with("https://") && url.len() > 8).then_some(url)
}

/// Short name from a feed's host, e.g. "calendar.google.com" → "Google Calendar".
fn host_name(url: &str) -> String {
    let host = url.trim_start_matches("https://").split('/').next().unwrap_or_default();
    match host {
        h if h.ends_with("google.com") => "Google Calendar".to_string(),
        h if h.ends_with("outlook.office365.com") || h.ends_with("outlook.live.com") => "Outlook".to_string(),
        h if h.ends_with("icloud.com") => "iCloud".to_string(),
        h if h.ends_with("fastmail.com") => "Fastmail".to_string(),
        h => h.split('.').next().unwrap_or(h).to_string(),
    }
}

/// Subscribed feeds and the events last read from them.
#[derive(Default)]
pub struct Feeds {
    pub sources: Vec<Source>,
    pub feeds: Vec<Feed>,
    /// When each feed was last fetched (same order as `sources`).
    pub fetched: Vec<Option<Instant>>,
    /// Last failure per feed, shown in place of its events.
    pub errors: Vec<Option<String>>,
}

impl Feeds {
    /// The saved feeds; nothing when the calendar is off.
    pub fn load(on: bool) -> Self {
        if !on {
            return Self::default();
        }
        Self::from_lines(&store::load(FILE))
    }

    /// Feeds from the lines of `calendars.txt`, so loading is testable without touching the
    /// user's files.
    pub fn from_lines(lines: &[String]) -> Self {
        let sources: Vec<Source> = lines.iter().filter_map(|line| Source::parse(line)).take(MAX_FEEDS).collect();
        let count = sources.len();
        // One entry per source in every array, so an index from any of them addresses all three.
        Self {
            sources,
            feeds: vec![Feed::default(); count],
            fetched: vec![None; count],
            errors: vec![None; count],
        }
    }

    /// Read the subscriptions again, keeping what was already fetched for the addresses that are
    /// still listed. Called before every fetch, since `calendars.txt` may have been edited by hand.
    pub fn reload(&mut self) {
        self.adopt(Self::from_lines(&store::load(FILE)));
    }

    /// Take the subscriptions of `fresh`, carrying over the data fetched for the addresses that
    /// are still there. Matching is by address: the name and the position in the file may both
    /// have changed.
    fn adopt(&mut self, fresh: Self) {
        let old = std::mem::replace(self, fresh);
        self.carry_over(&old);
    }

    /// Copy the fetched data of `old` onto the matching addresses in `self`. Matching is by
    /// address: the name and the position in the file may both have changed, and a feed that is
    /// no longer listed loses its data.
    fn carry_over(&mut self, old: &Feeds) {
        for (index, source) in self.sources.iter().enumerate() {
            let Some(from) = old.sources.iter().position(|candidate| candidate.url == source.url) else { continue };
            if let Some(feed) = old.feeds.get(from) {
                self.feeds[index] = feed.clone();
            }
            self.fetched[index] = old.fetched.get(from).copied().flatten();
            self.errors[index] = old.errors.get(from).cloned().flatten();
        }
    }

    /// Feeds whose data is missing or old.
    pub fn stale(&self) -> Vec<(usize, Source)> {
        self.sources
            .iter()
            .enumerate()
            .filter(|(i, _)| self.fetched.get(*i).copied().flatten().is_none_or(|at| at.elapsed() >= REFRESH))
            .map(|(i, source)| (i, source.clone()))
            .collect()
    }
}

/// What one fetch produced.
pub struct Fetched {
    pub index: usize,
    pub feed: Option<Feed>,
    /// Reason the feed could not be read.
    pub error: Option<String>,
}

/// Fetch one feed on a worker thread; the result arrives as [`Event::Calendar`].
pub fn fetch(index: usize, source: Source, notify: Notifier) {
    let spawned = thread::Builder::new().name("calendar feed".into()).spawn(move || {
        let result = match http::get(&source.url, USER_AGENT) {
            Ok(body) => {
                let horizon = crate::system::datetime::now_seconds().div_euclid(86_400);
                let feed = Feed::parse(&body, horizon);
                debug_log!("calendar feed: {} events", feed.events.len());
                if feed.events.is_empty() && feed.name.is_empty() {
                    // An address that is wrong or no longer shared answers with a page, not a feed.
                    Fetched { index, feed: None, error: Some("not a calendar feed".to_string()) }
                } else {
                    Fetched { index, feed: Some(feed), error: None }
                }
            }
            Err(e) => {
                // The address is a secret: log the kind of failure, never the URL.
                error_log!("calendar feed failed: {e}");
                Fetched { index, feed: None, error: Some(format!("cannot read: {e}")) }
            }
        };
        notify.send(Event::Calendar(result));
    });
    if let Err(e) = spawned {
        error_log!("failed to start calendar thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_feed_lines() {
        let source = Source::parse("Work|https://calendar.google.com/calendar/ical/x/basic.ics").unwrap();
        assert_eq!(source.name, "Work");
        assert!(source.url.starts_with("https://"));

        // A bare address takes its name from the host.
        assert_eq!(Source::parse("https://calendar.google.com/x.ics").unwrap().name, "Google Calendar");
        assert_eq!(Source::parse("https://outlook.office365.com/x.ics").unwrap().name, "Outlook");
        // webcal links are read as https.
        assert_eq!(Source::parse("webcal://calendar.google.com/x.ics").unwrap().url, "https://calendar.google.com/x.ics");
        // Plain http would send the secret address in the clear.
        assert!(Source::parse("Calendar|http://example.com/x.ics").is_none());
        assert!(Source::parse("").is_none());
        assert!(Source::parse("# a note").is_none());
    }

    #[test]
    fn reads_subscriptions_from_the_file() {
        let lines = [
            "# a note".to_string(),
            "Work|https://calendar.google.com/a.ics".to_string(),
            "https://outlook.office365.com/b.ics".to_string(),
            "Bad|http://example.com/c.ics".to_string(),
            "".to_string(),
        ];
        let feeds = Feeds::from_lines(&lines);
        assert_eq!(feeds.sources.len(), 2, "a plain http address is refused");
        assert_eq!(feeds.sources[0].name, "Work");
        assert_eq!(feeds.sources[1].name, "Outlook", "a bare address takes its host's name");
        assert_eq!(feeds.fetched.len(), 2);
        assert_eq!(feeds.errors.len(), 2);
    }

    #[test]
    fn reload_keeps_fetched_data_by_address() {
        let a = "A|https://x/a.ics".to_string();
        let b = "B|https://x/b.ics".to_string();
        let c = "C|https://x/c.ics".to_string();
        let mut feeds = Feeds::from_lines(&[a, b]);
        feeds.feeds[0].name = "A feed".into();
        feeds.feeds[1].name = "B feed".into();
        let at = Instant::now();
        feeds.fetched[0] = Some(at);
        feeds.errors[1] = Some("cannot read".into());

        // The file was edited: B moved, A's name changed, C is new.
        feeds.adopt(Feeds::from_lines(&["Renamed|https://x/a.ics".to_string(), c, "B|https://x/b.ics".to_string()]));
        assert_eq!(feeds.sources.len(), 3);
        assert_eq!(feeds.sources[0].name, "Renamed");
        assert_eq!(feeds.feeds.len(), 3, "one feed slot per source");
        assert_eq!(feeds.feeds[0].name, "A feed", "data follows the address, not the position");
        assert_eq!(feeds.feeds[1].name, "", "C was never fetched");
        assert_eq!(feeds.feeds[2].name, "B feed");
        assert_eq!(feeds.fetched[0], Some(at), "the fetch time follows the address too");
        assert_eq!(feeds.fetched[1], None);
        assert_eq!(feeds.errors[2].as_deref(), Some("cannot read"));
        assert_eq!(feeds.stale().len(), 2, "only the unfetched feeds are due");
    }

    #[test]
    fn only_the_first_feeds_are_kept() {
        // A file with more lines than feeds would otherwise fetch without end; the cap keeps
        // the panel's fixed rows and the 30-minute refresh bounded.
        let lines: Vec<String> = (0..MAX_FEEDS + 2).map(|i| format!("F{i}|https://x/{i}.ics")).collect();
        let feeds = Feeds::from_lines(&lines);
        assert_eq!(feeds.sources.len(), MAX_FEEDS);
        assert_eq!(feeds.sources[0].name, "F0", "the first lines are the ones kept");
        assert_eq!(feeds.sources[MAX_FEEDS - 1].name, format!("F{}", MAX_FEEDS - 1));
        assert_eq!(feeds.feeds.len(), MAX_FEEDS, "one slot per kept source");
        assert_eq!(feeds.fetched.len(), MAX_FEEDS);
    }

    #[test]
    fn long_names_are_truncated() {
        // The name sits in a fixed-width row; an untruncated one would push the date and the
        // event count out of view.
        let long = "A very long calendar name nobody would fit";
        let name = Source::parse(&format!("{long}|https://x/a.ics")).unwrap().name;
        assert_eq!(name, &long[..MAX_NAME]);
        // A name of exactly the limit is kept whole.
        let exact = "B".repeat(MAX_NAME);
        assert_eq!(Source::parse(&format!("{exact}|https://x/b.ics")).unwrap().name, exact);
    }

    #[test]
    fn carry_over_follows_the_address() {
        let mut old = Feeds::from_lines(&["A|https://x/a.ics".to_string(), "B|https://x/b.ics".to_string()]);
        old.feeds[0].name = "A feed".into();
        old.feeds[1].name = "B feed".into();
        let at = Instant::now();
        old.fetched = vec![Some(at), Some(at)];

        // A was renamed in the file, B was removed.
        let mut fresh = Feeds::from_lines(&["Renamed|https://x/a.ics".to_string()]);
        fresh.carry_over(&old);
        assert_eq!(fresh.sources[0].name, "Renamed");
        assert_eq!(fresh.feeds[0].name, "A feed", "the events follow the address, not the name");
        assert_eq!(fresh.fetched, vec![Some(at)]);
        assert_eq!(fresh.feeds.len(), 1, "an address that is gone takes its slot with it");
        assert!(fresh.sources.iter().all(|s| s.url != "https://x/b.ics"));
    }

    #[test]
    fn everything_is_stale_until_fetched() {
        let mut feeds = Feeds::default();
        assert!(feeds.stale().is_empty());
        feeds.sources.push(Source { name: "a".into(), url: "https://x/a.ics".into() });
        feeds.fetched.push(None);
        feeds.errors.push(None);
        assert_eq!(feeds.stale().len(), 1);
        feeds.fetched[0] = Some(Instant::now());
        assert!(feeds.stale().is_empty(), "just fetched");
        feeds.fetched[0] = Some(Instant::now() - REFRESH);
        assert_eq!(feeds.stale().len(), 1, "old data is fetched again");
    }
}
