//! Shape, visibility, animation, and drawing of each frame.

use super::*;

impl App {
    /// Reclaim the topmost position (e.g. above the lock screen).
    pub(super) fn raise(&self) {
        let _ = unsafe {
            SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
        };
    }

    pub(super) fn set_expanded(&mut self, expanded: bool) {
        if self.expanded == expanded || !self.shown {
            return;
        }
        if !expanded {
            self.end_edit(true, true);
        }
        self.expanded = expanded;
        if expanded {
            self.hud_visible = false;
            self.kill_timer(TIMER_HUD);
            if self.page == Page::Shelf && self.shelf.prune_missing() {
                self.shelf_changed();
            }
            // A small greeting: the mascot blinks shortly after the panel opens.
            self.set_timer(TIMER_BLINK, 650);
        } else {
            self.hovered = None;
            self.page_anim = None;
            // The calendar returns to today when the panel reopens.
            self.cal_offset = 0;
            self.agenda.scroll = 0;
        }
        self.refresh();
    }

    pub(super) fn wanted_view(&self) -> View {
        // A thin line when the active window is maximized, also while the camera/mic is in use
        // (its privacy dot shows at the end of the line). A line touched by the cursor turns back into a pill first.
        let slim = self.mode == Mode::Maximized && !self.locked && !self.peek;
        if self.expanded {
            // Wide Home with the calendar (not on the lock screen: the agenda is private), or
            // with the lyrics column beside the player.
            let calendar = self.settings.calendar && !self.locked;
            let wide = self.page == Page::Home && (calendar || self.lyric_column()) && !self.welcome;
            if wide {
                View::PanelWide
            } else if self.page == Page::Settings && !self.welcome {
                View::PanelTall
            } else {
                View::Panel
            }
        } else if self.hud_visible {
            if self.compact_hud() { View::Strip } else { View::Hud }
        } else if slim {
            View::Slim
        } else if self.playing() && self.lyric_line().is_some() {
            View::LyricPill
        } else {
            View::Pill
        }
    }

    /// Point the shape at the latest state, then redraw if needed.
    pub(super) fn refresh(&mut self) {
        let view = self.wanted_view();
        let target = self.view_size(view);
        if view != self.view || target != self.target {
            self.start_motion();
            let opening = target.0 * target.1 > self.width.value * self.height.value;
            let bounce = if opening { OPEN_BOUNCE } else { CLOSE_BOUNCE };
            self.width.set_damping_ratio(bounce);
            self.height.set_damping_ratio(bounce);
            if view != self.view {
                debug_log!("shape: {:?} -> {view:?}", self.view);
                self.from = (self.width.value, self.height.value);
                self.panel_resize = self.view.is_panel() && view.is_panel();
            } else if self.transition() >= 1.0 {
                // Only the size changed (new lyric line): the content does not need to fade.
                self.from = target;
            }
            self.view = view;
            self.target = target;
            self.width.target = target.0;
            self.height.target = target.1;
        }
        // The lyrics column only slides while the panel is open; when closed it snaps into place.
        if !self.expanded {
            self.lyric_mix = self.lyric_goal();
        }
        self.update_ambient();
        self.update_system_watch();
        if !self.animating() {
            self.redraw();
        }
    }

    /// Jump to the target shape without animation (e.g. when Crest just reappeared).
    pub(super) fn snap_view(&mut self) {
        self.view = self.wanted_view();
        let (w, h) = self.view_size(self.view);
        self.target = (w, h);
        self.width.snap(w);
        self.height.snap(h);
        self.from = (w, h);
        self.panel_resize = false;
        self.lyric_mix = self.lyric_goal();
        self.alpha = self.alpha_goals();
    }

    /// Shape size; the lyrics pill widens, or wraps to two lines, to fit its lyric line.
    /// The width is also capped by the monitor width, so the shape does not cover a narrow screen.
    pub(super) fn view_size(&self, view: View) -> (f32, f32) {
        let (w, h) = match (view, self.lyric_line()) {
            (View::LyricPill, Some((_, line))) => self.renderer.lyric_size(&line),
            _ => view.size(),
        };
        let w = if view == View::Pill && self.download_text().is_some() { w + DOWNLOAD_GROW } else { w };
        let w = w.min(self.max_width());
        match view {
            View::Pill | View::LyricPill if self.peek => (w + PEEK_GROW.0, h + PEEK_GROW.1),
            _ => (w, h),
        }
    }

    /// Widest shape width that still fits this monitor (DIP).
    pub(super) fn max_width(&self) -> f32 {
        let monitor = (self.monitor_rect.right - self.monitor_rect.left) as f32 / self.renderer.scale();
        (monitor * ui::tokens::MAX_SCREEN_SHARE).max(240.0)
    }

    /// Size of the running download for the pill, while no music or timer is active.
    pub(super) fn download_text(&self) -> Option<String> {
        let (count, bytes) = self.downloading.filter(|_| !self.playing() && !self.timer.running())?;
        let size = crate::system::stats::size(bytes);
        Some(if count > 1 { format!("{count} · {size}") } else { size })
    }

    /// Full shadow while the panel is open, light when the cursor touches the pill.
    pub(super) fn shadow_goal(&self) -> f32 {
        if !self.shown {
            0.0
        } else if self.expanded {
            1.0
        } else if self.peek {
            0.6
        } else {
            0.0
        }
    }

    /// Timer for self-animating content, only while something is actually running.
    pub(super) fn update_ambient(&mut self) {
        let playing = self.playing();
        let focusing = self.focus.running();
        let timing = self.timer.running();
        let last_minute = self.timer.remaining(Instant::now()) < Duration::from_secs(60);
        let interval = match self.view {
            _ if !self.shown => None,
            v if v.is_panel() && self.page == Page::Shelf && self.shelf.drop_hover => Some(DASH_MS),
            View::Pill | View::LyricPill if playing => Some(WAVE_MS),
            // In the last minute the pill counts seconds.
            View::Pill if timing => Some(if last_minute { PROGRESS_MS } else { PILL_FOCUS_MS }),
            View::Pill if focusing => Some(PILL_FOCUS_MS),
            v if v.is_panel() && self.page == Page::Timer && timing => Some(FOCUS_TICK_MS),
            // Player header waveform; the track second and lyric line are checked along with it.
            v if v.is_panel() && self.page == Page::Home && playing => Some(WAVE_MS),
            v if v.is_panel() && self.page == Page::Focus && focusing => Some(FOCUS_TICK_MS),
            _ => None,
        };
        if interval != self.ambient {
            self.ambient = interval;
            match interval {
                Some(ms) => self.set_timer(TIMER_AMBIENT, ms),
                None => self.kill_timer(TIMER_AMBIENT),
            }
        }
        self.update_gaze_poll();
    }

    /// Recheck the active window: full-screen, maximized, or normal. `raise` reclaims the
    /// topmost position, only needed when the active window changes (not on periodic checks).
    pub(super) fn refresh_mode(&mut self, raise: bool) {
        let mode = desktop::current_mode(self.hwnd, self.monitor, &self.monitor_rect);
        if mode != self.mode {
            debug_log!("mode: {mode:?}");
            self.mode = mode;
            self.apply_visibility();
            self.refresh();
        }
        if raise && self.shown {
            // Another "always on top" window may have risen above Crest; reclaim the topmost position.
            self.raise();
        }
    }

    pub(super) fn apply_visibility(&mut self) {
        // The lock screen is detected as a full-screen window, but Crest stays visible there.
        let want = !self.user_hidden && (self.locked || self.mode != Mode::Fullscreen);
        if want == self.shown {
            return;
        }
        if !want {
            self.end_edit(true, false);
        }
        self.shown = want;
        if want {
            self.snap_view();
            self.redraw();
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
            self.schedule_blink();
        } else {
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
            // Reappear later as a closed pill, not a panel that is still open.
            self.expanded = false;
            self.hovering = false;
            self.tracking_leave = false;
            self.hovered = None;
            self.hud_visible = false;
            self.blink_since = None;
            self.page_anim = None;
            self.ambient = None;
            self.gaze_poll = None;
            for timer in [TIMER_OPEN, TIMER_CLOSE, TIMER_BLINK, TIMER_HUD, TIMER_AMBIENT, TIMER_GAZE] {
                self.kill_timer(timer);
            }
        }
    }

    /// Reposition on the primary monitor, e.g. after the resolution or display scale changes.
    pub(super) fn place(&mut self) {
        let (monitor, rect) = primary_monitor();
        self.monitor = monitor;
        self.monitor_rect = rect;
        if let Err(e) = self.renderer.set_dpi(scaled_dpi(monitor_dpi(monitor), self.settings.size)) {
            error_log!("failed to change DPI: {e}");
        }
        self.redraw();
    }

    /// How far the shape transition has progressed, 0..1.
    pub(super) fn transition(&self) -> f32 {
        let (tw, th) = self.target;
        let total = (self.from.0 - tw).hypot(self.from.1 - th);
        if total < 0.5 {
            return 1.0;
        }
        (1.0 - (self.width.value - tw).hypot(self.height.value - th) / total).clamp(0.0, 1.0)
    }

    /// The target shape's content only appears once the shape has nearly arrived; other content fades.
    pub(super) fn alpha_goals(&self) -> [f32; CONTENTS] {
        let mut goals = [0.0; CONTENTS];
        if let Some(i) = self.view.content()
            && (self.transition() > 0.6 || self.panel_resize)
        {
            goals[i] = 1.0;
        }
        goals
    }

    pub(super) fn animating(&self) -> bool {
        self.shown
            && (self.width.is_moving()
                || self.height.is_moving()
                || self.alpha != self.alpha_goals()
                || self.blink_since.is_some()
                || self.page_anim.is_some()
                || self.gaze != self.gaze_target
                || self.shadow != self.shadow_goal()
                || !self.tile_fx.is_empty()
                || self.drop_glow.is_some()
                || self.hud_fill != self.hud_fill_target
                || self.hud_roll.is_some()
                || self.wake.is_some()
                || self.lyric_mix != self.lyric_goal()
                || self.lyric_scroll.is_some()
                || self.hover_mix < 1.0)
    }

    /// Call before changing an animation target, so the first frame does not jump.
    pub(super) fn start_motion(&mut self) {
        if !self.animating() {
            self.last_frame = self.now();
        }
    }

    pub(super) fn tick(&mut self) {
        FRAMES.fetch_add(1, Ordering::Relaxed);
        // A settled pill that is only blinking/glancing just needs a redraw around its eyes.
        let eyes_only = self.view == View::Pill
            && self.alpha[0] == 1.0
            && !self.width.is_moving()
            && !self.height.is_moving()
            && self.alpha == self.alpha_goals()
            && self.page_anim.is_none()
            && !self.playing()
            && !self.focus.running()
            && !self.timer.running();
        let now = self.now();
        let dt = (now - self.last_frame) as f32;
        self.last_frame = now;
        self.width.step(dt);
        self.height.step(dt);
        let goals = self.alpha_goals();
        for (alpha, goal) in self.alpha.iter_mut().zip(goals) {
            let tau = if goal > *alpha { 0.08 } else { 0.05 };
            ease(alpha, goal, dt, tau, 0.004);
        }
        if self.blink_since.is_some_and(|start| now - start >= BLINK_SECS) {
            self.blink_since = None;
        }
        if self.page_anim.is_some_and(|(_, start, _)| now - start >= PAGE_ANIM_SECS) {
            self.page_anim = None;
        }
        // Tiles that finished fading out are only removed from the shelf now.
        let mut removed = false;
        let shelf = &mut self.shelf;
        self.tile_fx.retain(|&id, &mut (start, removing)| {
            let done = now - start >= if removing { TILE_OUT_SECS } else { TILE_IN_SECS };
            if done && removing {
                shelf.remove(id);
                removed = true;
            }
            !done
        });
        if removed {
            self.shelf_changed();
        }
        if self.drop_glow.is_some_and(|start| now - start >= GLOW_SECS) {
            self.drop_glow = None;
        }
        if self.hud_roll.as_ref().is_some_and(|(_, start, _)| now - start >= ROLL_SECS) {
            self.hud_roll = None;
        }
        // After waking up, the mascot blinks once right away.
        if self.wake.is_some_and(|start| now - start >= WAKE_CLOSED_SECS + WAKE_OPEN_SECS) {
            self.wake = None;
            self.set_timer(TIMER_BLINK, 350);
        }
        if self.lyric_scroll.is_some_and(|start| now - start >= LYRIC_SCROLL_SECS) {
            self.lyric_scroll = None;
        }
        let goal = self.lyric_goal();
        ease(&mut self.lyric_mix, goal, dt, 0.09, 0.004);
        ease(&mut self.hover_mix, 1.0, dt, HOVER_TAU, 0.01);
        ease(&mut self.hud_fill, self.hud_fill_target, dt, 0.07, 0.002);
        let shadow = self.shadow_goal();
        ease(&mut self.shadow, shadow, dt, 0.1, 0.01);
        if self.gaze != self.gaze_target {
            let k = 1.0 - (-dt / 0.09).exp();
            let (tx, ty) = self.gaze_target;
            self.gaze = (self.gaze.0 + (tx - self.gaze.0) * k, self.gaze.1 + (ty - self.gaze.1) * k);
            if (tx - self.gaze.0).abs() < 0.01 && (ty - self.gaze.1).abs() < 0.01 {
                self.gaze = self.gaze_target;
            }
        }
        if eyes_only && self.shown {
            let result = self
                .renderer
                .draw_eyes(self.width.value, self.height.value, self.eyes())
                .and_then(|dirty| self.renderer.present(self.hwnd, self.window_pos(), Some(&dirty)));
            if let Err(e) = result {
                error_log!("failed to draw eyes: {e}");
            }
        } else {
            self.redraw();
        }
    }

    pub(super) fn redraw(&mut self) {
        if !self.shown {
            return;
        }
        self.clock_text.refresh();
        let battery = self.battery.or_else(power::battery_percent).map(|b| (b, self.plugged == Some(true)));
        let media = self.media.as_ref().map(|m| media_view(m, self.seek_preview));
        let lyrics = self.lyrics_view();
        let focus = self.focus_view();
        let water = self.water_view();
        let prayer = self.prayer_view();
        let timer = self.timer_view();
        let controls = self.controls_view();
        let weather = self.weather_text();
        let calendar = self.calendar_view();
        let download = self.download_text();
        let lyric = self.lyric_line();
        let now = self.now();
        let roll = self.hud_roll.as_ref().map(|(from, start, up)| Roll {
            from: from.clone(),
            t: ((now - start) / ROLL_SECS).min(1.0) as f32,
            up: *up,
        });
        let shelf_fx = ShelfFx {
            targeted: self.shelf.drop_hover && self.drop_over == Some(DropZone::Shelf),
            share_targeted: self.shelf.drop_hover && self.drop_over == Some(DropZone::Share),
            dash: (now * 5.0 % 5.0) as f32,
            glow: self.drop_glow.map(|start| ((now - start) / GLOW_SECS).min(1.0) as f32),
            tiles: self.tile_fx.iter().map(|(id, (start, removing))| (*id, tile_look(now - start, *removing))).collect(),
        };
        let scene = Scene {
            width: self.width.value,
            height: self.height.value,
            alpha: self.alpha,
            eyes: self.eyes(),
            shadow: self.shadow,
            time: &self.clock_text.time,
            date: &self.clock_text.date,
            greeting: self.clock_text.greeting,
            battery,
            media: media.as_ref(),
            lyric: lyric.as_ref().map(|(_, text)| text.as_str()),
            lyrics: lyrics.as_ref(),
            lyric_mix: self.lyric_mix,
            lyrics_button: self.settings.lyrics.then_some(self.settings.lyric_panel),
            hud: self.hud.as_ref(),
            // Follows the HUD kind, not the current shape: while the HUD fades out, the shape is
            // already back to a pill, but the content must stay compact.
            hud_compact: self.compact_hud(),
            hud_roll: roll.as_ref(),
            hud_fill: self.hud_fill,
            page: self.page,
            pages: self.settings.pages,
            page_from: self
                .page_anim
                .map(|(from, start, dir)| (from, ((now - start) / PAGE_ANIM_SECS).clamp(0.0, 1.0) as f32, dir)),
            shelf: &self.shelf,
            shelf_fx: &shelf_fx,
            tasks: &self.tasks,
            focus: &focus,
            water: &water,
            prayer: &prayer,
            privacy: &self.privacy,
            system: self.system.as_ref(),
            uptime: &self.clock_text.uptime,
            hovered: self.hovered,
            seconds: now,
            hover: self.hover_mix,
            locked: self.locked,
            timer: &timer,
            controls: &controls,
            weather: weather.as_deref(),
            city_draft: self.city_draft.as_deref(),
            calendar: calendar.as_ref(),
            agenda_draft: self.agenda.draft.as_deref(),
            agents: &self.agent_rows,
            agent_hooks: self.agent_hooks,
            mail: &self.mail_view,
            plugins: &self.plugins_view,
            agent_scroll: self.agent_scroll,
            download: download.as_deref(),
            welcome: self.welcome && self.expanded && !self.locked,
            settings: self.settings,
        };
        let result = self.renderer.draw(&scene).and_then(|_| self.renderer.present(self.hwnd, self.window_pos(), None));
        if let Err(e) = result {
            error_log!("failed to draw: {e}");
        }
        self.last_full = now;
        self.lyric_shown = lyric.and_then(|(index, _)| index);
        self.lyric_drawn = self.lyric_index();
        self.second_drawn = self.media.as_ref().map(|m| m.position_now() as u64);
    }

    /// Top center of the primary monitor.
    pub(super) fn window_pos(&self) -> POINT {
        let size = self.renderer.size();
        let r = &self.monitor_rect;
        POINT { x: r.left + (r.right - r.left - size.cx) / 2, y: r.top }
    }
}

/// Exponential approach of `value` toward `goal` with time constant `tau` (seconds), snapping
/// to the goal once within `epsilon`.
fn ease(value: &mut f32, goal: f32, dt: f32, tau: f32, epsilon: f32) {
    *value += (goal - *value) * (1.0 - (-dt / tau).exp());
    if (goal - *value).abs() < epsilon {
        *value = goal;
    }
}
