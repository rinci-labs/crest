//! Quick timer (e.g. cooking or a short break), separate from the pomodoro.

use std::time::{Duration, Instant};

/// Duration choices on the timer page, in minutes.
pub const PRESETS: [u32; 4] = [1, 5, 10, 15];
/// Duration cap; the "+1 min" button stops here.
const MAX: Duration = Duration::from_secs(99 * 60);

#[derive(Default)]
pub struct Timer {
    /// Full duration, for the progress ring and the finished notice.
    total: Duration,
    /// When it ends, while running.
    end: Option<Instant>,
    /// Time left, while paused.
    paused: Option<Duration>,
}

impl Timer {
    pub fn start(&mut self, minutes: u32, now: Instant) {
        self.total = Duration::from_secs(minutes as u64 * 60).min(MAX);
        self.end = Some(now + self.total);
        self.paused = None;
    }

    /// Add one minute (running or paused).
    pub fn add_minute(&mut self, now: Instant) {
        let left = self.remaining(now);
        if !self.active() || left + Duration::from_secs(60) > MAX {
            return;
        }
        self.total += Duration::from_secs(60);
        match (&mut self.end, &mut self.paused) {
            (Some(end), _) => *end += Duration::from_secs(60),
            (_, Some(paused)) => *paused += Duration::from_secs(60),
            _ => {}
        }
    }

    pub fn toggle_pause(&mut self, now: Instant) {
        if let Some(end) = self.end.take() {
            self.paused = Some(end.saturating_duration_since(now));
        } else if let Some(left) = self.paused.take() {
            self.end = Some(now + left);
        }
    }

    pub fn cancel(&mut self) {
        self.end = None;
        self.paused = None;
    }

    pub fn running(&self) -> bool {
        self.end.is_some()
    }

    /// Running or paused.
    pub fn active(&self) -> bool {
        self.end.is_some() || self.paused.is_some()
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        match (self.end, self.paused) {
            (Some(end), _) => end.saturating_duration_since(now),
            (_, Some(left)) => left,
            _ => Duration::ZERO,
        }
    }

    /// Elapsed fraction, 0..1.
    pub fn progress(&self, now: Instant) -> f32 {
        if self.total.is_zero() || !self.active() {
            return 0.0;
        }
        1.0 - self.remaining(now).as_secs_f32() / self.total.as_secs_f32()
    }

    /// Full duration in minutes (rounded up).
    pub fn total_minutes(&self) -> u32 {
        self.total.as_secs().div_ceil(60) as u32
    }

    /// Finish when time is up; `true` once, when that happens.
    pub fn finish_if_due(&mut self, now: Instant) -> bool {
        if self.end.is_some_and(|end| end <= now) {
            self.end = None;
            return true;
        }
        false
    }
}

/// Short time left for the pill: "4m", or "45s" in the last minute.
pub fn short(left: Duration) -> String {
    let secs = left.as_secs_f32().ceil() as u64;
    if secs >= 60 { format!("{}m", secs.div_ceil(60)) } else { format!("{secs}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_pause_add_and_finish() {
        let t0 = Instant::now();
        let mut timer = Timer::default();
        assert!(!timer.active());
        timer.start(5, t0);
        assert_eq!(timer.remaining(t0 + Duration::from_secs(60)), Duration::from_secs(240));
        assert!((timer.progress(t0 + Duration::from_secs(150)) - 0.5).abs() < 0.01);

        timer.toggle_pause(t0 + Duration::from_secs(60));
        assert!(!timer.running() && timer.active());
        // Time does not run down while paused.
        assert_eq!(timer.remaining(t0 + Duration::from_secs(600)), Duration::from_secs(240));
        timer.add_minute(t0);
        assert_eq!(timer.remaining(t0), Duration::from_secs(300));
        assert_eq!(timer.total_minutes(), 6);

        timer.toggle_pause(t0 + Duration::from_secs(600));
        assert!(!timer.finish_if_due(t0 + Duration::from_secs(899)));
        assert!(timer.finish_if_due(t0 + Duration::from_secs(900)));
        assert!(!timer.active());
        assert!(!timer.finish_if_due(t0 + Duration::from_secs(901)), "the finished notice fires only once");
    }

    #[test]
    fn short_labels() {
        assert_eq!(short(Duration::from_secs(250)), "5m");
        assert_eq!(short(Duration::from_secs(60)), "1m");
        assert_eq!(short(Duration::from_millis(44_200)), "45s");
    }
}
