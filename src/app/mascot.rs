//! The mascot: waking, gaze, blinking, and mood.

use super::*;

impl App {
    /// Start the mascot wake-up animation.
    pub(super) fn wake_up(&mut self) {
        self.start_motion();
        self.wake = Some(self.now());
    }

    /// The mascot's eyes are visible: on the pill with no music/focus session, or on the clock page.
    pub(super) fn eyes_visible(&self) -> bool {
        self.shown
            && match self.view {
                View::Pill => !self.playing() && !self.focus.running() && !self.timer.running(),
                View::Panel | View::PanelWide => self.page == Page::Home && self.media.is_none() && self.page_anim.is_none(),
                _ => false,
            }
    }

    /// Sample the cursor position only while the eyes are visible.
    pub(super) fn update_gaze_poll(&mut self) {
        let visible = self.eyes_visible();
        let interval = visible.then(|| if self.cursor_moved.elapsed() >= GAZE_REST { GAZE_IDLE_MS } else { GAZE_MS });
        if interval != self.gaze_poll {
            self.gaze_poll = interval;
            match interval {
                Some(ms) => self.set_timer(TIMER_GAZE, ms),
                None => self.kill_timer(TIMER_GAZE),
            }
        }
        // Hidden eyes recenter, so they do not reappear mid-glance.
        if !visible {
            self.gaze = (0.0, 0.0);
            self.gaze_target = (0.0, 0.0);
        }
    }

    /// The eyes glance toward the cursor; if it stays still for long, they look ahead again.
    pub(super) fn on_gaze(&mut self) {
        let mut cursor = POINT::default();
        if !self.eyes_visible() || unsafe { GetCursorPos(&mut cursor) }.is_err() {
            self.update_gaze_poll();
            return;
        }
        let now = Instant::now();
        if cursor != self.cursor {
            self.cursor = cursor;
            self.cursor_moved = now;
        }
        let target = if now.duration_since(self.cursor_moved) >= GAZE_REST {
            (0.0, 0.0)
        } else {
            let (pos, size, scale) = (self.window_pos(), self.renderer.size(), self.renderer.scale());
            // Eye position in DIP from the top center of the canvas: left of the pill, or right of
            // the clock page (right of the normal panel, or right of the wide Home's left column).
            // Pages are drawn `CONTENT_SHIFT` below their own coordinates.
            let hero = crate::ui::tokens::HERO_Y + crate::ui::tokens::CONTENT_SHIFT;
            let (ex, ey) = if self.view == View::Panel {
                (View::Panel.size().0 / 2.0 - crate::ui::tokens::INSET - 40.0, hero)
            } else if self.view == View::PanelWide {
                (18.0, hero)
            } else {
                (18.0 - self.width.value / 2.0, self.height.value / 2.0)
            };
            let eye_x = pos.x as f32 + size.cx as f32 / 2.0 + ex * scale;
            let eye_y = pos.y as f32 + ey * scale;
            // Full glance when the cursor is about a third of the screen away.
            let reach = ((self.monitor_rect.right - self.monitor_rect.left) as f32 / 3.0).max(1.0);
            (((cursor.x as f32 - eye_x) / reach).clamp(-1.0, 1.0), ((cursor.y as f32 - eye_y) / (reach * 0.6)).clamp(-1.0, 1.0))
        };
        let moved = (target.0 - self.gaze_target.0).abs().max((target.1 - self.gaze_target.1).abs());
        if moved > 0.05 {
            self.start_motion();
            self.gaze_target = target;
        }
        self.update_gaze_poll();
    }

    /// Mascot expression: temporary (happy/surprised), sleepy at night, or normal.
    pub(super) fn mood(&self) -> Mood {
        match self.mood {
            Some((mood, until)) if Instant::now() < until => mood,
            _ if night(clock::now().wHour) => Mood::Sleepy,
            _ => Mood::Normal,
        }
    }

    pub(super) fn set_mood(&mut self, mood: Mood, ms: u32) {
        self.mood = Some((mood, Instant::now() + Duration::from_millis(ms as u64)));
        self.set_timer(TIMER_MOOD, ms + 20);
    }

    pub(super) fn eyes(&self) -> Eyes {
        Eyes { open: self.eyes_open(), look: self.gaze, mood: self.mood() }
    }

    pub(super) fn start_blink(&mut self) {
        // Smiling eyes ("^ ^") do not blink.
        if self.eyes_visible() && self.mood() != Mood::Happy {
            self.start_motion();
            self.blink_since = Some(self.now());
        }
    }

    pub(super) fn eyes_open(&self) -> f32 {
        // Waking: closed briefly, then opening slowly (eased at start and end).
        if let Some(start) = self.wake {
            let t = ((self.now() - start - WAKE_CLOSED_SECS) / WAKE_OPEN_SECS).clamp(0.0, 1.0) as f32;
            return t * t * (3.0 - 2.0 * t);
        }
        match self.blink_since {
            Some(start) => {
                let t = ((self.now() - start) / BLINK_SECS).clamp(0.0, 1.0) as f32;
                1.0 - (t * std::f32::consts::PI).sin()
            }
            None => 1.0,
        }
    }
}
