//! Mouse, keyboard, wheel/touchpad, text editing, hotkey, and the page context menu.

use super::*;

impl App {
    pub(super) fn on_mouse_move(&mut self, lparam: LPARAM) {
        self.on_hover();
        if let Some(slider) = self.slider_drag {
            self.apply_slider(slider, lparam);
            return;
        }
        // A pressed shelf tile moved far enough starts being dragged out.
        if let Some((id, start)) = self.press {
            let (x, y) = point(lparam);
            let (dx, dy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
            if (x - start.x).abs() > dx || (y - start.y).abs() > dy {
                self.press = None;
                let _ = unsafe { ReleaseCapture() };
                self.drag_item = Some(id);
                let _ = unsafe { PostMessageW(Some(self.hwnd), WM_START_DRAG, WPARAM(0), LPARAM(0)) };
                return;
            }
        }
        let hit = if self.expanded { self.hit_at(lparam) } else { None };
        self.set_hovered(hit);
    }

    pub(super) fn on_hover(&mut self) {
        if !self.tracking_leave {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            self.tracking_leave = unsafe { TrackMouseEvent(&mut track) }.is_ok();
        }
        if !self.hovering {
            self.hovering = true;
            self.kill_timer(TIMER_CLOSE);
            if !self.expanded {
                self.set_timer(TIMER_OPEN, OPEN_DELAY_MS);
                self.peek = true;
                self.refresh();
            }
        }
    }

    pub(super) fn on_leave(&mut self) {
        self.tracking_leave = false;
        self.hovering = false;
        if std::mem::take(&mut self.peek) {
            self.refresh();
        }
        self.kill_timer(TIMER_OPEN);
        // While typing, the panel stays open even if the cursor leaves.
        if self.expanded && !self.editing() {
            self.set_timer(TIMER_CLOSE, CLOSE_DELAY_MS);
        }
    }

    pub(super) fn on_press(&mut self, lparam: LPARAM) {
        if !self.expanded {
            return;
        }
        match self.hit_at(lparam) {
            Some(Hit::ShelfItem(id)) => {
                let (x, y) = point(lparam);
                self.press = Some((id, POINT { x, y }));
                unsafe { SetCapture(self.hwnd) };
            }
            // The slider jumps to the cursor, then can be dragged.
            Some(slider @ (Hit::VolumeSlider | Hit::BrightnessSlider | Hit::MediaSeek)) => {
                self.slider_drag = Some(slider);
                unsafe { SetCapture(self.hwnd) };
                self.apply_slider(slider, lparam);
            }
            _ => {}
        }
    }

    /// Set volume/brightness from the cursor position on the slider.
    pub(super) fn apply_slider(&mut self, slider: Hit, lparam: LPARAM) {
        let x = point(lparam).0 as f32 / self.renderer.scale();
        let Some(value) = self.renderer.slider_value(slider, x) else { return };
        self.set_level(slider, value);
    }

    /// Volume (0..1) or brightness; the display changes immediately, the device follows.
    pub(super) fn set_level(&mut self, slider: Hit, value: f32) {
        let value = value.clamp(0.0, 1.0);
        match slider {
            Hit::VolumeSlider => {
                self.volume = Some((value, false));
                self.services.send(Task::SetVolume(value));
            }
            Hit::BrightnessSlider if self.brightness.is_some() => {
                let percent = (value * 100.0).round() as u8;
                self.brightness = Some(percent as u32);
                self.services.send(Task::SetBrightness(percent));
            }
            // The track bar only moves on screen while dragged; the seek happens on release.
            Hit::MediaSeek => self.seek_preview = Some(value),
            _ => return,
        }
        if !self.animating() {
            self.redraw();
        }
    }

    pub(super) fn on_click(&mut self, lparam: LPARAM) {
        if let Some(slider) = self.slider_drag.take() {
            let _ = unsafe { ReleaseCapture() };
            if slider == Hit::MediaSeek {
                self.finish_seek();
            }
            return;
        }
        // A single click on a shelf tile does nothing; double-click opens it.
        if self.press.take().is_some() {
            let _ = unsafe { ReleaseCapture() };
            return;
        }
        self.kill_timer(TIMER_OPEN);
        if !self.expanded {
            // A message HUD opens its app, a download HUD reveals the file in its folder
            // (not from the lock screen).
            if self.hud_visible && !self.locked {
                let opened = match &self.hud {
                    Some(Hud::Message { aumid, app, browser, .. }) => {
                        messages::open(self.hwnd, aumid, app, *browser);
                        true
                    }
                    // An agent notice switches to the agent's window; without one, it opens
                    // the Agents page like other notices.
                    Some(Hud::Agent { pid, project, .. }) => self.focus_agent(*pid, project),
                    // A new-mail notice opens Gmail itself: that is where the message is read.
                    Some(Hud::Mail { .. }) => {
                        dnd::open_url("https://mail.google.com");
                        true
                    }
                    Some(Hud::Download { path, .. }) => {
                        dnd::reveal(self.hwnd, path);
                        true
                    }
                    _ => false,
                };
                if opened {
                    self.hud_visible = false;
                    self.refresh();
                    return;
                }
            }
            // Clicking any other HUD opens its related page.
            if self.hud_visible
                && let Some(hud) = &self.hud
            {
                let page = match hud {
                    Hud::Focus { .. } => Page::Focus,
                    Hud::Water { .. } => Page::Water,
                    Hud::Privacy { .. } => Page::System,
                    Hud::Screenshot { .. } => Page::Shelf,
                    Hud::Timer { .. } => Page::Timer,
                    Hud::Agent { .. } => Page::Agents,
                    Hud::Mail { .. } => Page::Mail,
                    Hud::Prayer { .. } => Page::Prayer,
                    _ => Page::Home,
                };
                // Opening the notice is the answer to it: the timer alarm stops here.
                if matches!(hud, Hud::Timer { .. }) {
                    sound::stop();
                }
                self.page = if self.settings.pages.shown(page) { page } else { Page::Home };
                self.page_anim = None;
            }
            if self.locked {
                self.page = Page::Home;
            }
            self.set_expanded(true);
            return;
        }
        let hit = self.hit_at(lparam);
        let typing_here = matches!(hit, Some(Hit::TaskInput | Hit::CityInput | Hit::AgendaInput)) || match hit {
            Some(Hit::GoogleClient(0)) => self.edit_field == Some(Field::GoogleId),
            Some(Hit::GoogleClient(_)) => self.edit_field == Some(Field::GoogleSecret),
            _ => false,
        };
        if self.editing() && !typing_here {
            self.end_edit(true, true);
        }
        if let Some(hit) = hit {
            self.on_hit(hit);
        }
        self.refresh();
    }

    pub(super) fn on_hit(&mut self, hit: Hit) {
        let now = Instant::now();
        match hit {
            // The shared timer tab reopens whichever of Focus and Timer was used last.
            Hit::PageDot(page) => {
                let page = if page == Page::Focus && self.settings.pages.tab(Page::Timer) == Page::Focus { self.timer_tab } else { page };
                self.go_page(page, self.page_dir(page));
            }
            Hit::TimerMode(page) => self.go_page(page, if page == Page::Timer { 1.0 } else { -1.0 }),
            Hit::Media(button) => self.media_command(button),
            Hit::LyricsToggle => {
                self.start_motion();
                self.settings.lyric_panel = !self.settings.lyric_panel;
                self.settings.save();
                self.request_lyrics();
                // The panel changes between narrow and wide (and the lyrics column slides), so the
                // shape has to be pointed at the new target at once. Without this the card stayed
                // collapsed until some later event happened to redraw.
                self.refresh();
            }
            // Tiles shrink and fade first; they leave the shelf once the animation ends.
            Hit::ShelfClear => {
                let ids: Vec<u64> = self.shelf.items.iter().map(|item| item.id).collect();
                ids.into_iter().for_each(|id| self.fade_tile(id));
            }
            Hit::ShelfRemove(id) => self.fade_tile(id),
            Hit::FocusToggle => {
                self.focus.toggle(now);
                self.schedule_focus();
            }
            Hit::FocusReset => {
                self.focus.reset();
                self.schedule_focus();
            }
            Hit::FocusPhase(phase) => {
                if self.focus.phase != phase {
                    self.focus.select(phase);
                    self.schedule_focus();
                }
            }
            Hit::WaterDrink => {
                let before = self.water.on(&self.day);
                self.water.drink(&self.day);
                self.water.save();
                self.water_due = now + WATER_EVERY;
                // Today's goal reached: the mascot is happy too.
                if before < GOAL_ML && self.water.on(&self.day) >= GOAL_ML {
                    self.set_mood(Mood::Happy, 8_000);
                }
            }
            Hit::WaterUndo => {
                self.water.undo(&self.day);
                self.water.save();
            }
            Hit::TaskToggle(index) => {
                self.tasks.toggle(index);
                self.tasks.save();
            }
            Hit::TaskRemove(index) => {
                self.tasks.remove(index);
                self.tasks.save();
            }
            Hit::TaskInput => self.begin_edit(Field::Task),
            Hit::TasksClearDone => {
                self.tasks.clear_done();
                self.tasks.save();
            }
            Hit::VolumeMute => self.services.send(Task::ToggleMute),
            Hit::MicToggle => self.services.send(Task::ToggleMic),
            Hit::QuietToggle => {
                if self.quiet() {
                    self.quiet_until = None;
                    self.kill_timer(TIMER_QUIET);
                } else {
                    self.quiet_until = Some(now + QUIET);
                    self.set_timer(TIMER_QUIET, QUIET.as_millis() as u32);
                }
            }
            Hit::CityInput => self.begin_edit(Field::City),
            Hit::WelcomeDone => {
                self.welcome = false;
                self.settings.welcomed = true;
                self.settings.save();
                self.set_expanded(false);
            }
            Hit::CalendarDay(offset) => {
                self.cal_offset = offset;
                self.agenda.scroll = 0;
            }
            Hit::AgendaToggle(id) => {
                self.agenda.toggle(id);
                self.agenda.save();
            }
            Hit::AgendaRemove(id) => {
                self.agenda.remove(id);
                self.agenda.save();
                let count = self.agenda.on(self.selected_day()).len();
                self.agenda.scroll_by(0, count);
            }
            Hit::AgendaInput => self.begin_edit(Field::Agenda),
            Hit::TimerPreset(minutes) => {
                self.timer.start(minutes, now);
                self.schedule_alarm();
            }
            Hit::TimerToggle => {
                self.timer.toggle_pause(now);
                self.schedule_alarm();
            }
            Hit::TimerAdd => {
                self.timer.add_minute(now);
                self.schedule_alarm();
            }
            Hit::TimerCancel => {
                self.timer.cancel();
                self.schedule_alarm();
            }
            Hit::Command(command) => self.on_command(command),
            Hit::PluginsReload => self.reload_plugins(),
            Hit::PluginsFolder => plugins::open_folder(self.hwnd),
            Hit::PluginRow(index) => {
                if let Some(row) = self.plugins.rows().into_iter().chain(self.plugins.problems()).nth(index) {
                    dnd::open_url(&row.url);
                }
            }
            // While a sign-in is waiting, the same button cancels it.
            Hit::GoogleConnect if self.connecting => self.cancel_google(),
            Hit::GoogleConnect => self.connect_google(),
            Hit::GoogleClient(0) => self.begin_edit(Field::GoogleId),
            Hit::GoogleClient(_) => self.begin_edit(Field::GoogleSecret),
            Hit::GoogleDisconnect => self.disconnect_google(),
            Hit::GoogleOpen | Hit::MailRow(_) => {
                if google::connected() {
                    dnd::open_url("https://mail.google.com");
                } else {
                    self.connect_google();
                }
            }
            Hit::AgentRow(index) => {
                if let Some(session) = self.agent_rows.get(index) {
                    let (pid, project) = (session.pid, session.project.clone());
                    if self.focus_agent(pid, &project) {
                        self.set_expanded(false);
                    }
                }
            }
            Hit::AgentsRefresh => {
                if let Some(listener) = &self.agent_listener {
                    listener.refresh();
                }
            }
            Hit::AgentHooks => {
                if agents::install_hooks().is_err() {
                    error_log!("failed to install coding agent hooks");
                }
                self.agent_hooks = agents::installed_hooks();
                if let Some(listener) = &self.agent_listener {
                    listener.refresh();
                }
            }
            Hit::AgentHook(provider) => {
                if agents::install_provider(provider).is_err() {
                    error_log!("failed to install coding agent hook");
                }
                self.agent_hooks = agents::installed_hooks();
                if let Some(listener) = &self.agent_listener {
                    listener.refresh();
                }
            }
            Hit::AgentFolder(index) => {
                if let Some(session) = self.agent_rows.get(index)
                    && !session.folder.is_empty()
                {
                    dnd::reveal(self.hwnd, std::path::Path::new(&session.folder));
                }
            }
            Hit::ShelfItem(_)
            | Hit::ShelfStrip
            | Hit::TaskList
            | Hit::VolumeSlider
            | Hit::BrightnessSlider
            | Hit::MediaSeek
            | Hit::CalendarStrip
            | Hit::AgendaList
            | Hit::AgendaRow(_)
            | Hit::AgentList
            | Hit::PluginList
            | Hit::DropShare => {}
        }
    }

    pub(super) fn on_double_click(&mut self, lparam: LPARAM) {
        if self.expanded
            && let Some(Hit::ShelfItem(id)) = self.hit_at(lparam)
            && let Some(item) = self.shelf.get(id)
        {
            dnd::open(self.hwnd, &item.path);
        }
    }

    pub(super) fn on_right_click(&mut self, lparam: LPARAM) {
        if self.expanded
            && let Some(Hit::ShelfItem(id)) = self.hit_at(lparam)
            && let Some(item) = self.shelf.get(id)
        {
            dnd::reveal(self.hwnd, &item.path);
        }
    }

    /// Mouse wheel: scroll the shelf/task list when full, otherwise change page.
    pub(super) fn on_wheel(&mut self, wparam: WPARAM, lparam: LPARAM, horizontal: bool) {
        if !self.expanded && horizontal && !self.locked {
            self.on_swipe(((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32);
            return;
        }
        if !self.expanded || self.locked {
            return;
        }
        self.wheel += ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
        if self.wheel.abs() < 120 {
            return;
        }
        let up = self.wheel > 0;
        self.wheel = 0;
        // Wheel down or swipe right = next.
        let step = if horizontal == up { 1 } else { -1 };
        // Wheel position is in screen coordinates, not window coordinates.
        let (pos, scale) = (self.window_pos(), self.renderer.scale());
        let (x, y) = point(lparam);
        let hit = self.renderer.hit_at((x - pos.x) as f32 / scale, (y - pos.y) as f32 / scale);
        match hit {
            Some(Hit::ShelfStrip | Hit::ShelfItem(_) | Hit::ShelfRemove(_))
                if !horizontal && self.shelf.items.len() > shelf::VISIBLE =>
            {
                self.shelf.scroll_by(step)
            }
            Some(Hit::TaskList | Hit::TaskToggle(_) | Hit::TaskRemove(_))
                if !horizontal && self.tasks.items.len() > tasks::VISIBLE =>
            {
                self.tasks.scroll_by(step)
            }
            // Wheel over the date strip: shift the day (down/right = next day).
            Some(Hit::CalendarStrip | Hit::CalendarDay(_)) => {
                self.cal_offset += step;
                self.agenda.scroll = 0;
            }
            Some(Hit::AgendaList | Hit::AgendaRow(_) | Hit::AgendaToggle(_) | Hit::AgendaRemove(_))
                if !horizontal && self.agenda.on(self.selected_day()).len() > agenda::VISIBLE =>
            {
                let count = self.agenda.on(self.selected_day()).len();
                self.agenda.scroll_by(step, count);
            }
            Some(Hit::PluginList) if !horizontal => {}
            Some(Hit::AgentList | Hit::AgentRow(_)) if !horizontal && self.agent_rows.len() > agents::VISIBLE => {
                let last = self.agent_rows.len() - agents::VISIBLE;
                self.agent_scroll = (self.agent_scroll as i32 + step).clamp(0, last as i32) as usize;
            }
            // Wheel over a slider: up/down 2%.
            Some(slider @ (Hit::VolumeSlider | Hit::BrightnessSlider)) if !horizontal => {
                let current = match slider {
                    Hit::VolumeSlider => self.volume.map(|(level, _)| level),
                    _ => self.brightness.map(|b| b as f32 / 100.0),
                };
                if let Some(current) = current {
                    self.set_level(slider, current - step as f32 * 0.02);
                }
            }
            _ => self.go_page(self.settings.pages.step(self.page, step), step as f32),
        }
        self.hovered = None;
        self.refresh();
    }

    /// Two-finger swipe right/left on the pill while music plays: next/previous track.
    pub(super) fn on_swipe(&mut self, delta: i32) {
        if !self.playing() {
            return;
        }
        let now = self.now();
        let (mut total, last, skipped) = self.swipe;
        // A swipe after a long pause counts as a new gesture.
        if now - last > 0.4 {
            total = 0;
        }
        total += delta;
        self.swipe = (total, now, skipped);
        if total.abs() >= SWIPE_DISTANCE && now - skipped >= SWIPE_COOLDOWN {
            self.swipe = (0, now, now);
            self.media_command(if total > 0 { Button::Next } else { Button::Previous });
        }
    }

    pub(super) fn go_page(&mut self, page: Page, dir: f32) {
        if page == self.page || self.locked {
            return;
        }
        self.end_edit(true, true);
        self.start_motion();
        self.page_anim = Some((self.page, self.now(), dir));
        self.page = page;
        if matches!(page, Page::Focus | Page::Timer) {
            self.timer_tab = page;
        }
        self.hovered = None;
        if page == Page::Shelf && self.shelf.prune_missing() {
            self.shelf_changed();
        }
    }

    /// Typing a task, agenda item, city name, Google client, or feed address.
    pub(super) fn editing(&self) -> bool {
        self.tasks.draft.is_some() || self.city_draft.is_some() || self.agenda.draft.is_some() || self.edit_field.is_some()
    }

    /// Start typing into `field` (task, weather city, or agenda item). Crest may
    /// temporarily take keyboard focus.
    pub(super) fn begin_edit(&mut self, field: Field) {
        if self.editing() {
            return;
        }
        match field {
            Field::Task => self.tasks.draft = Some(String::new()),
            Field::City => {
                self.city_draft = Some(String::new());
                self.city_status = None;
            }
            Field::Agenda => self.agenda.draft = Some(String::new()),
            // The client and feed boxes start with what is already there, so a typo can be fixed.
            Field::GoogleId => self.edit_text = self.google_id.clone(),
            Field::GoogleSecret => self.edit_text = self.google_secret.clone(),
        }
        self.edit_field = Some(field);
        self.pending_surrogate = None;
        self.edit_return = Some(unsafe { GetForegroundWindow() });
        // The Mail page shows which box is being typed into.
        if matches!(field, Field::GoogleId | Field::GoogleSecret) {
            self.rebuild_views();
        }
        self.set_activatable(true);
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
    }

    /// Stop typing. `commit` keeps the text; `restore_focus` returns focus to the previous
    /// app (not needed if the user already switched apps).
    pub(super) fn end_edit(&mut self, commit: bool, restore_focus: bool) {
        let (task, city, event) = (self.tasks.draft.take(), self.city_draft.take(), self.agenda.draft.take());
        let field = self.edit_field.take();
        if task.is_none() && city.is_none() && event.is_none() && field.is_none() {
            return;
        }
        let text = std::mem::take(&mut self.edit_text);
        match field {
            Some(Field::GoogleId) if commit => self.google_id = text.trim().to_string(),
            Some(Field::GoogleSecret) if commit => self.google_secret = text.trim().to_string(),
            // The feed box keeps its text until the + button is pressed.
            Some(_) => {}
            None => {}
        }
        if commit
            && let Some(event) = event
            && self.agenda.add(self.selected_day(), &event)
        {
            self.agenda.save();
        }
        if commit
            && let Some(task) = task
            && self.tasks.add(&task)
        {
            self.tasks.reveal(self.tasks.items.len() - 1);
            self.tasks.save();
        }
        if commit
            && let Some(name) = city.map(|c| c.trim().to_string())
            && !name.is_empty()
        {
            self.city_status = Some("Searching for city…".to_string());
            weather::find_city(name, self.services.notifier());
        }
        self.set_activatable(false);
        if let Some(previous) = self.edit_return.take()
            && restore_focus
            && previous != self.hwnd
            && unsafe { IsWindow(Some(previous)) }.as_bool()
        {
            let _ = unsafe { SetForegroundWindow(previous) };
        }
        if self.expanded && !self.hovering {
            self.set_timer(TIMER_CLOSE, CLOSE_DELAY_MS);
        }
        // The committed text returns to the Mail page's boxes.
        if matches!(field, Some(Field::GoogleId | Field::GoogleSecret)) {
            self.rebuild_views();
        }
    }

    pub(super) fn set_activatable(&self, on: bool) {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, if on { style & !flag } else { style | flag });
        }
    }

    pub(super) fn on_char(&mut self, unit: u16) {
        if !self.editing() {
            return;
        }
        match unit {
            0x08 => {
                if let Some(draft) = self.tasks.draft.as_mut().or(self.city_draft.as_mut()).or(self.agenda.draft.as_mut()) {
                    draft.pop();
                } else if self.edit_field.is_some() {
                    self.edit_text.pop();
                }
            }
            // Enter in the city box: search for the city.
            0x0D if self.city_draft.is_some() => self.end_edit(true, true),
            // Enter in a Google client box closes it; the Connect button then uses the text.
            0x0D if matches!(self.edit_field, Some(Field::GoogleId | Field::GoogleSecret)) => {
                self.end_edit(true, true)
            }
            // Enter in the agenda box: save and get ready for the next item; an empty box closes.
            0x0D if self.agenda.draft.is_some() => {
                let text = self.agenda.draft.replace(String::new()).unwrap_or_default();
                if self.agenda.add(self.selected_day(), &text) {
                    self.agenda.save();
                } else {
                    self.end_edit(false, true);
                }
            }
            0x0D => {
                // Enter: save and get ready for the next task; Enter in an empty box closes.
                let text = self.tasks.draft.replace(String::new()).unwrap_or_default();
                if self.tasks.add(&text) {
                    self.tasks.reveal(self.tasks.items.len() - 1);
                    self.tasks.save();
                } else {
                    self.end_edit(false, true);
                }
            }
            0x1B => self.end_edit(false, true),
            0x16 => {
                if let Some(text) = clipboard_text() {
                    self.type_text(text.lines().next().unwrap_or(""));
                }
            }
            0xD800..=0xDBFF => self.pending_surrogate = Some(unit),
            _ if unit < 0x20 || unit == 0x7F => {}
            _ => {
                let units = match self.pending_surrogate.take() {
                    Some(high) => vec![high, unit],
                    None => vec![unit],
                };
                self.type_text(&String::from_utf16_lossy(&units));
            }
        }
        // Typing and Backspace change the text of a Google box on the Mail page. Enter and Escape
        // closed the box already, and `end_edit` rebuilt the view for that.
        if matches!(self.edit_field, Some(Field::GoogleId | Field::GoogleSecret)) {
            self.rebuild_views();
        }
        self.refresh();
    }

    pub(super) fn type_text(&mut self, text: &str) {
        let (draft, max) = match (&mut self.tasks.draft, &mut self.city_draft, &mut self.agenda.draft) {
            (Some(draft), _, _) => (draft, tasks::MAX_LEN),
            (None, Some(draft), _) => (draft, CITY_MAX),
            (None, None, Some(draft)) => (draft, agenda::MAX_LEN),
            (None, None, None) if self.edit_field.is_some() => (&mut self.edit_text, GOOGLE_ID_MAX),
            (None, None, None) => return,
        };
        let room = max.saturating_sub(draft.chars().count());
        draft.extend(text.chars().filter(|c| !c.is_control()).take(room));
    }

    pub(super) fn hit_at(&self, lparam: LPARAM) -> Option<Hit> {
        let scale = self.renderer.scale();
        let (x, y) = point(lparam);
        self.renderer.hit_at(x as f32 / scale, y as f32 / scale)
    }

    pub(super) fn set_hovered(&mut self, hit: Option<Hit>) {
        if self.hovered != hit {
            self.hovered = hit;
            // Only an open panel fades its highlights in; the frames stop once it settles.
            if hit.is_some() && self.view.is_panel() {
                self.start_motion();
                self.hover_mix = 0.0;
            }
            if !self.animating() {
                self.redraw();
            }
        }
    }

    /// Register or unregister the Win+Alt+N hotkey.
    pub(super) fn set_hotkey(&mut self, on: bool) {
        if on == self.hotkey {
            return;
        }
        if on {
            let modifiers = MOD_WIN | MOD_ALT | MOD_NOREPEAT;
            match unsafe { RegisterHotKey(Some(self.hwnd), HOTKEY_ID, modifiers, u32::from(b'N')) } {
                Ok(()) => self.hotkey = true,
                Err(e) => debug_log!("Win+Alt+N hotkey unavailable: {e}"),
            }
        } else {
            let _ = unsafe { UnregisterHotKey(Some(self.hwnd), HOTKEY_ID) };
            self.hotkey = false;
        }
    }

    /// Win+Alt+N: open the panel, or close it if open.
    pub(super) fn on_hotkey(&mut self) {
        if !self.shown {
            return;
        }
        if self.expanded {
            self.set_expanded(false);
            return;
        }
        self.kill_timer(TIMER_OPEN);
        self.set_expanded(true);
        if !self.hovering {
            self.set_timer(TIMER_CLOSE, HOTKEY_CLOSE_MS);
        }
    }

    /// Slide direction toward `page`: forward if it lies right of the current page.
    /// Bring an agent's terminal or editor to the front: through its process when known,
    /// otherwise a window whose title names the project folder.
    pub(super) fn focus_agent(&self, pid: u32, project: &str) -> bool {
        agents::focus(pid) || messages::focus_window_titled(project)
    }

    pub(super) fn page_dir(&self, page: Page) -> f32 {
        // Settings sits after every page in the tab bar.
        let pages = &self.settings.pages;
        let index = |page: Page| if page == Page::Settings { usize::MAX } else { pages.index(page) };
        if index(page) > index(self.page) { 1.0 } else { -1.0 }
    }

    /// Right-click on the panel (except on a shelf tile): prepare the page settings menu.
    pub(super) fn page_menu(&mut self, lparam: LPARAM) -> Option<PageMenu> {
        let on_tile = matches!(self.hit_at(lparam), Some(Hit::ShelfItem(_) | Hit::ShelfRemove(_)));
        if !self.expanded || self.locked || on_tile || self.page == Page::Settings {
            return None;
        }
        self.end_edit(true, false);
        self.menu_open = true;
        self.kill_timer(TIMER_CLOSE);
        let pages = self.settings.pages;
        Some(PageMenu {
            page: self.page,
            // Focus and Timer move together, as the tab they share.
            can_left: pages.can_move(pages.tab(self.page), -1),
            can_right: pages.can_move(pages.tab(self.page), 1),
            hidden: pages.order().into_iter().filter(|p| !pages.shown(*p)).collect(),
        })
    }

    pub(super) fn after_page_menu(&mut self, page: Page, command: Option<PageCommand>) {
        self.menu_open = false;
        match command {
            Some(PageCommand::Move(delta)) => {
                let tab = self.settings.pages.tab(page);
                self.settings.pages.move_by(tab, delta);
            }
            Some(PageCommand::Hide) => {
                let next = self.settings.pages.step(page, 1);
                self.settings.pages.set_shown(page, false);
                self.page = next;
                self.page_anim = None;
            }
            Some(PageCommand::Show(shown)) => {
                self.settings.pages.set_shown(shown, true);
                self.go_page(shown, self.page_dir(shown));
            }
            None => {}
        }
        if command.is_some() {
            self.settings.save();
            self.update_watchers();
        }
        self.hovered = None;
        // While the menu was open, the cursor counted as having left the panel.
        if !self.cursor_over_window() {
            self.set_timer(TIMER_CLOSE, CLOSE_DELAY_MS);
        }
        self.refresh();
    }

    pub(super) fn cursor_over_window(&self) -> bool {
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return false;
        }
        let (pos, size) = (self.window_pos(), self.renderer.size());
        (pos.x..pos.x + size.cx).contains(&cursor.x) && (pos.y..pos.y + size.cy).contains(&cursor.y)
    }

    /// Listen to raw keyboard input, only to track Caps Lock / Num Lock. Other keys are
    /// ignored immediately and never stored.
    pub(super) fn set_raw_keyboard(&mut self, on: bool) {
        if on == self.raw_keyboard {
            return;
        }
        let device = RAWINPUTDEVICE {
            usUsagePage: 0x01, // generic desktop controls
            usUsage: 0x06,     // keyboard
            dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
            hwndTarget: if on { self.hwnd } else { HWND::default() },
        };
        match unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) } {
            Ok(()) => {
                self.raw_keyboard = on;
                self.lock_keys = lock_key_states();
            }
            Err(e) => error_log!("failed to {} raw keyboard input: {e}", if on { "register" } else { "unregister" }),
        }
    }

    pub(super) fn on_raw_input(&mut self, lparam: LPARAM) {
        let mut input = RAWINPUT::default();
        let mut size = size_of::<RAWINPUT>() as u32;
        let header = size_of::<RAWINPUTHEADER>() as u32;
        let read = unsafe {
            GetRawInputData(HRAWINPUT(lparam.0 as _), RID_INPUT, Some((&mut input as *mut RAWINPUT).cast()), &mut size, header)
        };
        if read == u32::MAX || input.header.dwType != RIM_TYPEKEYBOARD.0 {
            return;
        }
        let key = unsafe { input.data.keyboard };
        let released = key.Flags as u32 & RI_KEY_BREAK != 0;
        if released && (key.VKey == VK_CAPITAL.0 || key.VKey == VK_NUMLOCK.0) {
            // The light state only changes shortly after the key is released.
            self.set_timer(TIMER_LOCK_KEYS, 40);
        }
    }

    pub(super) fn on_lock_keys(&mut self) {
        self.kill_timer(TIMER_LOCK_KEYS);
        let (caps, num) = lock_key_states();
        let (old_caps, old_num) = std::mem::replace(&mut self.lock_keys, (caps, num));
        if caps != old_caps {
            self.show_hud(Hud::LockKey { caps: true, on: caps }, LOCK_KEY_HUD_MS);
        } else if num != old_num {
            self.show_hud(Hud::LockKey { caps: false, on: num }, LOCK_KEY_HUD_MS);
        }
        self.refresh();
    }
}
