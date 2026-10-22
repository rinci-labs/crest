//! Tray menu state and the commands it sends back.

use super::*;
use crate::feeds::plugins;

impl App {
    pub(super) fn menu_state(&self) -> MenuState {
        MenuState {
            visible: !self.user_hidden,
            autostart: autostart::is_enabled(),
            settings: self.settings,
            messages_available: self.messages_available,
            messages_denied: self.messages_denied,
            hotkey_taken: self.settings.hotkey && !self.hotkey,
            can_activate: install::is_installed_copy(),
        }
    }

    /// The current page just left the tab bar (its feature was switched off): go Home.
    fn leave_hidden_page(&mut self) {
        if self.page != Page::Settings && !self.settings.pages.visible().any(|p| p == self.page) {
            self.page = Page::Home;
            self.page_anim = None;
        }
    }

    pub(super) fn on_command(&mut self, command: Command) {
        match command {
            Command::ToggleVisible => {
                self.user_hidden = !self.user_hidden;
                self.apply_visibility();
                self.refresh();
            }
            Command::ToggleAutostart => {
                if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                    error_log!("failed to change autostart: {e}");
                }
            }
            Command::ToggleLyrics => {
                self.settings.lyrics = !self.settings.lyrics;
                self.settings.save();
                if self.settings.lyrics {
                    self.request_lyrics();
                } else {
                    self.lyrics = None;
                    self.lyric_pending = None;
                }
                self.refresh();
            }
            Command::ToggleSound => {
                self.settings.sound = !self.settings.sound;
                self.settings.save();
            }
            Command::ToggleLockKeys => {
                self.settings.lock_keys = !self.settings.lock_keys;
                self.settings.save();
                self.set_raw_keyboard(self.settings.lock_keys);
            }
            Command::ToggleBrightness => {
                self.settings.brightness = !self.settings.brightness;
                self.settings.save();
            }
            Command::ToggleHideContent => {
                self.settings.hide_content = !self.settings.hide_content;
                self.settings.save();
            }
            Command::Messages(mode) => {
                self.settings.messages = mode;
                self.settings.save();
                if mode == Messages::Off {
                    self.services.send(Task::WatchMessages(false));
                } else if self.messages_available {
                    // Check permission (and request it if never asked); the result turns watching on.
                    messages::request_access(self.services.notifier());
                }
            }
            Command::ToggleHotkey => {
                self.settings.hotkey = !self.settings.hotkey;
                self.settings.save();
                self.set_hotkey(self.settings.hotkey);
            }
            Command::ToggleWifi => {
                self.settings.wifi = !self.settings.wifi;
                self.settings.save();
                self.network = None;
                self.services.send(Task::WatchNetwork(self.settings.wifi));
            }
            Command::ToggleDownloads => {
                self.settings.downloads = !self.settings.downloads;
                self.downloading = None;
                self.settings.save();
                self.update_watchers();
            }
            Command::ToggleMeeting => {
                self.settings.meeting = !self.settings.meeting;
                self.settings.save();
            }
            Command::ToggleWeather => {
                self.settings.weather = !self.settings.weather;
                self.settings.save();
                if !self.settings.weather {
                    self.weather = None;
                } else if self.city.is_none() {
                    // No city yet: open the controls page and ask for the city name right away.
                    self.page = Page::Controls;
                    self.page_anim = None;
                    self.set_expanded(true);
                    self.begin_edit(Field::City);
                }
                self.fetch_weather();
                self.refresh();
            }
            Command::ToggleMail => {
                self.settings.mail = !self.settings.mail;
                self.settings.sync_pages();
                self.settings.save();
                self.mail_status = None;
                self.load_google();
                self.rebuild_views();
                self.leave_hidden_page();
                self.refresh();
            }
            Command::ToggleGoogleCalendar => {
                self.settings.google_calendar = !self.settings.google_calendar;
                self.settings.save();
                if self.settings.google_calendar {
                    self.fetch_calendars();
                    self.refresh_google();
                } else {
                    self.kill_timer(TIMER_CALENDARS);
                    self.google_events.clear();
                }
                self.rebuild_views();
            }
            Command::TogglePlugins => {
                self.settings.plugins = !self.settings.plugins;
                self.settings.sync_pages();
                self.settings.save();
                if self.settings.plugins {
                    // The folder explains the format with a commented example.
                    plugins::write_example();
                    self.plugins = Plugins::load(true);
                    self.refresh_plugins();
                } else {
                    // A hidden feature keeps no rows, so nothing stale is shown later.
                    self.plugins = Plugins::default();
                }
                self.rebuild_views();
                self.leave_hidden_page();
                self.refresh();
            }
            Command::GoogleConnect => {
                // The page holds the setup boxes, so the sign-in happens where the text was typed.
                self.settings.mail = true;
                self.settings.sync_pages();
                self.settings.save();
                self.go_page(Page::Mail, 1.0);
                if google::Client::load().is_some() || !self.google_id.trim().is_empty() {
                    self.connect_google();
                } else {
                    self.mail_status = Some("Paste the client id on this page".to_string());
                    self.refresh();
                }
            }
            Command::GoogleDisconnect => self.disconnect_google(),
            Command::PluginsFolder => plugins::open_folder(self.hwnd),
            Command::FeedsFile => self.open_feeds_file(),
            Command::ToggleCalendar => {
                self.settings.calendar = !self.settings.calendar;
                self.settings.save();
                // Setting the column up is what asks for feeds; hiding it stops fetching them.
                self.fetch_calendars();
                self.refresh();
            }
            Command::ToggleAgentNotices => {
                self.settings.agent_notices = !self.settings.agent_notices;
                self.settings.save();
            }
            Command::ToggleCompactHud => {
                self.settings.compact_hud = !self.settings.compact_hud;
                self.settings.save();
            }
            Command::ToggleBrowserMessages => {
                self.settings.browser_messages = !self.settings.browser_messages;
                self.settings.save();
            }
            Command::ToggleScreenshots => {
                self.settings.screenshots = !self.settings.screenshots;
                self.settings.save();
                self.update_watchers();
            }
            Command::TogglePage(page) => {
                let shown = !self.settings.pages.shown(page);
                self.settings.pages.set_shown(page, shown);
                self.settings.save();
                if !shown && self.page == page {
                    self.page = Page::Home;
                    self.page_anim = None;
                }
                self.update_watchers();
                self.refresh();
            }
            Command::Size(size) => {
                self.settings.size = size;
                self.settings.save();
                self.place();
            }
            Command::Theme(theme) => {
                self.settings.theme = theme;
                self.settings.save();
                self.renderer.set_theme(theme);
                self.redraw();
            }
            Command::PrayerMethod(method) => {
                self.settings.prayer_method = method;
                self.settings.save();
                self.update_prayer();
                self.refresh();
            }
            Command::PrayerMadhab(madhab) => {
                self.settings.prayer_madhab = madhab;
                self.settings.save();
                self.update_prayer();
                self.refresh();
            }
            Command::PrayerLead(lead) => {
                self.settings.prayer_lead = lead;
                self.settings.save();
                self.refresh();
            }
            Command::ActivateMessages => self.activate_messages(),
            Command::OpenLogs => {
                if let Some(folder) = crate::shell::log::folder() {
                    let _ = std::fs::create_dir_all(&folder);
                    let _ = std::process::Command::new("explorer.exe").arg(folder).spawn();
                }
            }
            Command::Exit => unsafe { PostQuitMessage(0) },
        }
    }
}
