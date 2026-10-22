//! Prayer times for the saved city: computed once a day, checked every minute for notices.

use super::*;
use crate::tools::prayer::{self, Prayer};

impl App {
    /// Recompute today's times. Called at start, when the date, city, or method changes.
    pub(super) fn update_prayer(&mut self) {
        self.prayer = self.city.as_ref().map(|city| {
            let date = today();
            let minutes = |date: Date| -> [Option<u32>; 6] {
                let seconds = prayer::utc_seconds(date, city.lat, city.lon, self.settings.prayer_method, self.settings.prayer_madhab);
                // The wall clock in the user's zone (with its daylight saving rules) for each instant.
                seconds.map(|s| s.and_then(|s| clock::local_moment(date.days() * 86_400 + s)).map(|(_, minute)| minute))
            };
            prayer::Day { day: date.days(), minutes: minutes(date), tomorrow_fajr: minutes(date.add_days(1))[0] }
        });
    }

    /// A reminder `prayer_lead` minutes before each azan, and a notice when it is time. A hidden
    /// Prayer page means the feature is not in use.
    pub(super) fn check_prayer(&mut self) {
        if !self.settings.pages.shown(Page::Prayer) {
            return;
        }
        if self.prayer.is_none_or(|day| day.day != today().days()) {
            self.update_prayer();
        }
        let t = clock::now();
        let now = t.wHour as u32 * 60 + t.wMinute as u32;
        let Some(day) = self.prayer else { return };
        if let Some((which, ahead)) = day.due(now, self.settings.prayer_lead) {
            let time = day.minutes[Prayer::ALL.iter().position(|p| *p == which).unwrap_or(0)].map(prayer::clock).unwrap_or_default();
            self.alert(Hud::Prayer { name: which.label(), time, ahead }, PRAYER_HUD_MS, Sound::Reminder);
            self.refresh();
        }
    }

    pub(super) fn prayer_view(&self) -> PrayerView {
        let t = clock::now();
        let now = t.wHour as u32 * 60 + t.wMinute as u32;
        let next = self.prayer.and_then(|day| day.next(now));
        let times = Prayer::ALL
            .iter()
            .enumerate()
            .map(|(i, p)| PrayerTime {
                name: p.label(),
                time: self.prayer.and_then(|day| day.minutes[i]).map(prayer::clock).unwrap_or_else(|| "--:--".to_string()),
                passed: self.prayer.and_then(|day| day.minutes[i]).is_some_and(|m| m <= now),
                // Tomorrow's Fajr is not today's tile, so nothing lights up after Isha.
                next: next.is_some_and(|(n, m, _)| n == *p && self.prayer.and_then(|day| day.minutes[i]) == Some(m)),
            })
            .collect();
        PrayerView {
            city: self.city.as_ref().map(|c| c.name.clone()),
            status: self.city_status.clone(),
            times,
            next: next.map(|(p, minute, left)| (p.label(), prayer::clock(minute), prayer::countdown(left))),
            method: self.settings.prayer_method,
            lead: self.settings.prayer_lead,
        }
    }
}
