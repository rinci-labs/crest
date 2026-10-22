//! Reactions to background services: power, network, privacy, messages, files, sessions.

use super::*;

impl App {
    /// Enable incoming messages: explain first, then run the Administrator step on another thread
    /// so Crest stays responsive while the Windows permission prompt is open.
    pub(super) fn activate_messages(&mut self) {
        let text = "Enable incoming messages (WhatsApp, Telegram, etc.)?\n\n\
                    Windows only lets packaged apps read notifications. To do that, Crest will:\n\
                    • create a small package containing only Crest's name and logo (no program),\n\
                    • sign it with a certificate created on this computer only,\n\
                    • trust that certificate; its private key lives only in memory and is gone\n   \
                    right away, so it can never sign anything else.\n\n\
                    Requires Administrator permission once, then Crest restarts.";
        let style = windows::Win32::UI::WindowsAndMessaging::MB_YESNO | windows::Win32::UI::WindowsAndMessaging::MB_ICONQUESTION;
        if self.identity_pending || install::ask(text, style) != windows::Win32::UI::WindowsAndMessaging::IDYES {
            return;
        }
        self.identity_pending = true;
        let hwnd = self.hwnd.0 as usize;
        let spawned = std::thread::Builder::new().name("identity".into()).spawn(move || {
            let result = match install::run_elevated("--prepare-identity") {
                Ok(true) => 0,
                Err(e) if install::cancelled(&e) => 2,
                Ok(false) | Err(_) => 1,
            };
            let _ = unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), WM_IDENTITY, WPARAM(result), LPARAM(0)) };
        });
        if let Err(e) = spawned {
            self.identity_pending = false;
            error_log!("failed to spawn identity thread: {e}");
        }
    }

    /// The Administrator step finished. The package is registered by a temporary copy of Crest
    /// (this instance may hold the old package identity), then Crest restarts.
    pub(super) fn on_identity(&mut self, result: usize) {
        if !std::mem::take(&mut self.identity_pending) {
            return;
        }
        match result {
            0 => {
                let copy = std::env::temp_dir().join(identity::REGISTER_COPY);
                match std::env::current_exe().and_then(|exe| std::fs::copy(exe, &copy)) {
                    Ok(_) => {
                        install::launch(&copy, Some("--register-identity"));
                        unsafe { PostQuitMessage(0) };
                    }
                    Err(e) => error_log!("failed to create registration copy: {e}"),
                }
            }
            // Administrator permission denied: nothing changed.
            2 => {}
            _ => {
                let text = "Could not enable incoming messages. Details are in the log\n\
                            (tray icon > Settings > Open log folder).";
                install::ask(text, windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR);
            }
        }
    }

    pub(super) fn on_service(&mut self) {
        let mut battery_update = false;
        while let Some(event) = self.services.next_event() {
            match event {
                Event::Media(snapshot) => {
                    self.renderer.set_art(snapshot.as_ref().and_then(|s| s.art.as_deref()));
                    // A new track must not keep anything measured against the old one: the
                    // elapsed time, the seek preview, the lyric line and its scroll position.
                    let changed = match (&self.media, &snapshot) {
                        (Some(old), Some(new)) => old.title != new.title,
                        _ => false,
                    };
                    if changed {
                        self.seek_preview = None;
                        self.lyric_drawn = None;
                        self.lyric_shown = None;
                        self.lyric_scroll = None;
                        self.second_drawn = None;
                        // The old song's lines must go at once, not fade out under the new title.
                        self.lyrics = None;
                        self.lyric_pending = None;
                    }
                    self.media = snapshot;
                    self.request_lyrics();
                }
                Event::Lyrics(found) => {
                    if self.lyric_pending.as_ref() == Some(&found.key) {
                        self.lyric_pending = None;
                    }
                    if self.settings.lyrics && self.media_key().as_ref() == Some(&found.key) {
                        self.lyrics = Some(found);
                    }
                }
                Event::Privacy(usage) => self.on_privacy(usage),
                Event::Agents(rows) => self.on_agents(rows),
                Event::System(stats) => self.system = Some(stats),
                Event::Volume { level, muted, initial } => {
                    self.volume = Some((level, muted));
                    if !initial {
                        self.show_hud(Hud::Volume { level, muted }, VOLUME_HUD_MS);
                    }
                }
                Event::Mic(muted) => self.mic = muted,
                Event::City(found) => match found {
                    Some(city) => {
                        city.save();
                        self.city = Some(city);
                        self.city_status = None;
                        self.update_prayer();
                        self.fetch_weather();
                    }
                    None => self.city_status = Some("City not found".to_string()),
                },
                Event::Weather(found) => {
                    debug_log!("weather: {found:?}");
                    if let Some(found) = found {
                        self.weather = Some((found, Instant::now()));
                    }
                }
                Event::Bluetooth(notice) => {
                    let sound = if notice.connected { Sound::Connect } else { Sound::Disconnect };
                    let hud = Hud::Bluetooth {
                        id: notice.id,
                        name: notice.name,
                        connected: notice.connected,
                        battery: notice.battery,
                        kind: notice.kind,
                    };
                    self.alert(hud, BLUETOOTH_HUD_MS, sound);
                }
                Event::MessagesAccess(allowed) => {
                    debug_log!("notification read permission: {allowed}");
                    self.messages_denied = !allowed;
                    self.services.send(Task::WatchMessages(allowed && self.settings.messages != Messages::Off));
                }
                Event::Message(message) => self.on_message(message),
                Event::Network(state) => self.on_network(state),
                Event::Files { kind, paths } => self.on_files(kind, paths),
                Event::Downloading(state) => self.downloading = state.filter(|_| self.settings.downloads),
                Event::BluetoothBattery { id, battery } => {
                    // A battery level read late is only used if its HUD is still showing.
                    if self.hud_visible
                        && let Some(Hud::Bluetooth { id: shown, battery: slot, .. }) = &mut self.hud
                        && *shown == id
                    {
                        *slot = Some(battery);
                        battery_update = true;
                    }
                }
                Event::Google(result) => self.on_google(result),
                Event::Plugin(name, output) => self.on_plugin(name, output),
                Event::Calendar(fetched) => self.on_calendar(fetched),
                Event::Thumbnail(thumb) => {
                    if self.shelf.get(thumb.id).is_some() {
                        self.renderer.set_thumbnail(&thumb);
                    }
                }
            }
        }
        if battery_update {
            self.set_timer(TIMER_HUD, 2_000);
        }
        self.refresh();
    }

    /// An app just started using the camera or microphone. The first event is only the
    /// initial state when Crest starts, so it is not announced.
    pub(super) fn on_privacy(&mut self, usage: Usage) {
        let started = |now: &[String], before: &[String]| now.iter().find(|app| !before.contains(app)).cloned();
        let hud = if !self.privacy_known {
            None
        } else if let Some(app) = started(&usage.camera, &self.privacy.camera) {
            Some(Hud::Privacy { camera: true, app })
        } else {
            started(&usage.microphone, &self.privacy.microphone).map(|app| Hud::Privacy { camera: false, app })
        };
        self.privacy = usage;
        self.privacy_known = true;
        if let Some(hud) = hud {
            self.alert(hud, PRIVACY_HUD_MS, Sound::Privacy);
            self.set_mood(Mood::Surprised, PRIVACY_HUD_MS + MOOD_MS);
        }
    }

    /// Incoming message from a chat app, filtered by settings. On the lock screen or when
    /// requested, the message content is hidden.
    pub(super) fn on_message(&mut self, message: messages::Message) {
        let wanted = match self.settings.messages {
            Messages::Off => false,
            // From a browser (e.g. WhatsApp Web) only when browser notifications are enabled.
            _ if message.from_browser => self.settings.browser_messages,
            Messages::WhatsApp => message.is_whatsapp(),
            Messages::AllChat => true,
        };
        debug_log!("message from {} (browser: {}), shown: {wanted}", message.app, message.from_browser);
        if !wanted {
            return;
        }
        if let Some(logo) = &message.logo {
            self.renderer.set_logo(&message.aumid, messages::LOGO_PX, logo);
        }
        let messages::Message { app, aumid, sender, text, from_browser, .. } = message;
        // On the lock screen, or in a meeting (camera/mic in use, maybe while screen
        // sharing), others may be watching: hide the sender and content.
        let private = self.locked || self.in_meeting();
        let (app, sender) = if private { (String::new(), app) } else { (app, sender) };
        let text = if self.settings.hide_content || private || text.is_empty() { "New message".to_string() } else { text };
        let hud = Hud::Message { app, aumid, sender, text, browser: from_browser };
        // No sound from Crest: WhatsApp/the browser already plays its own.
        self.show_hud(hud, MESSAGE_HUD_MS);
    }

    /// Fetch the weather for the configured city, then schedule the next refresh.
    pub(super) fn fetch_weather(&mut self) {
        match (&self.city, self.settings.weather) {
            (Some(city), true) => {
                weather::fetch(city, self.services.notifier());
                self.set_timer(TIMER_WEATHER, WEATHER_EVERY_MS);
            }
            _ => self.kill_timer(TIMER_WEATHER),
        }
    }

    /// Wi-Fi connected, disconnected, or lost internet. The first event is only the initial state.
    pub(super) fn on_network(&mut self, state: network::State) {
        debug_log!("network: {state:?}");
        let Some(before) = self.network.replace(state.clone()) else { return };
        if !self.settings.wifi {
            return;
        }
        let change = match (&before.wifi, &state.wifi) {
            (_, Some(name)) if before.wifi.as_ref() != Some(name) => {
                Some((Hud::Wifi { name: name.clone(), connected: true, internet: true }, Some(Sound::Connect)))
            }
            (Some(name), None) => Some((Hud::Wifi { name: name.clone(), connected: false, internet: false }, Some(Sound::Disconnect))),
            (Some(_), Some(name)) if before.internet && !state.internet => {
                Some((Hud::Wifi { name: name.clone(), connected: true, internet: false }, Some(Sound::Notice)))
            }
            _ => None,
        };
        match change {
            Some((hud, Some(sound))) => self.alert(hud, WIFI_HUD_MS, sound),
            Some((hud, None)) => self.show_hud(hud, WIFI_HUD_MS),
            None => {}
        }
    }

    /// Newly written files: screenshots go to the shelf, downloads get a HUD.
    pub(super) fn on_files(&mut self, kind: watch::Kind, paths: Vec<std::path::PathBuf>) {
        let name = |path: &std::path::Path| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match kind {
            watch::Kind::Screenshot => {
                if !self.settings.screenshots || !self.settings.pages.shown(Page::Shelf) {
                    return;
                }
                let added = self.shelf.add(paths);
                let Some((id, path)) = added.last().cloned() else { return };
                self.shelf.save();
                self.pop_tiles(&added);
                let more = added.len() - 1;
                self.services.thumbnails(added);
                self.alert(Hud::Screenshot { id, name: name(&path), more }, SCREENSHOT_HUD_MS, Sound::Notice);
            }
            watch::Kind::Downloads => {
                let Some(path) = paths.first().cloned() else { return };
                if self.settings.downloads {
                    let hud = Hud::Download { name: name(&path), path, more: paths.len() - 1 };
                    self.alert(hud, DOWNLOAD_HUD_MS, Sound::Download);
                }
            }
        }
    }

    /// Start/stop folder watchers according to settings.
    pub(super) fn update_watchers(&mut self) {
        let shots = self.settings.screenshots && self.settings.pages.shown(Page::Shelf);
        if shots != self.screenshots.is_some() {
            self.screenshots = shots.then(|| Watch::start(watch::Kind::Screenshot, self.services.notifier())).flatten();
        }
        if self.settings.downloads != self.downloads.is_some() {
            self.downloads =
                self.settings.downloads.then(|| Watch::start(watch::Kind::Downloads, self.services.notifier())).flatten();
        }
        // Feeds are fetched by the calendar's own timer, so hiding the page only stops the
        // plugin runs: those depend on the page being open.
        if self.settings.plugins && self.settings.pages.shown(Page::Plugins) && self.expanded {
            self.refresh_plugins();
        }
        let agents = self.settings.pages.shown(Page::Agents);
        if agents != self.agent_listener.is_some() {
            self.agent_listener = agents.then(|| agents::Listener::start(self.services.notifier())).flatten();
            if !agents {
                self.agent_rows.clear();
                self.agents_seen = false;
            }
        }
    }

    /// Laptop lock/unlock. Crest stays on the lock screen, but only with the clock and music.
    pub(super) fn on_session_change(&mut self, change: u32) {
        match change {
            WTS_SESSION_LOCK => {
                self.end_edit(true, false);
                self.locked = true;
                self.page = Page::Home;
                self.page_anim = None;
                self.expanded = false;
                self.hovered = None;
                self.apply_visibility();
                self.raise();
                // The Mail page hides its content while the machine is locked.
                self.rebuild_views();
                self.refresh();
            }
            WTS_SESSION_UNLOCK => {
                self.locked = false;
                self.refresh_mode(true);
                self.apply_visibility();
                self.wake_up();
                self.rebuild_views();
                self.refresh();
            }
            _ => {}
        }
    }

    /// Measure the system only while the System page is visible.
    pub(super) fn update_system_watch(&mut self) {
        let want = self.shown && self.expanded && self.page == Page::System;
        if want != self.system_watch {
            self.system_watch = want;
            self.services.send(Task::WatchSystem(want));
        }
    }

    /// Reread agent sessions. `force` scans even when the page is closed (used by the refresh
    /// button); periodic scans only run while the Agents page is open.
    /// New agent rows from the listener: raise a notice for a change, then show the rows.
    pub(super) fn on_agents(&mut self, rows: Vec<agents::Session>) {
        if self.agents_seen && self.settings.agent_notices {
            // One notice per scan; rows are sorted by state, so the most urgent change wins.
            if let Some(change) = agents::changes(&self.agent_rows, &rows).into_iter().next() {
                let (session, needs_input) = match change {
                    agents::Change::NeedsInput(session) => (session, true),
                    agents::Change::Finished(session) => (session, false),
                };
                let sound = if needs_input { Sound::Reminder } else { Sound::FocusDone };
                let hud = Hud::Agent {
                    kind: session.kind,
                    title: session.title,
                    project: session.project,
                    detail: session.detail,
                    needs_input,
                    pid: session.pid,
                };
                self.alert(hud, AGENT_HUD_MS, sound);
            }
        }
        self.agents_seen = true;
        self.agent_rows = rows;
        self.agent_scroll = self.agent_scroll.min(self.agent_rows.len().saturating_sub(agents::VISIBLE));
        if self.expanded && self.page == Page::Agents && !self.animating() {
            self.redraw();
        }
    }

    pub(super) fn on_power_broadcast(&mut self, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match wparam.0 as u32 {
            PBT_APMRESUMEAUTOMATIC => {
                self.on_clock();
                self.wake_up();
                None
            }
            PBT_POWERSETTINGCHANGE => {
                if let Some(change) = power::parse(lparam) {
                    self.on_power(change);
                }
                Some(LRESULT(1))
            }
            _ => None,
        }
    }

    pub(super) fn on_power(&mut self, change: power::Change) {
        match change {
            power::Change::Plugged(plugged) => {
                // The first event is only the initial state when Crest starts.
                if self.plugged.replace(plugged).is_some_and(|old| old != plugged) {
                    let percent = self.battery.or_else(power::battery_percent);
                    let sound = if plugged { Sound::Connect } else { Sound::Disconnect };
                    self.alert(Hud::Power { plugged, percent, low: false }, POWER_HUD_MS, sound);
                }
            }
            power::Change::Percent(percent) => {
                let before = self.battery.replace(percent);
                let crossed = before.is_some_and(|b| LOW_BATTERY.iter().any(|&limit| b > limit && percent <= limit));
                if crossed && self.plugged == Some(false) {
                    let hud = Hud::Power { plugged: false, percent: Some(percent), low: true };
                    self.alert(hud, POWER_HUD_MS + 1_000, Sound::LowBattery);
                    self.set_mood(Mood::Surprised, POWER_HUD_MS + 1_000 + MOOD_MS);
                }
            }
            power::Change::Brightness(level) => {
                // The first event is only the brightness when Crest starts.
                let changed = self.brightness.replace(level).is_some_and(|old| old != level);
                if changed && self.settings.brightness {
                    self.show_hud(Hud::Brightness { level: level as f32 / 100.0 }, BRIGHTNESS_HUD_MS);
                }
            }
        }
        self.refresh();
    }
}
