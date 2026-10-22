//! Clock strings for the scene, rebuilt once per minute instead of on every animation frame.

use super::*;

#[derive(Default)]
pub(super) struct ClockText {
    /// (day, hour, minute) the strings were built for.
    stamp: Option<(u16, u16, u16)>,
    pub(super) time: String,
    pub(super) date: String,
    pub(super) greeting: &'static str,
    pub(super) uptime: String,
}

impl ClockText {
    /// Rebuild the strings if the minute changed since the last call.
    pub(super) fn refresh(&mut self) {
        let t = clock::now();
        let stamp = (t.wDay, t.wHour, t.wMinute);
        if self.stamp == Some(stamp) {
            return;
        }
        self.stamp = Some(stamp);
        self.time = clock::time(&t);
        self.date = clock::date(&t);
        self.greeting = clock::greeting(&t);
        self.uptime = clock::uptime();
    }
}
