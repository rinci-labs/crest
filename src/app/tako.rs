//! Tako overlay: the connection, donations as notices, and the Tako page's data.

use super::*;
use crate::feeds::tako::{self, Choice, Countdown, Gift, Goal, Rank, Song, Status, Update, Vip};
use crate::system::player;
use crate::ui::{PlayerView, TakoGoal, TakoRow, TakoSection, TakoView};

/// Donations kept for the Recent view, newest first. Only in memory: Tako keeps the history.
const RECENT_GIFTS: usize = 20;
/// How often the player looks for Edge's window and keeps it docked over the slot.
pub(super) const PLAYER_POLL_MS: u32 = 200;
/// Edge that has not shown the page by now is not going to; the link opens in the browser.
const PLAYER_FIND_SECS: f64 = 15.0;

/// A video playing in the player shape.
pub(super) struct Playing {
    session: player::Session,
    url: String,
    /// When it started, so a window that never appears is given up on.
    since: f64,
}

/// Everything the Tako page and its notices need.
#[derive(Default)]
pub(super) struct TakoState {
    pub(super) listener: Option<tako::Listener>,
    pub(super) status: Option<Status>,
    pub(super) gifts: Vec<Gift>,
    pub(super) board: Vec<Rank>,
    pub(super) goal: Option<Goal>,
    pub(super) songs: Vec<Song>,
    pub(super) poll: Vec<Choice>,
    pub(super) countdown: Option<Countdown>,
    pub(super) vips: Vec<Vip>,
    pub(super) section: TakoSection,
    /// Text of the link box until Connect is pressed.
    pub(super) link: String,
    /// Why the last Connect did not work.
    pub(super) problem: Option<String>,
    /// A key is saved; read once at start and kept in step by Connect and Disconnect, so
    /// rebuilding the page does not decrypt the key file.
    pub(super) has_key: bool,
}

impl TakoState {
    /// Forget everything read from the overlay (the key and the chosen section stay).
    fn clear(&mut self) {
        self.listener = None;
        self.status = None;
        self.gifts.clear();
        self.board.clear();
        self.goal = None;
        self.songs.clear();
        self.poll.clear();
        self.countdown = None;
        self.vips.clear();
    }
}

impl App {
    /// Start or stop the connection to match the switch, the page, and the saved key.
    pub(super) fn sync_tako(&mut self) {
        let wanted = self.settings.tako && self.settings.pages.shown(Page::Tako);
        match (wanted, self.tako.listener.is_some()) {
            (true, false) => {
                if let Some(key) = tako::key() {
                    self.tako.status = Some(Status::Connecting);
                    self.tako.listener = Some(tako::Listener::start(key, self.services.notifier()));
                }
            }
            // Dropping the listener stops its thread; nothing it reports later is kept.
            (false, true) => self.tako.clear(),
            _ => {}
        }
        self.rebuild_views();
    }

    /// Save the pasted link's key and connect.
    pub(super) fn connect_tako(&mut self) {
        let Some(key) = tako::parse_key(&self.tako.link) else {
            self.tako.problem = Some("No overlay key in that link".to_string());
            self.rebuild_views();
            self.refresh();
            return;
        };
        tako::save_key(&key);
        self.tako.has_key = true;
        // The box held a secret; it is not kept once the key is stored.
        self.tako.link.clear();
        self.tako.problem = None;
        if !self.settings.tako {
            self.settings.tako = true;
            self.settings.sync_pages();
            self.settings.save();
        }
        self.tako.clear();
        self.sync_tako();
        self.refresh();
    }

    /// Forget the key and drop everything read with it.
    pub(super) fn disconnect_tako(&mut self) {
        tako::forget();
        self.tako.has_key = false;
        self.tako.clear();
        self.rebuild_views();
        self.refresh();
    }

    /// Show the Tako page, e.g. to paste the link from Settings.
    pub(super) fn open_tako(&mut self) {
        if !self.settings.tako {
            self.settings.tako = true;
        }
        self.settings.pages.set_shown(Page::Tako, true);
        self.settings.sync_pages();
        self.settings.save();
        self.sync_tako();
        self.go_page(Page::Tako, 1.0);
        self.set_expanded(true);
        self.refresh();
    }

    /// Open a row's link: a media share's video or a song.
    pub(super) fn open_tako_row(&mut self, index: usize) {
        let url = match self.tako.section {
            TakoSection::Recent => self.tako.gifts.get(index).and_then(|g| g.media.as_ref()).filter(|m| !m.flagged).map(|m| m.url.clone()),
            TakoSection::Songs => self.tako.songs.get(index).map(|s| s.url.clone()),
            _ => None,
        };
        let line = match self.tako.section {
            TakoSection::Recent => self.tako.gifts.get(index).map(|g| (g.name.clone(), gift_line(g, true))),
            TakoSection::Songs => self.tako.songs.get(index).map(|s| (s.sender.clone(), format!("{} · {}", s.artist, s.title))),
            _ => None,
        };
        if let (Some(url), Some((name, line))) = (url.filter(|u| !u.is_empty()), line) {
            self.play(&url, name, line);
        }
    }

    /// Play a media share or song in the player shape: the island grows into a 16:9 slot and
    /// Edge's window is docked over it. Without Edge, a link the player accepts opens in the
    /// default browser instead; any other link is left alone.
    pub(super) fn play(&mut self, url: &str, name: String, line: String) {
        if !player::allowed(url) {
            return;
        }
        self.close_player();
        let slot = self.player_slot(View::Player.size().0);
        let Some(session) = player::Session::start(url, slot) else {
            dnd::open_url(url);
            return;
        };
        self.player = Some(Playing { session, url: url.to_string(), since: self.now() });
        self.player_view = Some(PlayerView { name, line, ready: false });
        self.set_timer(TIMER_PLAYER, PLAYER_POLL_MS);
        // The panel gives way to the player; closing the player brings the pill back.
        self.set_expanded(false);
        self.refresh();
    }

    /// Stop the video and close its window; the shape shrinks back on the next refresh.
    pub(super) fn close_player(&mut self) {
        if let Some(mut playing) = self.player.take() {
            playing.session.close();
            self.kill_timer(TIMER_PLAYER);
        }
    }

    /// Open the playing video in the default browser and close the player.
    pub(super) fn player_to_browser(&mut self) {
        if let Some(url) = self.player.as_ref().map(|p| p.url.clone()) {
            self.close_player();
            dnd::open_url(&url);
        }
    }

    /// Poll: find Edge's window, notice it closing, and keep it docked.
    pub(super) fn on_player_timer(&mut self) {
        let now = self.now();
        let Some(playing) = &mut self.player else {
            self.kill_timer(TIMER_PLAYER);
            return;
        };
        if playing.session.ended() {
            self.close_player();
            self.refresh();
            return;
        }
        if !playing.session.find() && now - playing.since > PLAYER_FIND_SECS {
            error_log!("player: Edge window did not appear");
            self.player_to_browser();
            self.refresh();
            return;
        }
        self.dock_player();
    }

    /// Show the video over the slot once the shape has settled, and hide it while the shape
    /// moves, fades, or Crest is hidden: a window cannot follow the spring frame by frame.
    pub(super) fn dock_player(&mut self) {
        let settled = self.shown
            && !self.locked
            && self.view == View::Player
            && !self.width.is_moving()
            && !self.height.is_moving()
            && View::Player.content().is_some_and(|i| self.alpha[i] >= 1.0);
        let slot = self.player_slot(self.width.value);
        let radius = (ui::PLAYER_RADIUS * self.renderer.scale()).round() as i32;
        let Some(playing) = &mut self.player else { return };
        if settled {
            playing.session.dock(slot, radius);
        } else {
            playing.session.hide();
        }
        let ready = playing.session.ready() && settled;
        if let Some(view) = &mut self.player_view
            && view.ready != ready
        {
            view.ready = ready;
            if !self.animating() {
                self.redraw();
            }
        }
    }

    /// The video slot of a player shape `width` DIP wide, in screen pixels.
    fn player_slot(&self, width: f32) -> RECT {
        let scale = self.renderer.scale();
        let origin = self.window_pos();
        let slot = ui::player_slot(width);
        let px = |v: f32| (v * scale).round() as i32;
        RECT {
            left: origin.x + px(slot.left),
            top: origin.y + px(slot.top),
            right: origin.x + px(slot.right),
            bottom: origin.y + px(slot.bottom),
        }
    }

    pub(super) fn on_tako(&mut self, update: Update) {
        // A listener that was stopped may still deliver what it had in hand.
        if self.tako.listener.is_none() {
            return;
        }
        match update {
            Update::Status(status) => self.tako.status = Some(status),
            Update::Gift(gift) => self.on_tako_gift(gift),
            Update::Board(board) => self.tako.board = board,
            Update::Goal(goal) => self.tako.goal = goal,
            Update::Songs(songs) => self.tako.songs = songs,
            Update::Poll(poll) => self.tako.poll = poll,
            Update::Countdown(countdown) => self.tako.countdown = countdown,
            Update::Vips(vips) => self.tako.vips = vips,
        }
        self.rebuild_views();
        self.refresh();
    }

    /// A donation: kept for the Recent list, and a notice if that kind is switched on.
    fn on_tako_gift(&mut self, gift: Gift) {
        let wanted = if gift.sound.is_some() { self.settings.tako_sounds } else { self.settings.tako_alerts };
        if wanted {
            // The lock screen keeps donors' messages to itself, like chat messages.
            let private = self.locked || !self.settings.tako_messages;
            let hud = Hud::Tako {
                name: gift.name.clone(),
                value: gift.value(),
                line: gift_line(&gift, private),
                initials: gift.initials(),
                url: gift.media.as_ref().filter(|m| !m.flagged).map(|m| m.url.clone()).unwrap_or_default(),
            };
            self.alert(hud, TAKO_HUD_MS, Sound::Reminder);
        }
        if !gift.test {
            self.tako.gifts.insert(0, gift);
            self.tako.gifts.truncate(RECENT_GIFTS);
        }
    }

    /// The Tako page from the state above.
    pub(super) fn build_tako(&self) -> TakoView {
        // The lock screen shows nothing of donors or amounts.
        if self.locked {
            return TakoView::default();
        }
        let editing = self.edit_field == Some(Field::TakoKey);
        let status = self.tako.status.unwrap_or(Status::Connecting);
        let shown = if self.settings.tako { status.label() } else { "Off" };
        let private = !self.settings.tako_messages;
        TakoView {
            setup: !self.tako.has_key,
            link: if editing { self.edit_text.clone() } else { self.tako.link.clone() },
            editing,
            status: shown.to_string(),
            live: self.settings.tako && status == Status::Live,
            failing: self.settings.tako && matches!(status, Status::Offline | Status::Rejected),
            problem: self.tako.problem.clone(),
            section: self.tako.section,
            gifts: self
                .tako
                .gifts
                .iter()
                .map(|g| TakoRow {
                    name: g.name.clone(),
                    value: g.value(),
                    message: gift_line(g, private),
                    initials: g.initials(),
                    url: g.media.as_ref().filter(|m| !m.flagged).map(|m| m.url.clone()).unwrap_or_default(),
                })
                .collect(),
            board: self.tako.board.iter().map(|r| (r.name.clone(), tako::rupiah(r.amount))).collect(),
            goal: self.tako.goal.as_ref().map(|g| TakoGoal {
                title: g.title.clone(),
                current: tako::rupiah(g.current),
                target: tako::rupiah(g.target),
                fraction: g.fraction(),
            }),
            songs: self
                .tako
                .songs
                .iter()
                .map(|s| TakoRow {
                    name: s.title.clone(),
                    value: if s.duration > 0 { format!("{}:{:02}", s.duration / 60, s.duration % 60) } else { String::new() },
                    message: format!("{} · from {}", s.artist, s.sender),
                    initials: String::new(),
                    url: s.url.clone(),
                })
                .collect(),
            poll: self.tako.poll.iter().map(|c| (c.name.clone(), tako::rupiah(c.amount), c.percent as f32 / 100.0)).collect(),
            timer: self.tako.countdown.map(countdown_text),
            vips: self
                .tako
                .vips
                .iter()
                .map(|v| TakoRow {
                    name: v.name.clone(),
                    value: tako::rupiah(v.amount),
                    message: if v.detail.is_empty() { "No details".to_string() } else { v.detail.clone() },
                    initials: String::new(),
                    url: String::new(),
                })
                .collect(),
        }
    }
}

/// The line under a donation's name: the video for a media share, the sound for a soundboard
/// donation, otherwise the message (or a neutral line when `private` or there is none).
fn gift_line(gift: &Gift, private: bool) -> String {
    if let Some(media) = &gift.media {
        return media.line();
    }
    if let Some(sound) = &gift.sound {
        return format!("Played “{sound}”");
    }
    match (private || gift.message.is_empty(), gift.test) {
        (true, true) => "Test from the Tako dashboard".to_string(),
        (true, false) => "Sent a donation".to_string(),
        (false, _) => gift.message.clone(),
    }
}

/// The Timer section's two lines, to the minute: the page is redrawn on the minute tick.
fn countdown_text(countdown: Countdown) -> (String, String) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let left = countdown.left(now);
    let minutes = left.div_ceil(60);
    let big = match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    };
    let line = if countdown.running(now) {
        "left on the stream timer"
    } else if countdown.ends_at > 0 {
        "Time is up"
    } else {
        "Set, not running"
    };
    if countdown.ends_at > 0 && !countdown.running(now) { ("0 min".to_string(), line.to_string()) } else { (big, line.to_string()) }
}
