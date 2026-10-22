//! Win32 timers and everything that runs on a schedule.

use super::*;

impl App {
    pub(super) fn on_timer(&mut self, id: usize) {
        match id {
            TIMER_CLOCK => self.on_clock(),
            TIMER_OPEN => {
                self.kill_timer(TIMER_OPEN);
                if self.hovering {
                    self.set_expanded(true);
                }
            }
            TIMER_CLOSE => {
                self.kill_timer(TIMER_CLOSE);
                // The welcome stays open until its confirm button is clicked.
                if !self.hovering
                    && !self.editing()
                    && !self.shelf.drop_hover
                    && !self.menu_open
                    && self.slider_drag.is_none()
                    && !self.welcome
                {
                    self.set_expanded(false);
                }
            }
            TIMER_BLINK => {
                self.start_blink();
                self.schedule_blink();
            }
            TIMER_DESKTOP => {
                self.kill_timer(TIMER_DESKTOP);
                self.refresh_mode(true);
            }
            // On the lock screen, the topmost position is reclaimed every second.
            TIMER_POLL => self.refresh_mode(self.locked),
            TIMER_HUD => {
                self.kill_timer(TIMER_HUD);
                self.hud_visible = false;
                self.refresh();
            }
            TIMER_AMBIENT => self.on_ambient(),
            TIMER_FOCUS => {
                if let Some(finished) = self.focus.finish_if_due(Instant::now()) {
                    self.alert(Hud::Focus { finished }, FOCUS_HUD_MS, Sound::FocusDone);
                    if finished == focus::Phase::Focus {
                        self.set_mood(Mood::Happy, FOCUS_HUD_MS + MOOD_MS);
                    }
                }
                self.schedule_focus();
                self.refresh();
            }
            TIMER_LOCK_KEYS => self.on_lock_keys(),
            TIMER_GAZE => self.on_gaze(),
            TIMER_ALARM => {
                self.kill_timer(TIMER_ALARM);
                if self.timer.finish_if_due(Instant::now()) {
                    let minutes = self.timer.total_minutes();
                    self.alert(Hud::Timer { minutes }, TIMER_HUD_MS, Sound::Alarm);
                }
                self.schedule_alarm();
                self.refresh();
            }
            TIMER_WEATHER => self.fetch_weather(),
            TIMER_CALENDARS => {
                self.kill_timer(TIMER_CALENDARS);
                self.fetch_calendars();
            }
            TIMER_GOOGLE => {
                self.kill_timer(TIMER_GOOGLE);
                if self.settings.mail && google::connected() {
                    self.refresh_google();
                }
            }
            TIMER_WELCOME => {
                self.kill_timer(TIMER_WELCOME);
                if self.welcome && !self.locked {
                    self.page = Page::Home;
                    self.set_expanded(true);
                }
            }
            TIMER_QUIET => {
                self.kill_timer(TIMER_QUIET);
                self.quiet_until = None;
                self.refresh();
            }
            TIMER_MOOD => {
                self.kill_timer(TIMER_MOOD);
                self.mood = None;
                if !self.animating() {
                    self.redraw();
                }
            }
            _ => {}
        }
    }

    pub(super) fn on_ambient(&mut self) {
        if self.animating() {
            return;
        }
        // New lyrics found or the track changed: the shape changes too.
        if self.wanted_view() != self.view {
            self.refresh();
            return;
        }
        let settled = self.view.content().is_some_and(|i| self.alpha[i] == 1.0);
        // Player in the panel: the header waveform can be redrawn on its own. Full redraw when
        // the track second changes; the next lyric line scrolls up, while a jump (e.g. after
        // dragging the bar) is drawn immediately.
        if self.view.is_panel() && self.page == Page::Home && self.playing() {
            let index = self.lyric_index();
            if self.lyric_column() && index != self.lyric_drawn {
                if let (Some(new), Some(old)) = (index, self.lyric_drawn)
                    && new == old + 1
                {
                    self.start_motion();
                    self.lyric_scroll = Some(self.now());
                } else {
                    self.redraw();
                }
                return;
            }
            let second = self.media.as_ref().map(|m| m.position_now() as u64);
            if settled && second == self.second_drawn && self.page_anim.is_none() {
                let accent = self.media.as_ref().and_then(|m| m.art.as_ref()).and_then(|art| art.accent);
                let result = self
                    .renderer
                    .draw_header_wave(self.width.value, self.now(), accent)
                    .and_then(|dirty| self.renderer.present(self.hwnd, self.window_pos(), Some(&dirty)));
                if let Err(e) = result {
                    error_log!("failed to draw waveform: {e}");
                }
            } else {
                self.redraw();
            }
            return;
        }
        // A settled pill only needs its waveform updated; full redraw when the lyric line
        // changes, and occasionally so the clock and focus ring stay current.
        let pill = matches!(self.view, View::Pill | View::LyricPill);
        let lyric_moved = self.view == View::LyricPill && self.lyric_line().map(|(i, _)| i) != Some(self.lyric_shown);
        if pill && self.playing() && settled && !lyric_moved && self.now() - self.last_full < 5.0 {
            let accent = self.media.as_ref().and_then(|m| m.art.as_ref()).and_then(|art| art.accent);
            let result = self
                .renderer
                .draw_wave(self.width.value, self.height.value, self.now(), accent)
                .and_then(|dirty| self.renderer.present(self.hwnd, self.window_pos(), Some(&dirty)));
            if let Err(e) = result {
                error_log!("failed to draw waveform: {e}");
            }
        } else if lyric_moved {
            // The new line may be longer or shorter: the pill adjusts.
            self.refresh();
        } else {
            self.redraw();
        }
    }

    pub(super) fn on_clock(&mut self) {
        debug_log!(
            "last minute: hook callbacks {}, animation frames {}",
            desktop::take_callback_count(),
            FRAMES.swap(0, Ordering::Relaxed)
        );
        let today = clock::day_key(&clock::now());
        if today != self.day {
            self.day = today;
            self.focus.new_day();
        }
        self.check_water();
        self.check_agenda();
        self.check_prayer();
        self.tick_integrations();
        if !self.animating() {
            self.redraw();
        }
        self.schedule_clock();
    }

    /// Agenda items due this exact minute appear as a HUD with a reminder sound.
    pub(super) fn check_agenda(&mut self) {
        let t = clock::now();
        let due = self.agenda.due(today(), t.wHour as u32, t.wMinute as u32).first().map(|e| (e.text.clone(), e.time_text()));
        if let Some((text, time)) = due {
            self.alert(Hud::Agenda { text, time }, AGENDA_HUD_MS, Sound::Reminder);
            self.refresh();
        }
    }

    /// Remind to drink when due, only during active hours and while the goal is not met.
    pub(super) fn check_water(&mut self) {
        let now = Instant::now();
        // A hidden water page means the reminder is not in use.
        if now < self.water_due || !self.settings.pages.shown(Page::Water) {
            return;
        }
        let ml = self.water.on(&self.day);
        let awake = WATER_HOURS.contains(&clock::now().wHour);
        if awake && ml < GOAL_ML && self.shown && !self.expanded {
            self.alert(Hud::Water { ml, goal: GOAL_ML }, WATER_HUD_MS, Sound::Reminder);
            self.refresh();
        }
        self.water_due = now + WATER_AGAIN;
    }

    pub(super) fn schedule_focus(&self) {
        if self.focus.running() {
            let left = self.focus.remaining(Instant::now()).as_millis().min(u32::MAX as u128) as u32;
            self.set_timer(TIMER_FOCUS, left.saturating_add(20));
        } else {
            self.kill_timer(TIMER_FOCUS);
        }
    }

    /// Meeting mode is active: the camera or microphone is in use.
    pub(super) fn in_meeting(&self) -> bool {
        self.settings.meeting && self.privacy.active()
    }

    pub(super) fn quiet(&self) -> bool {
        self.quiet_until.is_some_and(|until| Instant::now() < until)
    }

    /// Schedule the timer-finished HUD.
    pub(super) fn schedule_alarm(&self) {
        if self.timer.running() {
            let left = self.timer.remaining(Instant::now()).as_millis().min(u32::MAX as u128) as u32;
            self.set_timer(TIMER_ALARM, left.saturating_add(20));
        } else {
            self.kill_timer(TIMER_ALARM);
        }
    }

    pub(super) fn schedule_blink(&mut self) {
        // xorshift32: enough to make blink intervals feel random.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.set_timer(TIMER_BLINK, 3_500 + self.rng % 4_000);
    }

    pub(super) fn schedule_clock(&self) {
        self.set_timer(TIMER_CLOCK, clock::ms_to_next_minute(&clock::now()));
    }

    pub(super) fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    pub(super) fn set_timer(&self, id: usize, ms: u32) {
        unsafe { SetTimer(Some(self.hwnd), id, ms, None) };
    }

    pub(super) fn kill_timer(&self, id: usize) {
        let _ = unsafe { KillTimer(Some(self.hwnd), id) };
    }
}
