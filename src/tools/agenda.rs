//! Personal agenda for the calendar on the Home page. Typed directly into Crest and stored in
//! `agenda.txt` as "2026-10-20|09:00|0|Team meeting" (time "-" = all day, 1 = done),
//! so it stays readable and editable in Notepad. No accounts and no internet.

use crate::shell::store;

const FILE: &str = "agenda.txt";
pub const MAX_LEN: usize = 60;
/// Events that fit in the calendar column at once; the rest scroll with the mouse wheel.
pub const VISIBLE: usize = 2;
/// Events older than this are dropped on load.
const KEEP_DAYS: i64 = 60;

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Plain calendar date.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    /// Days since 1970-01-01 (Howard Hinnant's "days from civil" algorithm).
    pub fn days(self) -> i64 {
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
        Self { year, month, day }
    }

    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.days() + n)
    }

    /// Same day `n` months later, clamped to the month's last day (31 Jan + 1 month = 28 Feb).
    pub fn add_months(self, n: i64) -> Self {
        let months = self.year as i64 * 12 + self.month as i64 - 1 + n;
        let year = months.div_euclid(12) as i32;
        let month = months.rem_euclid(12) as u32 + 1;
        let mut date = Self { year, month, day: 1 };
        date.day = self.day.min(date.days_in_month());
        date
    }

    /// Days in this date's month.
    pub fn days_in_month(self) -> u32 {
        let month = self.month;
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ if (self.year % 4 == 0 && self.year % 100 != 0) || self.year % 400 == 0 => 29,
            _ => 28,
        }
    }

    /// Weekday as 0 (Sunday) through 6 (Saturday), matching `RRULE`'s `BYDAY`.
    pub fn weekday_index(self) -> u32 {
        (self.days() + 4).rem_euclid(7) as u32
    }

    /// Short weekday name, e.g. "Mon".
    pub fn weekday(self) -> &'static str {
        // 1970-01-01 was a Thursday.
        DAYS[((self.days() + 4).rem_euclid(7)) as usize]
    }

    /// Short month name, e.g. "Oct".
    pub fn month_name(self) -> &'static str {
        MONTHS[(self.month as usize + 11) % 12]
    }

    fn key(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok().filter(|m| (1..=12).contains(m))?;
        let day = parts.next()?.parse().ok().filter(|d| (1..=31).contains(d))?;
        Some(Self { year, month, day })
    }
}

pub struct Event {
    pub id: u64,
    pub date: Date,
    /// Hour and minute; `None` = all day.
    pub time: Option<(u32, u32)>,
    pub text: String,
    pub done: bool,
}

impl Event {
    /// "09.00", or empty for all-day events.
    pub fn time_text(&self) -> String {
        self.time.map(|(h, m)| format!("{h:02}.{m:02}")).unwrap_or_default()
    }
}

#[derive(Default)]
pub struct Agenda {
    pub events: Vec<Event>,
    next_id: u64,
    /// Text being typed while the add-event box is active.
    pub draft: Option<String>,
    pub scroll: usize,
}

impl Agenda {
    pub fn load(today: Date) -> Self {
        Self::parse(&store::load(FILE), today)
    }

    pub fn save(&self) {
        store::save(FILE, self.lines());
    }

    fn parse(lines: &[String], today: Date) -> Self {
        let mut agenda = Self::default();
        let oldest = today.add_days(-KEEP_DAYS);
        for line in lines {
            let mut parts = line.splitn(4, '|');
            let (Some(date), Some(time), Some(done), Some(text)) = (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Some(date) = Date::parse(date.trim()).filter(|d| *d >= oldest) else { continue };
            let text = clean(text);
            if !text.is_empty() {
                agenda.push(date, parse_time(time.trim()), text, done.trim() == "1");
            }
        }
        agenda
    }

    fn lines(&self) -> Vec<String> {
        let mut sorted: Vec<&Event> = self.events.iter().collect();
        sorted.sort_by_key(|e| (e.date, e.time));
        sorted
            .iter()
            .map(|e| {
                let time = e.time.map_or("-".to_string(), |(h, m)| format!("{h:02}:{m:02}"));
                format!("{}|{time}|{}|{}", e.date.key(), if e.done { 1 } else { 0 }, e.text)
            })
            .collect()
    }

    fn push(&mut self, date: Date, time: Option<(u32, u32)>, text: String, done: bool) {
        self.next_id += 1;
        self.events.push(Event { id: self.next_id, date, time, text, done });
    }

    /// Add an event from typed text, e.g. "09.00 Team meeting" or "Buy a gift 15:30".
    /// A time at the start or end of the text becomes its time. `false` if the text is empty.
    pub fn add(&mut self, date: Date, typed: &str) -> bool {
        let (time, text) = split_time(typed);
        let text = clean(&text);
        if text.is_empty() {
            return false;
        }
        self.push(date, time, text, false);
        true
    }

    /// Events on `date`, sorted by time (all-day first).
    pub fn on(&self, date: Date) -> Vec<&Event> {
        let mut day: Vec<&Event> = self.events.iter().filter(|e| e.date == date).collect();
        day.sort_by_key(|e| (e.time.is_some(), e.time));
        day
    }

    pub fn has(&self, date: Date) -> bool {
        self.events.iter().any(|e| e.date == date)
    }

    pub fn toggle(&mut self, id: u64) {
        if let Some(event) = self.events.iter_mut().find(|e| e.id == id) {
            event.done = !event.done;
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.events.retain(|e| e.id != id);
    }

    /// Unfinished events due exactly at this minute, for reminders.
    pub fn due(&self, date: Date, hour: u32, minute: u32) -> Vec<&Event> {
        self.events.iter().filter(|e| e.date == date && e.time == Some((hour, minute)) && !e.done).collect()
    }

    pub fn scroll_by(&mut self, step: i32, count: usize) {
        let max = count.saturating_sub(VISIBLE);
        self.scroll = ((self.scroll as i32 + step).max(0) as usize).min(max);
    }
}

/// "09.00", "9:30", "21.15" → (hour, minute). Minutes must be two digits so plain numbers like
/// "3.5" are not mistaken for a time.
fn parse_time(token: &str) -> Option<(u32, u32)> {
    let (h, m) = token.split_once(['.', ':'])?;
    if !(1..=2).contains(&h.len()) || m.len() != 2 {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// Split a time at the start or end of the text from the rest.
fn split_time(typed: &str) -> (Option<(u32, u32)>, String) {
    let words: Vec<&str> = typed.split_whitespace().collect();
    if let Some(time) = words.first().and_then(|w| parse_time(w)) {
        return (Some(time), words[1..].join(" "));
    }
    if words.len() > 1
        && let Some(time) = words.last().and_then(|w| parse_time(w))
    {
        return (Some(time), words[..words.len() - 1].join(" "));
    }
    (None, words.join(" "))
}

fn clean(text: &str) -> String {
    let text: String = text.chars().filter(|c| !c.is_control() && *c != '|').collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_move_across_months_and_years() {
        let d = Date::new(2026, 10, 20);
        assert_eq!(d.weekday(), "Tue");
        assert_eq!(d.month_name(), "Oct");
        assert_eq!(d.add_days(12), Date::new(2026, 11, 1));
        assert_eq!(Date::new(2026, 12, 30).add_days(3), Date::new(2027, 1, 2));
        assert_eq!(Date::new(2028, 3, 1).add_days(-1), Date::new(2028, 2, 29), "leap year");
        assert_eq!(Date::new(1970, 1, 1).weekday(), "Thu");
    }

    #[test]
    fn typed_times_are_understood() {
        assert_eq!(split_time("09.00 Team meeting"), (Some((9, 0)), "Team meeting".to_string()));
        assert_eq!(split_time("Buy a gift 15:30"), (Some((15, 30)), "Buy a gift".to_string()));
        assert_eq!(split_time("Jogging"), (None, "Jogging".to_string()));
        assert_eq!(split_time("Meeting 25.00"), (None, "Meeting 25.00".to_string()), "an invalid time stays as text");
        assert_eq!(split_time("Price 3.5"), (None, "Price 3.5".to_string()));
    }

    #[test]
    fn add_sort_round_trip_and_prune() {
        let today = Date::new(2026, 10, 20);
        let mut agenda = Agenda::default();
        assert!(agenda.add(today, "15.30 Buy a gift"));
        assert!(agenda.add(today, "Team meeting 09:00"));
        assert!(agenda.add(today, "Mom's birthday"));
        assert!(!agenda.add(today, "   "));
        let texts: Vec<&str> = agenda.on(today).iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["Mom's birthday", "Team meeting", "Buy a gift"]);
        assert_eq!(agenda.due(today, 9, 0).len(), 1);

        let id = agenda.on(today)[1].id;
        agenda.toggle(id);
        assert!(agenda.due(today, 9, 0).is_empty(), "done events are not reminded");

        let mut lines = agenda.lines();
        lines.push("2026-01-01|-|0|Long past".to_string());
        lines.push("broken".to_string());
        let loaded = Agenda::parse(&lines, today);
        assert_eq!(loaded.events.len(), 3);
        assert!(loaded.on(today).iter().any(|e| e.done && e.text == "Team meeting"));
    }
}
