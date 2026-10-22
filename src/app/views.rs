//! Builds the read-only view structs handed to the renderer.

use super::*;

impl App {
    /// Day currently selected in the calendar.
    pub(super) fn selected_day(&self) -> Date {
        today().add_days(self.cal_offset as i64)
    }

    /// Calendar column for the wide Home page.
    pub(super) fn calendar_view(&self) -> Option<CalendarView> {
        if !self.settings.calendar || self.locked {
            return None;
        }
        let (today, selected) = (today(), self.selected_day());
        let days = (-2..=2)
            .map(|k| {
                let day = selected.add_days(k as i64);
                DayView {
                    offset: self.cal_offset + k,
                    name: day.weekday(),
                    number: day.day,
                    today: day == today,
                    selected: k == 0,
                    has_events: self.has_events(day),
                }
            })
            .collect();
        let events = self.calendar_entries(selected, today);
        Some(CalendarView {
            month: selected.month_name().to_string(),
            year: selected.year.to_string(),
            days,
            events,
            scroll: self.agenda.scroll,
        })
    }

    /// Does anything happen on `day`? The day strip puts a dot under a day with a typed agenda
    /// entry, an event from a subscribed feed, or a Google event.
    fn has_events(&self, day: Date) -> bool {
        let number = day.days();
        self.agenda.has(day)
            || self.feeds.feeds.iter().any(|feed| feed.has(number))
            || self.google_events.iter().any(|event| event.day == number)
    }

    /// One day's rows: typed agenda entries first (they can be ticked off), then feed and
    /// Google events in start order.
    fn calendar_entries(&self, selected: Date, today: Date) -> Vec<CalendarEntry> {
        let now = clock::now();
        let current = (today == selected).then_some(now.wHour as u32 * 60 + now.wMinute as u32);
        let mut entries: Vec<CalendarEntry> = self
            .agenda
            .on(selected)
            .into_iter()
            .map(|e| CalendarEntry {
                id: Some(e.id),
                time: e.time_text(),
                text: e.text.clone(),
                done: e.done,
                source: None,
                now: false,
            })
            .collect();
        let day = selected.days();
        let mut remote: Vec<CalendarEntry> = self
            .feeds
            .feeds
            .iter()
            .flat_map(|feed| feed.on(day).map(move |event| (feed, event)))
            .filter(|(_, event)| !event.text.is_empty())
            .map(|(feed, event)| CalendarEntry {
                id: None,
                time: event.minute_text(),
                text: event.text.clone(),
                done: false,
                source: Some(feed.name.clone()).filter(|n| !n.is_empty()),
                now: current.is_some_and(|minute| event.active_at(calfeed::Moment { day, minute })),
            })
            .collect();
        remote.extend(self.google_events.iter().filter(|event| event.day == day).map(|event| {
            let minute = event.minute.unwrap_or(0);
            CalendarEntry {
                id: None,
                time: event.minute.map_or_else(String::new, |m| format!("{:02}.{:02}", m / 60, m % 60)),
                text: event.summary.clone(),
                done: false,
                source: Some("Google".to_string()),
                now: event.minute.is_some()
                    && current.is_some_and(|now| minute <= now && now < minute + 60),
            }
        }));
        // All-day entries stay above timed ones; the rest run in time order.
        remote.sort_by(|a, b| (a.time.is_empty(), &a.time).cmp(&(b.time.is_empty(), &b.time)));
        entries.extend(remote);
        entries
    }

    pub(super) fn timer_view(&self) -> TimerView {
        let now = Instant::now();
        let left = self.timer.remaining(now);
        TimerView {
            running: self.timer.running(),
            active: self.timer.active(),
            clock: focus::clock(left),
            short: timer::short(left),
            progress: self.timer.progress(now),
            total_minutes: self.timer.total_minutes(),
        }
    }

    pub(super) fn controls_view(&self) -> ControlsView {
        let quiet = self.quiet_until.filter(|_| self.quiet());
        // The city button also shows the temperature, since the music player covers the clock page.
        let fresh = self.weather.as_ref().filter(|(_, at)| at.elapsed() < WEATHER_STALE);
        let city = self.settings.weather.then(|| {
            self.city_status.clone().unwrap_or_else(|| match (&self.city, fresh) {
                (Some(city), Some((weather, _))) => format!("{} · {:.0}°", city.name, weather.temp),
                (Some(city), None) => format!("City: {}", city.name),
                (None, _) => "Set weather city".to_string(),
            })
        });
        ControlsView {
            volume: self.volume,
            brightness: self.brightness,
            mic: self.mic,
            quiet_minutes: quiet.map(|until| until.saturating_duration_since(Instant::now()).as_secs().div_ceil(60) as u32),
            city,
        }
    }

    /// Rebuild the pages that render many rows, so that drawing a frame neither clones their
    /// rows nor formats their text. Call it where those inputs change, not every frame.
    pub(super) fn rebuild_views(&mut self) {
        self.mail_view = self.build_mail();
        self.plugins_view = self.build_plugins();
    }

    /// Unread mail, or the reason the page has none.
    fn build_mail(&self) -> MailView {
        // The lock screen shows nothing private: no subjects, no senders, no account.
        if self.locked {
            return MailView {
                account: None,
                status: None,
                rows: Vec::new(),
                setup: false,
                id: String::new(),
                secret: String::new(),
                editing: None,
                connecting: false,
            };
        }
        if !self.settings.mail {
            return MailView {
                account: None,
                status: Some("Mail is off".to_string()),
                rows: Vec::new(),
                setup: false,
                id: self.google_id.clone(),
                secret: self.google_secret.clone(),
                editing: None,
                connecting: false,
            };
        }
        // Until an account is connected the page holds the setup boxes, whichever part of the
        // client is already saved: a missing secret could otherwise never be filled in again.
        let setup = !google::connected();
        MailView {
            account: self.account.clone(),
            status: self.mail_status.clone(),
            rows: self
                .mail
                .iter()
                .map(|m| MailRow { from: m.from.clone(), subject: m.subject.clone(), age: m.age.clone(), initials: m.initials.clone() })
                .collect(),
            setup,
            id: match self.edit_field {
                Some(Field::GoogleId) => self.edit_text.clone(),
                _ => self.google_id.clone(),
            },
            secret: match self.edit_field {
                Some(Field::GoogleSecret) => self.edit_text.clone(),
                _ => self.google_secret.clone(),
            },
            editing: match self.edit_field {
                Some(Field::GoogleId) => Some(0),
                Some(Field::GoogleSecret) => Some(1),
                _ => None,
            },
            connecting: self.connecting,
        }
    }

    /// Plugin rows and the plugins that failed.
    fn build_plugins(&self) -> PluginsView {
        let count = self.plugins.list.len();
        PluginsView { rows: self.plugins.rows(), problems: self.plugins.problems(), count, empty: count == 0 }
    }

    pub(super) fn weather_text(&self) -> Option<String> {
        let (weather, _) = self.weather.as_ref().filter(|(_, at)| at.elapsed() < WEATHER_STALE)?;
        let city = self.city.as_ref().filter(|_| self.settings.weather)?;
        let desc = weather::describe(weather.code);
        let sky = if desc.is_empty() { String::new() } else { format!(" {desc}") };
        Some(format!("{:.0}°{sky} · {}", weather.temp, city.name))
    }

    pub(super) fn focus_view(&self) -> FocusView {
        let now = Instant::now();
        let left = self.focus.remaining(now);
        FocusView {
            phase: self.focus.phase,
            running: self.focus.running(),
            active: self.focus.active(),
            clock: focus::clock(left),
            progress: self.focus.progress(now),
            sessions: self.focus.sessions_today,
            minutes_left: left.as_secs().div_ceil(60) as u32,
        }
    }

    pub(super) fn water_view(&self) -> WaterView {
        let minutes = self.water_due.saturating_duration_since(Instant::now()).as_secs().div_ceil(60);
        WaterView {
            ml: self.water.on(&self.day),
            goal: GOAL_ML,
            week: clock::recent_days(7).iter().map(|(day, initial)| (self.water.on(day), *initial)).collect(),
            next: Some(clock::time_in(minutes as u32)),
        }
    }
}
