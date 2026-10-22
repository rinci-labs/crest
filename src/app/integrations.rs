//! Google account, calendar feeds, and plugins: background refreshes and their results.

use super::*;

impl App {
    /// Read the saved account and start the first refresh, or explain what is missing.
    pub(super) fn load_google(&mut self) {
        if !google::connected() {
            self.mail_status = Some("Not connected".to_string());
            self.rebuild_views();
            return;
        }
        if !self.settings.mail {
            self.mail_status = Some("Mail is off".to_string());
            self.rebuild_views();
            return;
        }
        self.account = crate::shell::store::load("google-account.txt").into_iter().next().filter(|a| !a.trim().is_empty());
        self.mail_status = Some("Checking…".to_string());
        self.rebuild_views();
        self.refresh_google();
    }

    /// Refresh the access token and read whatever is switched on.
    pub(super) fn refresh_google(&mut self) {
        let Some(client) = google::Client::load() else { return };
        let Some(refresh) = google::token() else { return };
        // Only the parts the user turned on are fetched, so nothing is read behind their back.
        let mail = self.settings.mail;
        let calendar = self.settings.google_calendar;
        if !mail && !calendar {
            return;
        }
        self.google_due = Some(Instant::now() + GOOGLE_REFRESH);
        self.set_timer(TIMER_GOOGLE, GOOGLE_REFRESH.as_millis() as u32);
        google::spawn(google::Step::Refresh { client, refresh, mail, calendar }, self.services.notifier());
    }

    /// The newest message Crest has not shown before, and how many other new ones came with it.
    ///
    /// The first read only records what is there: an unread message that predates Crest must not
    /// arrive as a notice. After that, a message whose id was not in the previous list is news.
    fn new_mail(&mut self, rows: &[google::Mail]) -> Option<(google::Mail, usize)> {
        let seeds = !self.mail_seen;
        self.mail_seen = true;
        let (first, more) = fresh_mail(&self.mail_known, rows, seeds)?;
        self.mail_known = rows.iter().map(|row| row.id.clone()).collect();
        Some((first, more))
    }

    /// Give up on a sign-in that is still waiting for the browser.
    ///
    /// The worker thread stays blocked on its listener until the timeout; it is detached, and
    /// whatever it reports later is ignored because `connecting` is already false. Google never
    /// redirects when it refuses the sign-in, so without this the page would wait for minutes.
    pub(super) fn cancel_google(&mut self) {
        if !self.connecting {
            return;
        }
        self.connecting = false;
        self.mail_status = Some("Sign-in cancelled".to_string());
        self.rebuild_views();
        self.refresh();
    }

    /// Start the browser sign-in: bind the loopback port first, then open the consent page.
    pub(super) fn connect_google(&mut self) {
        if self.connecting {
            return;
        }
        // Pressing Connect is the user asking for mail: a page whose switch is off would swallow
        // the whole flow (the view then only says "Mail is off"), so the switch follows the click.
        if !self.settings.mail {
            self.settings.mail = true;
            self.settings.save();
        }
        // The boxes hold the text; saving here means Connect always uses what is on screen.
        let client = google::Client { id: self.google_id.trim().to_string(), secret: self.google_secret.trim().to_string() };
        if client.id.is_empty() {
            self.mail_status = Some("Paste the client id first".to_string());
            self.rebuild_views();
            self.refresh();
            return;
        }
        // A Google "Desktop app" client requires both parts: the token exchange is rejected with
        // invalid_client when the secret is missing, and that failure arrives only after the
        // browser round trip, so it is caught here instead.
        if client.secret.is_empty() {
            client.save();
            self.mail_status = Some("Paste the client secret too (Google shows it with the id)".to_string());
            self.rebuild_views();
            self.refresh();
            return;
        }
        client.save();
        let (verifier, challenge) = google::pkce();
        let state = crate::system::secrets::random_b64(16);
        let Ok((listener, port)) = google::listener() else {
            self.mail_status = Some("Cannot open a local port for the reply".to_string());
            self.rebuild_views();
            return;
        };
        let url = google::consent_url(&client, &challenge, &state, port);
        self.connecting = true;
        self.mail_status = Some("Waiting for the browser…".to_string());
        self.rebuild_views();
        // The listener moves to the worker thread, which blocks on the reply.
        google::spawn(google::Step::Authorize { client, verifier, state, listener }, self.services.notifier());
        google::open_browser(&url);
        self.refresh();
    }

    /// The sign-in finished, or new mail and calendar data arrived.
    pub(super) fn on_google(&mut self, result: std::result::Result<google::Outcome, String>) {
        match result {
            Ok(google::Outcome::Authorized { refresh, account }) => {
                // A reply that arrives after Cancel must not connect the account behind the
                // user's back.
                if !self.connecting {
                    return;
                }
                self.connecting = false;
                google::save_token(&refresh);
                if !account.is_empty() {
                    crate::shell::store::save("google-account.txt", [account.clone()]);
                }
                self.account = Some(account).filter(|a| !a.is_empty());
                self.mail_status = Some("Checking…".to_string());
                self.refresh_google();
            }
            Ok(google::Outcome::Data { snapshot, auth }) => {
                self.connecting = false;
                if auth {
                    // The stored token was revoked or expired: ask for a new sign-in instead of
                    // retrying every minute.
                    google::forget();
                    self.google_due = None;
                    self.kill_timer(TIMER_GOOGLE);
                    self.account = None;
                    self.mail.clear();
                    self.google_events.clear();
                    self.mail_status = Some("Sign in again".to_string());
                } else {
                    self.mail_status = None;
                    let fresh = self.new_mail(&snapshot.mail);
                    self.mail = snapshot.mail;
                    self.google_events = snapshot.events;
                    if let Some((mail, more)) = fresh {
                        let (from, subject, initials) = (mail.from, mail.subject, mail.initials);
                        self.alert(Hud::Mail { from, subject, initials, more }, MAIL_HUD_MS, Sound::Reminder);
                    }
                }
            }
            Err(e) => {
                self.connecting = false;
                // Sign-in failures are worth showing; a read failure is not.
                if self.mail_status.as_deref() == Some("Waiting for the browser…") {
                    self.mail_status = Some(e.clone());
                }
                debug_log!("google: {e}");
            }
        }
        self.rebuild_views();
        self.refresh();
    }

    /// Forget the account and stop refreshing.
    pub(super) fn disconnect_google(&mut self) {
        google::forget();
        let _ = crate::shell::store::dir().map(|dir| std::fs::remove_file(dir.join("google-account.txt")));
        self.account = None;
        self.mail.clear();
        self.google_events.clear();
        self.google_due = None;
        self.kill_timer(TIMER_GOOGLE);
        self.mail_status = Some("Not connected".to_string());
        self.rebuild_views();
        self.refresh();
    }

    /// Once a minute, decide which background work is worth doing.
    ///
    /// Mail is refreshed while its page is open, at most every few minutes. Plugins run while
    /// their page is open. Feeds follow the calendar, which changes slowly.
    pub(super) fn tick_integrations(&mut self) {
        let mail = self.settings.mail && self.settings.pages.shown(Page::Mail) && self.expanded;
        if mail && self.google_due.is_none_or(|due| Instant::now() >= due) && google::connected() {
            self.refresh_google();
        }
        let plugins = self.settings.plugins && self.settings.pages.shown(Page::Plugins) && self.expanded;
        if plugins {
            self.refresh_plugins();
        }
        if self.settings.google_calendar && self.settings.calendar {
            self.fetch_calendars();
        }
    }

    /// Fetch calendar feeds that are missing or old; also schedules the next refresh.
    ///
    /// `calendars.txt` is read again first, so edits made in Notepad (the only way to add or
    /// remove a feed) take effect without a restart.
    pub(super) fn fetch_calendars(&mut self) {
        if !self.settings.google_calendar || !self.settings.calendar {
            self.kill_timer(TIMER_CALENDARS);
            return;
        }
        self.feeds.reload();
        for (index, source) in self.feeds.stale() {
            calfeeds::fetch(index, source, self.services.notifier());
        }
        self.set_timer(TIMER_CALENDARS, calfeeds::REFRESH.as_millis() as u32);
    }

    /// One feed was fetched.
    pub(super) fn on_calendar(&mut self, fetched: calfeeds::Fetched) {
        let index = fetched.index;
        if index >= self.feeds.sources.len() {
            return;
        }
        if self.feeds.fetched.len() <= index {
            return;
        }
        self.feeds.fetched[index] = Some(Instant::now());
        while self.feeds.errors.len() <= index {
            self.feeds.errors.push(None);
        }
        self.feeds.errors[index] = fetched.error;
        if let Some(feed) = fetched.feed {
            while self.feeds.feeds.len() <= index {
                self.feeds.feeds.push(calfeed::Feed::default());
            }
            self.feeds.feeds[index] = feed;
        }
        self.refresh();
    }

    /// Run the plugins that are due while the Plugins page is open.
    pub(super) fn refresh_plugins(&mut self) {
        if !self.settings.plugins && !self.plugins.list.is_empty() {
            self.plugins = Plugins::default();
            self.rebuild_views();
            return;
        }
        for plugin in self.plugins.due(self.now()) {
            let name = plugin.name.clone();
            let notify = self.services.notifier();
            plugins::run(plugin, move |output| {
                notify.send(Event::Plugin(name, output));
            });
        }
    }

    /// Output from a plugin.
    pub(super) fn on_plugin(&mut self, name: String, mut output: plugins::Output) {
        output.ran = Some(self.now());
        self.plugins.output.insert(name, output);
        self.rebuild_views();
        self.refresh();
    }

    /// Open `calendars.txt` in Notepad: one feed per line as `name|address`.
    pub(super) fn open_feeds_file(&mut self) {
        let Some(dir) = crate::shell::store::dir() else { return };
        let path = dir.join("calendars.txt");
        if !path.exists() {
            crate::shell::store::save(
                "calendars.txt",
                ["# One calendar per line: name|https://calendar.google.com/calendar/ical/…/basic.ics".to_string()],
            );
        }
        crate::system::dnd::open(self.hwnd, &path);
    }

    /// Reload the plugin folder, e.g. after the folder was edited.
    pub(super) fn reload_plugins(&mut self) {
        let running = std::mem::take(&mut self.plugins.output);
        self.plugins = Plugins::load(self.settings.plugins);
        self.plugins.output = running;
        self.refresh_plugins();
        self.rebuild_views();
        self.refresh();
    }
}

/// The newest message that was not in `known`, and how many others came with it.
///
/// `seed` is the first read after start: it records the ids without reporting anything, so an
/// unread message that predates Crest never arrives as a notice.
fn fresh_mail(known: &[String], rows: &[google::Mail], seed: bool) -> Option<(google::Mail, usize)> {
    let fresh: Vec<&google::Mail> = rows.iter().filter(|row| !known.iter().any(|id| id == &row.id)).collect();
    if seed {
        return None;
    }
    let (first, rest) = fresh.split_first()?;
    Some(((*first).clone(), rest.len()))
}

#[cfg(test)]
mod mail_checks {
    use super::*;

    fn message(id: &str) -> google::Mail {
        google::Mail {
            id: id.into(),
            from: "Ana".into(),
            subject: format!("subject {id}"),
            age: "1m".into(),
            initials: "A".into(),
        }
    }

    /// The first read is silent, and only later arrivals are announced.
    #[test]
    fn only_mail_that_arrives_while_running_is_new() {
        let first = [message("a"), message("b")];
        // Seeding: nothing is new, whatever is there.
        assert!(fresh_mail(&[], &first, true).is_none());
        let known: Vec<String> = first.iter().map(|m| m.id.clone()).collect();
        // The same list again: still nothing new.
        assert!(fresh_mail(&known, &first, false).is_none());
        // One more message: that one is the news, with no others.
        let mut with_new = first.to_vec();
        with_new.insert(0, message("c"));
        let (new, more) = fresh_mail(&known, &with_new, false).unwrap();
        assert_eq!((new.id.as_str(), more), ("c", 0));
        // Two at once: the first is shown and the other is counted.
        let mut two = with_new.clone();
        two.insert(0, message("d"));
        let (new, more) = fresh_mail(&known, &two, false).unwrap();
        assert_eq!((new.id.as_str(), more), ("d", 1));
        // Reading a message (it leaves the unread list) announces nothing.
        assert!(fresh_mail(&known, &[], false).is_none());
    }
}
