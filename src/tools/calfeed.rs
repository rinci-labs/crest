//! Calendar feeds in iCalendar form (RFC 5545), as published by Google Calendar, Outlook,
//! iCloud, and Fastmail. Read-only: Crest never writes back.
//!
//! Only what the panel needs is understood: timed and all-day events, events spanning days,
//! local time with `TZID` (Windows names and common IANA names), `UTC`, floating times, and
//! the `RRULE` subset the popular calendars emit (`FREQ` day/week/month/year with `INTERVAL`,
//! `COUNT`, `UNTIL`, `BYDAY`, and `BYMONTHDAY`). `EXDATE` removes occurrences.
//!
//! Every event is turned into local wall-clock time, so the rest of the app never deals with
//! time zones.

use super::agenda::Date;

/// A feed is read this far ahead of the day it is fetched. Repeating events are expanded up to
/// there; anything further out comes with a later fetch.
pub const HORIZON_DAYS: i64 = 400;
/// Upper bound on occurrences of one event, so a badly formed rule cannot freeze the app.
const MAX_OCCURRENCES: usize = 400;

/// Wall-clock minute of an event, in local time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Moment {
    /// Days since 1970-01-01, local.
    pub day: i64,
    /// Minutes after local midnight, 0..1440.
    pub minute: u32,
}

impl Moment {
    /// The plain date, for the calendar strip.
    pub fn date(self) -> Date {
        Date::from_days(self.day)
    }
}

/// One event from a feed; repeating events are expanded into one `Event` per occurrence.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Stable id: the feed's `UID` plus the occurrence's start, so the same event from Google
    /// and from a typed agenda entry never collide.
    pub id: String,
    pub start: Moment,
    /// End, always after `start`; all-day events end at the following midnight.
    pub end: Moment,
    pub text: String,
    /// All-day event: no time is shown, and the row reads as a day marker.
    pub all_day: bool,
}

impl Event {
    /// "09.00", or empty for an all-day event.
    pub fn minute_text(&self) -> String {
        if self.all_day {
            return String::new();
        }
        let minute = self.start.minute;
        format!("{:02}.{:02}", minute / 60, minute % 60)
    }

    /// Does this event cover `day` at all? Used for the dot under a date.
    pub fn covers(&self, day: i64) -> bool {
        // Half-open on both sides, so an event ending at midnight does not reach the next day.
        let day = day * 1440;
        self.start.day * 1440 + self.start.minute as i64 <= day + 1439
            && day < self.end.day * 1440 + self.end.minute as i64
    }

    /// Is the event running at this moment?
    pub fn active_at(&self, at: Moment) -> bool {
        self.start <= at && at < self.end
    }
}

/// A parsed feed.
#[derive(Default, Clone, Debug)]
pub struct Feed {
    /// `X-WR-CALNAME`, when the feed names itself.
    pub name: String,
    pub events: Vec<Event>,
}

impl Feed {
    /// Parse a feed. Unreadable events are skipped; a feed that is not iCalendar at all gives an
    /// empty feed.
    pub fn parse(text: &str, horizon: i64) -> Self {
        let root = unfold(text);
        // Time zones may be defined after the events that use them, so they are collected first.
        let mut zones = Vec::new();
        for block in root.walk() {
            if block.name == "VTIMEZONE" {
                zones.push(zone(block));
            }
        }
        let mut feed = Self::default();
        let mut seen = std::collections::HashSet::new();
        for block in root.walk() {
            match block.name.as_str() {
                "VEVENT" => {
                    for event in expand(block, &zones, horizon) {
                        if seen.insert(event.id.clone()) {
                            feed.events.push(event);
                        }
                    }
                }
                // The calendar itself names the feed.
                _ if feed.name.is_empty() => {
                    if let Some(value) = block.get("X-WR-CALNAME") {
                        feed.name = unescape(value);
                    }
                }
                _ => {}
            }
        }
        feed.events.sort_by_key(|e| e.start);
        feed
    }

    /// Events that touch `day`, in start order.
    pub fn on(&self, day: i64) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.covers(day))
    }

    /// Does the feed have anything on `day`? Used for the dot under a date.
    pub fn has(&self, day: i64) -> bool {
        self.on(day).next().is_some()
    }
}

/// A component: its name, its own `NAME:VALUE` properties (parameters dropped), and the
/// components nested in it.
#[derive(Default)]
struct Block {
    name: String,
    properties: Vec<(String, String)>,
    /// Nested components: `VEVENT` and `VTIMEZONE` in the calendar, `STANDARD` and `DAYLIGHT`
    /// in a time zone.
    children: Vec<Block>,
}

impl Block {
    /// The value of `key` (its parameters are ignored), e.g. `SUMMARY`.
    fn get(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|(k, _)| base(k) == key).map(|(_, v)| v.as_str())
    }

    /// Every value of `key`, for properties that may repeat (`EXDATE`).
    fn all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.properties.iter().filter(move |(k, _)| base(k) == key).map(|(_, v)| v.as_str())
    }

    /// The parameter `param` of property `key`, e.g. `TZID` of `DTSTART`.
    fn param(&self, key: &str, param: &str) -> Option<&str> {
        let (left, _) = self.properties.iter().find(|(k, _)| base(k) == key)?;
        left.split(';').skip(1).find_map(|p| p.split_once('=')).filter(|(k, _)| k.trim().eq_ignore_ascii_case(param)).map(|(_, v)| v.trim())
    }

    /// Is `key` an all-day property (`VALUE=DATE`)?
    fn date_only(&self, key: &str) -> bool {
        self.param(key, "VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"))
    }

    /// This component and everything nested in it, depth first.
    fn walk(&self) -> Vec<&Block> {
        let mut out = vec![self];
        for child in &self.children {
            out.extend(child.walk());
        }
        out
    }
}

/// Property name without its parameters: `DTSTART;TZID=X` → `DTSTART`.
fn base(left: &str) -> String {
    left.split(';').next().unwrap_or(left).trim().to_uppercase()
}

/// Split the feed into components, joining folded lines (`\r\n` followed by a space or tab).
fn unfold(text: &str) -> Block {
    let mut logical: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = logical.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        logical.push(line.to_string());
    }
    // A stack of open components; closing one hands it to the component that contains it.
    let mut stack: Vec<Block> = vec![Block::default()];
    for line in logical {
        if let Some(name) = line.strip_prefix("BEGIN:") {
            stack.push(Block { name: name.trim().to_uppercase(), ..Block::default() });
            continue;
        }
        if line.starts_with("END:") {
            if stack.len() > 1
                && let Some(done) = stack.pop()
                && let Some(parent) = stack.last_mut()
            {
                parent.children.push(done);
            }
            continue;
        }
        let Some(block) = stack.last_mut() else { continue };
        let Some((left, value)) = line.split_once(':') else { continue };
        let left = left.trim();
        if left.is_empty() {
            continue;
        }
        // The parameters ("DTSTART;TZID=Europe/Berlin") are kept: times need their zone.
        block.properties.push((left.to_string(), value.to_string()));
    }
    // A feed that was cut short still yields what it did contain.
    while stack.len() > 1 {
        let Some(done) = stack.pop() else { break };
        if let Some(parent) = stack.last_mut() {
            parent.children.push(done);
        }
    }
    stack.pop().unwrap_or_default()
}

/// A `VTIMEZONE` definition: the `TZID` events use, and the UTC offset each of its rules has.
struct Zone {
    id: String,
    rules: Vec<Rule2>,
}

/// One `STANDARD`/`DAYLIGHT` block: the offset it applies from its start date on.
struct Rule2 {
    /// Seconds east of UTC (`TZOFFSETTO`).
    offset: i64,
    /// Month and day the rule starts (`DTSTART`), for picking the rule in force.
    from: (u32, u32),
}

fn zone(block: &Block) -> Zone {
    let id = block.get("TZID").map(unescape).unwrap_or_default();
    let mut rules = Vec::new();
    for child in &block.children {
        let Some(offset) = child.get("TZOFFSETTO").and_then(utc_offset) else { continue };
        let Some(start) = child.get("DTSTART").and_then(stamp) else { continue };
        let date = Date::from_days(start.day);
        rules.push(Rule2 { offset, from: (date.month, date.day) });
    }
    Zone { id, rules }
}

impl Zone {
    /// Offset in force at `date`: the last rule that started this year, or the first one.
    ///
    /// The yearly transition dates are not evaluated (they repeat by weekday), so around the
    /// changeover an event can be an hour off; everything else is exact.
    fn offset_at(&self, date: Date) -> i64 {
        let mut best: Option<&Rule2> = None;
        for rule in &self.rules {
            if (rule.from.0, rule.from.1) > (date.month, date.day) {
                continue;
            }
            if best.is_none_or(|b| (b.from.0, b.from.1) <= (rule.from.0, rule.from.1)) {
                best = Some(rule);
            }
        }
        best.or_else(|| self.rules.first()).map_or(0, |r| r.offset)
    }
}

/// `+0200` / `-0530` → seconds east of UTC.
fn utc_offset(value: &str) -> Option<i64> {
    let value = value.trim();
    let (sign, rest) = match value.as_bytes().first()? {
        b'+' => (1, &value[1..]),
        b'-' => (-1, &value[1..]),
        _ => (1, value),
    };
    if rest.len() != 4 {
        return None;
    }
    let hours: i64 = rest[0..2].parse().ok()?;
    let minutes: i64 = rest[2..4].parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

/// iCalendar times: `YYYYMMDD` or `YYYYMMDDTHHMMSSZ`, or `YYYYMMDDTHHMMSS` in a zone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stamp {
    day: i64,
    second: u32,
    /// The value ended in `Z`, so the instant is UTC.
    utc: bool,
}

fn stamp(value: &str) -> Option<Stamp> {
    let value = value.trim();
    let (body, utc) = match value.strip_suffix('Z') {
        Some(body) => (body, true),
        None => (value, false),
    };
    let (date, time) = match body.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (body, None),
    };
    if date.len() != 8 {
        return None;
    }
    let year: i32 = date[0..4].parse().ok()?;
    let month: u32 = date[4..6].parse().ok()?;
    let day: u32 = date[6..8].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let second = match time {
        None => 0,
        Some(time) if time.len() == 6 => {
            let hour: u32 = time[0..2].parse().ok()?;
            let minute: u32 = time[2..4].parse().ok()?;
            let second: u32 = time[4..6].parse().ok()?;
            if hour > 23 || minute > 59 || second > 60 {
                return None;
            }
            hour * 3600 + minute * 60 + second
        }
        Some(_) => return None,
    };
    Some(Stamp { day: Date::new(year, month, day).days(), second, utc })
}

/// A date-time property with its parameters, e.g. `DTSTART;TZID=…:20261020T090000`.
#[derive(Clone)]
struct DateTime {
    stamp: Stamp,
    /// `VALUE=DATE`, i.e. an all-day event.
    date_only: bool,
    /// The `TZID` parameter.
    zone: Option<String>,
}

impl DateTime {
    /// A property block's `key` (with its parameters) and its value.
    fn parse(block: &Block, key: &str) -> Option<Self> {
        let value = block.get(key)?;
        let stamp = stamp(value)?;
        let zone = block.param(key, "TZID").map(unescape);
        let date_only = block.date_only(key) || value.trim().len() == 8;
        Some(Self { stamp, date_only, zone })
    }

    /// The instant as local wall-clock time.
    ///
    /// UTC values are converted with the Windows zone; values in a feed zone are converted
    /// with that zone's own offset; floating values are taken as already local.
    fn moment(&self, zones: &[Zone]) -> Moment {
        let date = Date::from_days(self.stamp.day);
        let utc_seconds = self.stamp.day * 86_400 + self.stamp.second as i64;
        let east = if self.stamp.utc {
            0
        } else if let Some(zone) = self.zone.as_deref().and_then(|id| zones.iter().find(|z| z.id == id)) {
            zone.offset_at(date)
        } else {
            // Floating time: no conversion. A `TZID` without a definition is treated the same
            // way, since guessing its offset would put the event at the wrong hour.
            return Moment { day: self.stamp.day, minute: self.stamp.second / 60 };
        };
        // Seconds east of UTC → the UTC instant, then the user's own zone.
        let instant = utc_seconds - east;
        match crate::system::clock::local_moment(instant) {
            Some((day, minute)) => Moment { day, minute },
            // Before 1601 or after the Windows range: keep the value unshifted.
            None => Moment { day: self.stamp.day, minute: self.stamp.second / 60 },
        }
    }
}

/// Expand one `VEVENT` into the occurrences that fall before `horizon`.
/// `horizon` is a day number: repeating events stop there.
fn expand(block: &Block, zones: &[Zone], horizon: i64) -> Vec<Event> {
    let Some(start) = DateTime::parse(block, "DTSTART") else { return Vec::new() };
    let end = DateTime::parse(block, "DTEND");
    let summary = block.get("SUMMARY").map(unescape).unwrap_or_default();
    let summary = clean(&summary);
    let uid = block.get("UID").and_then(|u| u.split('@').next()).map(unescape).unwrap_or_else(|| summary.clone());
    let start_moment = start.moment(zones);

    // Duration, taken from DTEND when present and otherwise as a sensible default.
    let (duration, all_day) = match (&end, start.date_only) {
        (Some(end), true) => {
            let days = (end.stamp.day - start.stamp.day).max(1);
            (days * 1440, true)
        }
        (Some(end), false) => ((span(end.moment(zones)) - span(start_moment)).max(0), false),
        (None, true) => (1440, true),
        (None, false) => (60, false),
    };
    let event = |start: Moment, duration: i64| Event {
        id: format!("{uid}@{}", start.day * 1440 + start.minute as i64),
        start,
        end: add_minutes(start, duration),
        text: summary.clone(),
        all_day,
    };

    let Some(rule) = block.get("RRULE") else {
        // A one-off event is kept however far out it is: a holiday next year still shows.
        return vec![event(start_moment, duration)];
    };
    let rule = Rule::parse(rule);
    let excluded: Vec<i64> = exclusions(block, zones);
    let mut events = Vec::new();
    for occurrence in rule.occurrences(start_moment, horizon) {
        let key = occurrence.day * 1440 + occurrence.minute as i64;
        if excluded.contains(&key) {
            continue;
        }
        events.push(event(occurrence, duration));
    }
    events
}

/// Minutes from day zero, to measure durations across days.
fn span(moment: Moment) -> i64 {
    moment.day * 1440 + moment.minute as i64
}

fn add_minutes(start: Moment, minutes: i64) -> Moment {
    let total = span(start) + minutes.max(1);
    let day = total.div_euclid(1440);
    let minute = total.rem_euclid(1440) as u32;
    let mut moment = Moment { day, minute };
    // An all-day event ending at midnight belongs to the next day.
    if minute == 0 && minutes % 1440 == 0 {
        moment.minute = 0;
    }
    moment
}

/// `EXDATE` values as day/minute keys, with the zone the property names.
fn exclusions(block: &Block, zones: &[Zone]) -> Vec<i64> {
    let zone = block.param("EXDATE", "TZID").map(unescape);
    block
        .all("EXDATE")
        .flat_map(|value| value.split(','))
        .filter_map(|one| {
            Some(span(DateTime { stamp: stamp(one)?, date_only: false, zone: zone.clone() }.moment(zones)))
        })
        .collect()
}

/// The `RRULE` subset the popular calendars emit.
struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<usize>,
    until: Option<Moment>,
    by_day: Vec<u32>,
    by_month_day: Vec<i32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Rule {
    fn parse(text: &str) -> Self {
        let mut rule = Self { freq: Freq::Weekly, interval: 1, count: None, until: None, by_day: Vec::new(), by_month_day: Vec::new() };
        for part in text.split(';') {
            let Some((key, value)) = part.split_once('=') else { continue };
            match key.trim().to_uppercase().as_str() {
                "FREQ" => {
                    rule.freq = match value.trim().to_uppercase().as_str() {
                        "DAILY" => Freq::Daily,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        _ => Freq::Weekly,
                    }
                }
                "INTERVAL" => rule.interval = value.trim().parse::<i64>().unwrap_or(1).clamp(1, 100),
                "COUNT" => rule.count = value.trim().parse().ok(),
                "UNTIL" => rule.until = stamp(value).map(|s| Moment { day: s.day, minute: s.second / 60 }),
                "BYDAY" => {
                    rule.by_day = value
                        .split(',')
                        .filter_map(|day| match day.trim().to_uppercase().as_str() {
                            "SU" => Some(0),
                            "MO" => Some(1),
                            "TU" => Some(2),
                            "WE" => Some(3),
                            "TH" => Some(4),
                            "FR" => Some(5),
                            "SA" => Some(6),
                            // "2MO" (second Monday) is not supported; the rule then repeats simply.
                            _ => None,
                        })
                        .collect()
                }
                "BYMONTHDAY" => {
                    rule.by_month_day = value.split(',').filter_map(|d| d.trim().parse().ok()).collect()
                }
                _ => {}
            }
        }
        rule
    }

    /// Occurrence starts, the first one equal to `start`.
    fn occurrences(&self, start: Moment, horizon: i64) -> Vec<Moment> {
        let limit = self.count.unwrap_or(MAX_OCCURRENCES).min(MAX_OCCURRENCES);
        let mut out = Vec::new();
        let first = start.date();
        let mut step = 0i64;
        // Generous cap: monthly and yearly rules need many steps to cross the horizon.
        while out.len() < limit && step < 4_000 {
            let date = match self.freq {
                Freq::Daily => first.add_days(step * self.interval),
                Freq::Weekly => first.add_days(step * self.interval * 7),
                Freq::Monthly => first.add_months(step * self.interval),
                Freq::Yearly => first.add_months(step * self.interval * 12),
            };
            step += 1;
            // Repeating events stop at the horizon; the next fetch extends them.
            if date.days() > horizon + HORIZON_DAYS {
                break;
            }
            if !self.matches(date) {
                continue;
            }
            let moment = Moment { day: date.days(), minute: start.minute };
            if self.until.is_some_and(|until| moment > until) {
                break;
            }
            out.push(moment);
        }
        out
    }

    fn matches(&self, date: Date) -> bool {
        let weekday = date.weekday_index();
        if !self.by_day.is_empty() && !self.by_day.contains(&weekday) {
            return false;
        }
        if !self.by_month_day.is_empty() {
            let day = date.day as i32;
            // Negative values count from the end of the month (-1 = last day).
            let last = date.days_in_month() as i32;
            let wanted = self.by_month_day.iter().any(|d| *d == day || (*d < 0 && last + *d + 1 == day));
            if !wanted {
                return false;
            }
        }
        true
    }
}

/// Undo iCalendar text escaping (`\,` `\;` `\n` `\\`).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push(' '),
            Some('t') | Some('T') => out.push(' '),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// One line of readable text, as the agenda rows expect it.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(super::agenda::MAX_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = "BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//Google Inc//Google Calendar 70.9054//EN\r
X-WR-CALNAME:Work\r
BEGIN:VTIMEZONE\r
TZID:Europe/Berlin\r
BEGIN:STANDARD\r
DTSTART:19701025T030000\r
TZOFFSETFROM:+0200\r
TZOFFSETTO:+0100\r
END:STANDARD\r
BEGIN:DAYLIGHT\r
DTSTART:19700329T020000\r
TZOFFSETFROM:+0100\r
TZOFFSETTO:+0200\r
END:DAYLIGHT\r
END:VTIMEZONE\r
BEGIN:VEVENT\r
DTSTART;TZID=Europe/Berlin:20261020T090000\r
DTEND;TZID=Europe/Berlin:20261020T100000\r
SUMMARY:Team meeting\\, weekly\r
UID:abc-123@google.com\r
END:VEVENT\r
BEGIN:VEVENT\r
DTSTART;VALUE=DATE:20261021\r
DTEND;VALUE=DATE:20261022\r
SUMMARY:Mom's birthday\r
UID:def-456@google.com\r
END:VEVENT\r
END:VCALENDAR\r
";

    /// Local midnight at day zero, for comparing against the current zone.
    fn day(y: i32, m: u32, d: u32) -> i64 {
        Date::new(y, m, d).days()
    }

    #[test]
    fn reads_a_feed_with_a_zone_and_an_all_day_event() {
        let feed = Feed::parse(FEED, day(2026, 10, 20));
        assert_eq!(feed.name, "Work");
        assert_eq!(feed.events.len(), 2);
        let meeting = &feed.events[0];
        assert_eq!(meeting.text, "Team meeting, weekly");
        assert!(!meeting.all_day);
        assert_eq!(meeting.start.date(), Date::new(2026, 10, 20));
        assert!(meeting.covers(day(2026, 10, 20)));
        let birthday = &feed.events[1];
        assert!(birthday.all_day, "VALUE=DATE is an all-day event");
        assert_eq!(birthday.start.date(), Date::new(2026, 10, 21));
        assert_eq!(birthday.end.date(), Date::new(2026, 10, 22), "an all-day event ends the next midnight");
        assert!(birthday.covers(day(2026, 10, 21)));
        assert!(!birthday.covers(day(2026, 10, 22)), "does not run into the following day");
    }

    #[test]
    fn utc_times_become_local_wall_clock() {
        // A fixed instant in UTC, converted the same way the app converts it.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261020T120000Z\nDTEND:20261020T130000Z\nSUMMARY:Standup\nUID:u1\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        let event = &feed.events[0];
        let expected = crate::system::clock::local_moment(day(2026, 10, 20) * 86_400 + 12 * 3600)
            .map(|(day, minute)| Moment { day, minute })
            .unwrap();
        assert_eq!(event.start, expected);
        assert_eq!(event.end, add_minutes(expected, 60));
    }

    #[test]
    fn folded_lines_and_escapes() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261020T090000\nSUMMARY:Quarterly planning with\n  the whole team\\; bring notes\nUID:x\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        assert_eq!(feed.events[0].text, "Quarterly planning with the whole team; bring notes");
    }

    #[test]
    fn weekly_rule_expands_and_skips_excluded_dates() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261020T090000\nDTEND:20261020T093000\nRRULE:FREQ=WEEKLY;BYDAY=TU;COUNT=4\nEXDATE:20261103T090000\nSUMMARY:Standup\nUID:u2\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        let days: Vec<i64> = feed.events.iter().map(|e| e.start.day).collect();
        assert_eq!(days, [day(2026, 10, 20), day(2026, 10, 27), day(2026, 11, 10)], "3 Nov is excluded");
        assert!(feed.events.iter().all(|e| e.start.minute == 9 * 60));
    }

    #[test]
    fn daily_rule_stops_at_the_horizon() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20260101T080000\nDTEND:20260101T083000\nRRULE:FREQ=DAILY\nSUMMARY:Run\nUID:u3\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 12, 31));
        assert!(!feed.events.is_empty());
        assert!(feed.events.len() <= MAX_OCCURRENCES);
        assert!(feed.events.iter().all(|e| e.start.day <= day(2026, 12, 31) + HORIZON_DAYS));
    }

    #[test]
    fn monthly_and_yearly_rules() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20260131T100000\nDTEND:20260131T110000\nRRULE:FREQ=MONTHLY;COUNT=3\nSUMMARY:Invoice\nUID:u4\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 1, 1));
        let dates: Vec<Date> = feed.events.iter().map(|e| e.start.date()).collect();
        // 31 January, 28 February, 31 March 2026.
        assert_eq!(dates, [Date::new(2026, 1, 31), Date::new(2026, 2, 28), Date::new(2026, 3, 31)]);
    }

    #[test]
    fn a_timed_event_can_span_midnight() {
        // Late-evening events run into the next day; the calendar strip must put a dot on both
        // days and the agenda must show the event while it is running.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261020T220000\nDTEND:20261021T020000\nSUMMARY:Night shift\nUID:u6\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        let event = &feed.events[0];
        assert_eq!(event.start, Moment { day: day(2026, 10, 20), minute: 22 * 60 });
        assert_eq!(event.end, Moment { day: day(2026, 10, 21), minute: 2 * 60 });
        assert!(event.covers(day(2026, 10, 20)));
        assert!(event.covers(day(2026, 10, 21)), "the tail after midnight belongs to the next day");
        assert!(!event.covers(day(2026, 10, 19)));
        assert!(!event.covers(day(2026, 10, 22)));
        assert!(event.active_at(Moment { day: day(2026, 10, 20), minute: 23 * 60 }));
        assert!(event.active_at(Moment { day: day(2026, 10, 21), minute: 60 }));
        assert!(!event.active_at(Moment { day: day(2026, 10, 21), minute: 2 * 60 }), "the end is exclusive");
    }

    #[test]
    fn a_timed_event_ending_at_midnight_stops_there() {
        // Half-open at the end: an event that ends at 00:00 must not put a dot on the next day,
        // even though its end moment looks like that day's first minute.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261020T220000\nDTEND:20261021T000000\nSUMMARY:Late film\nUID:u7\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        let event = &feed.events[0];
        assert_eq!(event.end, Moment { day: day(2026, 10, 21), minute: 0 });
        assert!(event.covers(day(2026, 10, 20)));
        assert!(!event.covers(day(2026, 10, 21)), "ending at midnight is not running the next day");
        assert!(event.active_at(Moment { day: day(2026, 10, 20), minute: 1439 }), "the last minute still counts");
        assert!(!event.active_at(Moment { day: day(2026, 10, 21), minute: 0 }));
    }

    #[test]
    fn fortnightly_rule_honours_the_interval() {
        // INTERVAL=2 must skip every other week; ignoring it would double every entry.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261020T090000\nDTEND:20261020T100000\nRRULE:FREQ=WEEKLY;INTERVAL=2;COUNT=3\nSUMMARY:Sprint review\nUID:u8\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        let days: Vec<i64> = feed.events.iter().map(|e| e.start.day).collect();
        assert_eq!(days, [day(2026, 10, 20), day(2026, 11, 3), day(2026, 11, 17)]);
    }

    #[test]
    fn last_day_of_month_rule() {
        // BYMONTHDAY=-1 means "the month's last day", so the date has to follow short months:
        // a rule pinned to 31 would skip February and April entirely.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20260131T100000\nDTEND:20260131T110000\nRRULE:FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=4\nSUMMARY:Rent due\nUID:u9\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 1, 1));
        let dates: Vec<Date> = feed.events.iter().map(|e| e.start.date()).collect();
        assert_eq!(
            dates,
            [Date::new(2026, 1, 31), Date::new(2026, 2, 28), Date::new(2026, 3, 31), Date::new(2026, 4, 30)]
        );
        assert!(feed.events.iter().all(|e| e.start.minute == 10 * 60), "the time of day is kept");
    }

    #[test]
    fn a_zone_with_only_a_standard_rule_keeps_one_offset() {
        // Zones without DST publish only STANDARD. Picking a DAYLIGHT rule that does not exist
        // would leave the event an hour off, so both winter and summer use the same offset.
        let text = "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:Asia/Test\nBEGIN:STANDARD\nDTSTART:19700101T000000\nTZOFFSETFROM:+0300\nTZOFFSETTO:+0300\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VEVENT\nDTSTART;TZID=Asia/Test:20260115T090000\nDTEND;TZID=Asia/Test:20260115T100000\nSUMMARY:Winter call\nUID:u10\nEND:VEVENT\nBEGIN:VEVENT\nDTSTART;TZID=Asia/Test:20260715T090000\nDTEND;TZID=Asia/Test:20260715T100000\nSUMMARY:Summer call\nUID:u11\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 1, 1));
        for (event, date) in feed.events.iter().zip([(2026, 1, 15), (2026, 7, 15)]) {
            // 09:00 at UTC+3 is 06:00 UTC; the user's own zone does the rest.
            let expected = crate::system::clock::local_moment(day(date.0, date.1, date.2) * 86_400 + 6 * 3600)
                .map(|(day, minute)| Moment { day, minute });
            assert_eq!(Some(event.start), expected, "{} uses the STANDARD offset", event.text);
        }
    }

    #[test]
    fn an_impossible_start_date_is_skipped() {
        // Month 13 does not exist; a panic here would take down the whole feed and the panel.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261320T090000\nDTEND:20261320T100000\nSUMMARY:Never\nUID:u12\nEND:VEVENT\nBEGIN:VEVENT\nDTSTART:20261020T090000\nDTEND:20261020T100000\nSUMMARY:Real\nUID:u13\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        assert_eq!(feed.events.len(), 1, "the readable event survives the broken one");
        assert_eq!(feed.events[0].text, "Real");
    }

    #[test]
    fn several_excluded_dates_in_one_property() {
        // Calendars list exclusions comma-separated; reading only the first would let a
        // cancelled occurrence show up.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20260105T090000\nDTEND:20260105T093000\nRRULE:FREQ=DAILY;COUNT=5\nEXDATE:20260107T090000,20260108T090000\nSUMMARY:Standup\nUID:u14\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 1, 1));
        let days: Vec<i64> = feed.events.iter().map(|e| e.start.day).collect();
        assert_eq!(days, [day(2026, 1, 5), day(2026, 1, 6), day(2026, 1, 9)], "7 and 8 January are excluded");
    }

    #[test]
    fn unknown_zones_still_work() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;TZID=Somewhere/Unknown:20261020T090000\nDTEND;TZID=Somewhere/Unknown:20261020T100000\nSUMMARY:Call\nUID:u5\nEND:VEVENT\nEND:VCALENDAR\n";
        let feed = Feed::parse(text, day(2026, 10, 20));
        assert_eq!(feed.events[0].start, Moment { day: day(2026, 10, 20), minute: 540 });
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(Feed::parse("", 0).events.is_empty());
        assert!(Feed::parse("<html>not a feed</html>", 0).events.is_empty());
        assert!(Feed::parse("BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:No start\nEND:VEVENT\nEND:VCALENDAR", 0).events.is_empty());
    }
}


