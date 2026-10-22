//! Pomodoro timer: 25 minutes of focus, then a 5-minute break that starts right away.

use std::time::{Duration, Instant};

pub const FOCUS_MINUTES: u64 = 25;
pub const BREAK_MINUTES: u64 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Focus,
    Break,
}

impl Phase {
    pub fn length(self) -> Duration {
        let minutes = match self {
            Phase::Focus => FOCUS_MINUTES,
            Phase::Break => BREAK_MINUTES,
        };
        Duration::from_secs(minutes * 60)
    }
}

pub struct Focus {
    pub phase: Phase,
    /// Time left while paused or not yet started.
    paused_left: Duration,
    /// When this phase ends, while running.
    deadline: Option<Instant>,
    /// Focus sessions completed today.
    pub sessions_today: u32,
}

impl Default for Focus {
    fn default() -> Self {
        Self { phase: Phase::Focus, paused_left: Phase::Focus.length(), deadline: None, sessions_today: 0 }
    }
}

impl Focus {
    pub fn running(&self) -> bool {
        self.deadline.is_some()
    }

    /// Has been started (running, or paused midway).
    pub fn active(&self) -> bool {
        self.running() || self.paused_left < self.phase.length()
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        match self.deadline {
            Some(deadline) => deadline.saturating_duration_since(now),
            None => self.paused_left,
        }
    }

    /// Elapsed fraction, 0..1.
    pub fn progress(&self, now: Instant) -> f32 {
        1.0 - self.remaining(now).as_secs_f32() / self.phase.length().as_secs_f32()
    }

    /// Start or pause.
    pub fn toggle(&mut self, now: Instant) {
        match self.deadline.take() {
            Some(deadline) => self.paused_left = deadline.saturating_duration_since(now),
            None => self.deadline = Some(now + self.paused_left),
        }
    }

    /// Go back to the start of this phase, stopped.
    pub fn reset(&mut self) {
        self.deadline = None;
        self.paused_left = self.phase.length();
    }

    /// Switch to another phase and start over (stopped).
    pub fn select(&mut self, phase: Phase) {
        self.phase = phase;
        self.reset();
    }

    /// If time is up, switch phase and return the phase that just finished.
    /// Focus done → the break starts right away; break done → focus waits to be started.
    pub fn finish_if_due(&mut self, now: Instant) -> Option<Phase> {
        if self.deadline.is_none_or(|deadline| deadline > now) {
            return None;
        }
        let finished = self.phase;
        match finished {
            Phase::Focus => {
                self.sessions_today += 1;
                self.phase = Phase::Break;
                self.deadline = Some(now + Phase::Break.length());
            }
            Phase::Break => self.select(Phase::Focus),
        }
        Some(finished)
    }

    pub fn new_day(&mut self) {
        self.sessions_today = 0;
    }
}

/// "24:59"
pub fn clock(left: Duration) -> String {
    let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_then_break_then_idle() {
        let start = Instant::now();
        let mut focus = Focus::default();
        assert!(!focus.active());
        focus.toggle(start);
        assert!(focus.running());
        assert_eq!(focus.finish_if_due(start + Duration::from_secs(60)), None);

        let end = start + Phase::Focus.length();
        assert_eq!(focus.finish_if_due(end), Some(Phase::Focus));
        assert_eq!(focus.sessions_today, 1);
        assert_eq!(focus.phase, Phase::Break);
        assert!(focus.running(), "the break starts right away");

        assert_eq!(focus.finish_if_due(end + Phase::Break.length()), Some(Phase::Break));
        assert_eq!(focus.phase, Phase::Focus);
        assert!(!focus.running());
        assert_eq!(focus.remaining(end), Phase::Focus.length());
    }

    #[test]
    fn pause_keeps_remaining_time() {
        let start = Instant::now();
        let mut focus = Focus::default();
        focus.toggle(start);
        focus.toggle(start + Duration::from_secs(90));
        let later = start + Duration::from_secs(3600);
        assert_eq!(focus.remaining(later), Phase::Focus.length() - Duration::from_secs(90));
        assert!(focus.active());
        focus.reset();
        assert!(!focus.active());
    }

    #[test]
    fn clock_rounds_up_partial_seconds() {
        assert_eq!(clock(Duration::from_millis(1_499_200)), "25:00");
        assert_eq!(clock(Duration::from_secs(61)), "01:01");
        assert_eq!(clock(Duration::ZERO), "00:00");
    }
}
