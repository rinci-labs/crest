use std::path::{Path, PathBuf};

use super::*;
use crate::tools::shelf::Shelf;
use crate::tools::tasks::Tasks;
use crate::feeds::agents::{Kind as AgentKind, Limits, Meter, State as AgentState};

/// Wall clock of the sample data, for reset countdowns.
const SAMPLE_NOW: u64 = 1_800_000_000;

/// Sample data for every page.
struct Sample {
    media: MediaView,
    shelf: Shelf,
    tasks: Tasks,
    focus: FocusView,
    water: WaterView,
    prayer: PrayerView,
    privacy: Usage,
    system: Stats,
    timer: TimerView,
    controls: ControlsView,
    shelf_fx: ShelfFx,
    agents: Vec<crate::feeds::agents::Session>,
    limits: Vec<Limits>,
    mail: MailView,
    plugins: PluginsView,
    tako: TakoView,
}

/// Sessions per Agents filter key, as the app counts them.
fn agent_counts(rows: &[crate::feeds::agents::Session]) -> [usize; 4] {
    AgentFilter::ALL.map(|filter| rows.iter().filter(|row| filter.keeps(row.state)).count())
}

/// The rows a filter keeps.
fn agent_list(rows: &[crate::feeds::agents::Session], filter: AgentFilter) -> Vec<crate::feeds::agents::Session> {
    rows.iter().filter(|row| filter.keeps(row.state)).cloned().collect()
}

/// The filter a preview's name asks for ("page_agents_waiting").
fn agent_filter(name: &str) -> AgentFilter {
    if name.ends_with("_waiting") { AgentFilter::Waiting } else { AgentFilter::All }
}

fn agent(id: &str, kind: AgentKind, title: &str, project: &str, state: AgentState, detail: &str, age_secs: u64) -> crate::feeds::agents::Session {
    crate::feeds::agents::Session {
        id: id.into(),
        kind,
        title: title.into(),
        project: project.into(),
        state,
        detail: detail.into(),
        age_secs,
        pid: 4242,
        folder: format!("C:\\work\\{project}"),
    }
}

fn sample() -> Sample {
    let mut shelf = Shelf::default();
    shelf.add(["Project", "Corporate and Campaign.png", "Infuse Navy Tote Bag.png"].map(|n| PathBuf::from(format!("C:\\sample\\{n}"))));
    let mut tasks = Tasks::default();
    for text in ["Send the design revision to the client", "Buy coffee", "Tidy up the Downloads folder that is completely full"] {
        tasks.add(text);
    }
    tasks.add("Pay the electricity bill");
    tasks.toggle(1);
    Sample {
        media: MediaView {
            title: "Losing You Weighs On Me (a really long title)".into(),
            artist: "The Longing Band".into(),
            playing: true,
            progress: Some(0.38),
            elapsed: "1:06".into(),
            remaining: "-1:37".into(),
            can_previous: true,
            can_next: false,
            can_seek: true,
            seeking: false,
            accent: Some(0x4FC3F7),
        },
        shelf,
        tasks,
        focus: FocusView {
            phase: Phase::Focus,
            running: true,
            active: true,
            clock: "17:42".into(),
            progress: 0.29,
            sessions: 2,
            minutes_left: 18,
        },
        water: WaterView {
            ml: 1250,
            goal: 2000,
            week: vec![(1500, 'T'), (2000, 'W'), (750, 'T'), (1750, 'F'), (2250, 'S'), (1000, 'S'), (1250, 'M')],
            next: Some("19:37".into()),
        },
        prayer: sample_prayer(),
        privacy: Usage { camera: vec![], microphone: vec!["Discord".into()] },
        system: Stats {
            cpu: 23.0,
            gpu: Some(64.0),
            gpu_temp: Some(71.0),
            ram_used: 10_522_669_875,
            ram_total: 16_857_853_952,
            disk_free: 120_000_000_000,
            disk_total: 512_000_000_000,
            down: 1_258_291.0,
            up: 81_920.0,
        },
        timer: idle_timer(),
        shelf_fx: ShelfFx::default(),
        agents: vec![
            agent("claude:1", AgentKind::Claude, "Billing API backfill", "billing-api", AgentState::Waiting, "permission prompt", 40),
            agent("codex:2", AgentKind::Codex, "Migrate auth to passkeys", "accounts", AgentState::Running, "gpt-5.6", 8),
            agent("cursor:3", AgentKind::Cursor, "Restart local web and API dev servers", "web-dashboard", AgentState::Running, "bun dev", 7_500),
            agent("omp:4", AgentKind::Omp, "Write release notes", "crest", AgentState::Done, "", 180),
            agent("copilot:5", AgentKind::Copilot, "Fix flaky upload test", "media-service", AgentState::Idle, "", 1_500),
        ],
        limits: vec![Limits {
            kind: AgentKind::Codex,
            meters: vec![
                Meter { window: "5h".into(), used: 92, resets_at: SAMPLE_NOW + 2 * 3_600 + 14 * 60 },
                Meter { window: "Week".into(), used: 41, resets_at: SAMPLE_NOW + 3 * 86_400 },
            ],
        }],
        controls: ControlsView {
            volume: Some((0.62, false)),
            brightness: Some(70),
            mic: Some(false),
            quiet_minutes: None,
            city: Some("City: Jakarta".into()),
        },
        mail: MailView {
            account: Some("rafli@example.com".into()),
            status: None,
            rows: vec![
                MailRow { from: "Ana Silva".into(), subject: "Re: contract for the autumn campaign".into(), age: "4m".into(), initials: "AS".into() },
                MailRow { from: "GitHub".into(), subject: "[crest] Run failed: test (main)".into(), age: "38m".into(), initials: "G".into() },
                MailRow { from: "Billing".into(), subject: "Your invoice is ready".into(), age: "3h".into(), initials: "B".into() },
            ],
            setup: false,
            id: String::new(),
            secret: String::new(),
            editing: None,
            connecting: false,
        },
        plugins: PluginsView {
            rows: vec![
                plugin_row("inbox", "Inbox 3", "2 unread from Ana", "https://mail.google.com"),
                plugin_row("ci", "CI green", "crest · 4m 12s", "https://github.com/rinci-labs/crest/actions"),
                plugin_row("ci", "Open PR", "#412 Review the calendar feed", "https://github.com/rinci-labs/crest/pull/412"),
            ],
            problems: Vec::new(),
            count: 2,
            empty: false,
        },
        tako: TakoView {
            status: "Live".into(),
            live: true,
            gifts: vec![
                TakoRow {
                    name: "Ana Putri".into(),
                    value: "Rp 25.000".into(),
                    message: "Semangat streamnya kak!".into(),
                    initials: "AP".into(),
                    url: String::new(),
                },
                TakoRow {
                    name: "Budi".into(),
                    value: "Rp 50.000".into(),
                    message: "YouTube · Lagu favorit buat nemenin main".into(),
                    initials: "B".into(),
                    url: "https://www.youtube.com/watch?v=mm0YkIJZJ4w".into(),
                },
                TakoRow { name: "@rafaar".into(), value: "5× Mawar".into(), message: "Sent a donation".into(), initials: "R".into(), url: String::new() },
                TakoRow { name: "Sinta".into(), value: "Rp 10.000".into(), message: "Played “Airhorn”".into(), initials: "S".into(), url: String::new() },
            ],
            songs: vec![
                TakoRow {
                    name: "Sial".into(),
                    value: "4:12".into(),
                    message: "Mahalini · from Ana Putri".into(),
                    url: "https://www.youtube.com/watch?v=a".into(),
                    ..TakoRow::default()
                },
                TakoRow { name: "Tak Segampang Itu".into(), value: "3:41".into(), message: "Anggi Marito · from Budi".into(), ..TakoRow::default() },
            ],
            poll: vec![
                ("Main Valorant".into(), "Rp 150.000".into(), 0.6),
                ("Main Minecraft".into(), "Rp 75.000".into(), 0.3),
                ("Just chatting".into(), "Rp 25.000".into(), 0.1),
            ],
            timer: Some(("1 h 12 min".into(), "left on the stream timer".into())),
            vips: vec![TakoRow { name: "Ilham".into(), value: "Rp 100.000".into(), message: "Game ID: 1234 · Server: Asia".into(), ..TakoRow::default() }],
            board: vec![
                ("@rafaar".into(), "Rp 12.522.500".into()),
                ("Bagas".into(), "Rp 350.000".into()),
                ("Ilham".into(), "Rp 350.000".into()),
            ],
            goal: Some(TakoGoal { title: "New microphone".into(), current: "Rp 60.000".into(), target: "Rp 1.200.000".into(), fraction: 0.05 }),
            ..TakoView::default()
        },
    }
}

/// The Settings section a preview's name asks for: `_features`, `_prayer`, or `_agents`.
fn settings_section(name: &str) -> SettingsSection {
    SettingsSection::ALL.into_iter().find(|s| name.ends_with(&format!("_{}", s.label().to_lowercase()))).unwrap_or_default()
}

/// The Tako page variant a preview's name asks for: a section ("page_tako_poll"), `_empty`, or
/// `_setup`.
fn tako_view(sample: &TakoView, name: &str) -> TakoView {
    let key = |s: TakoSection| format!("_{}", s.label().to_lowercase().replace(' ', "_"));
    let section = TakoSection::ALL.into_iter().find(|s| name.ends_with(&key(*s))).unwrap_or_default();
    match name {
        _ if name.ends_with("_setup") => TakoView { setup: true, ..TakoView::default() },
        _ if name.ends_with("_empty") => TakoView { status: "Connecting…".into(), ..TakoView::default() },
        _ => TakoView { section, ..sample.clone() },
    }
}

/// Jakarta on 1 October 2026 at 13:20: Asr is next.
fn sample_prayer() -> PrayerView {
    let times = [("Fajr", "04:23"), ("Sunrise", "05:36"), ("Dhuhr", "11:45"), ("Asr", "14:51"), ("Maghrib", "17:49"), ("Isha", "18:59")];
    PrayerView {
        city: Some("Jakarta".into()),
        status: None,
        times: times
            .iter()
            .enumerate()
            .map(|(i, (name, time))| PrayerTime { name, time: (*time).into(), passed: i < 3, next: i == 3 })
            .collect(),
        next: Some(("Asr", "14:51".into(), "in 1 h 31 min".into())),
        method: crate::tools::prayer::Method::Kemenag,
        lead: 10,
    }
}

/// One plugin row for the previews.
fn plugin_row(plugin: &str, title: &str, detail: &str, url: &str) -> crate::feeds::plugins::Row {
    crate::feeds::plugins::Row { plugin: plugin.into(), title: title.into(), detail: detail.into(), url: url.into() }
}

/// Idle timer, for states other than the timer page/pill.
fn idle_timer() -> TimerView {
    TimerView { running: false, active: false, clock: "00:00".into(), short: String::new(), progress: 0.0, total_minutes: 0 }
}

/// One state rendered to a preview image.
struct State<'a> {
    name: &'a str,
    dpi: u32,
    theme: Theme,
    view: View,
    page: Page,
    media: bool,
    lyric: Option<&'a str>,
    hud: Option<&'a Hud>,
    hovered: Option<Hit>,
    locked: bool,
    eyes: Eyes,
    pages: Pages,
    weather: Option<&'a str>,
    city_draft: Option<&'a str>,
    /// Size other than the view's default (e.g. a hovered pill).
    size: Option<(f32, f32)>,
    shadow: Option<f32>,
    fx: Option<&'a ShelfFx>,
    roll: Option<&'a Roll>,
    calendar: Option<&'a CalendarView>,
    agenda_draft: Option<&'a str>,
    download: Option<&'a str>,
    battery: Option<(u32, bool)>,
    lyrics: Option<&'a LyricsView>,
    lyric_mix: f32,
    lyrics_button: Option<bool>,
    welcome: bool,
    agents: bool,
    /// Plan usage meters on the Agents page and Home.
    limits: bool,
    /// Mail page without an account: the setup boxes.
    mail_setup: bool,
    /// Which setup box is being typed into.
    mail_editing: Option<u8>,
    /// Plugins page with no plugin files.
    plugins_empty: bool,
    /// Plugins page where one plugin failed.
    plugins_failed: bool,
    /// The Tako player shape instead of the page.
    player: Option<&'a PlayerView>,
}

impl<'a> State<'a> {
    fn new(name: &'a str, view: View, page: Page) -> Self {
        Self {
            name,
            dpi: 96,
            theme: Theme::Ink,
            view,
            page,
            media: false,
            lyric: None,
            hud: None,
            hovered: None,
            locked: false,
            eyes: Eyes::default(),
            // The tab bar as a fresh install shows it (Mail and Plugins off).
            pages: Settings::default().pages,
            weather: None,
            city_draft: None,
            size: None,
            shadow: None,
            fx: None,
            roll: None,
            calendar: None,
            agenda_draft: None,
            download: None,
            battery: Some((81, false)),
            lyrics: None,
            lyric_mix: 0.0,
            lyrics_button: None,
            welcome: false,
            agents: false,
            limits: false,
            mail_setup: false,
            mail_editing: None,
            plugins_empty: false,
            plugins_failed: false,
            player: None,
        }
    }
}

/// Sample lyrics (original) with the third line being sung.
fn sample_lyrics(scroll: f32) -> LyricsView {
    LyricsView {
        lines: [
            "The evening sky fades slowly".into(),
            "You smile at the end of the road".into(),
            "And every song feels like it belongs to the two of us tonight".into(),
            "Don't go yet, wait a little while".into(),
            "Even the stars sing along".into(),
        ],
        scroll,
        status: None,
    }
}

/// Sample calendar: Tuesday 20 October 2026 selected and today, three events.
fn sample_calendar() -> CalendarView {
    let names = ["Sun", "Mon", "Tue", "Wed", "Thu"];
    CalendarView {
        month: "Oct".into(),
        year: "2026".into(),
        days: (0..5)
            .map(|i| DayView {
                offset: i - 2,
                name: names[i as usize],
                number: 18 + i as u32,
                today: i == 2,
                selected: i == 2,
                has_events: i == 2 || i == 4,
            })
            .collect(),
        events: vec![
            CalendarEntry { id: Some(1), time: String::new(), text: "Mom's birthday".into(), done: false, source: None, now: false },
            CalendarEntry {
                id: Some(2),
                time: "09.00".into(),
                text: "Team meeting".into(),
                done: false,
                source: None,
                now: false,
            },
            CalendarEntry {
                id: None,
                time: "10.30".into(),
                text: "Design review".into(),
                done: false,
                source: Some("Work".into()),
                now: true,
            },
            CalendarEntry {
                id: Some(3),
                time: "15.30".into(),
                text: "Buy a gift".into(),
                done: true,
                source: None,
                now: false,
            },
        ],
        scroll: 0,
    }
}

/// Render several states of the shape to `target/preview/*.bmp` so the look can be checked
/// without running the app. Run with `cargo test`.
#[test]
fn preview_states() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("preview");
    std::fs::create_dir_all(&dir).unwrap();
    let mut data = sample();
    let volume = Hud::Volume { level: 0.62, muted: false };
    let bluetooth = Hud::Bluetooth {
        id: String::new(),
        name: "Baseus Bowie WM01".into(),
        connected: true,
        battery: Some(80),
        kind: DeviceKind::Audio,
    };
    let water_hud = Hud::Water { ml: 1250, goal: 2000 };
    let camera = Hud::Privacy { camera: true, app: "Zoom".into() };
    let lyric = "The evening sky fades at the end of the road";
    let long_lyric = "You said I was everything to you, but in the end you left without a single word for me";
    let brightness = Hud::Brightness { level: 0.7 };
    let caps = Hud::LockKey { caps: true, on: true };
    let num = Hud::LockKey { caps: false, on: false };
    let tako_hud = Hud::Tako {
        name: "Ana Putri".into(),
        value: "Rp 25.000".into(),
        line: "Semangat streamnya kak, sehat selalu!".into(),
        initials: "AP".into(),
        url: String::new(),
    };
    let tako_media = Hud::Tako {
        name: "Budi".into(),
        value: "Rp 50.000".into(),
        line: "YouTube · Lagu favorit buat nemenin main".into(),
        initials: "B".into(),
        url: "https://www.youtube.com/watch?v=mm0YkIJZJ4w".into(),
    };
    let message = Hud::Message {
        app: "WhatsApp".into(),
        aumid: "sample!App".into(),
        sender: "Mom".into(),
        text: "What time are you coming home? I'm cooking your favorite soup".into(),
        browser: false,
    };
    let screenshot = Hud::Screenshot { id: 2, name: "Screenshot 2026-10-19 101112.png".into(), more: 0 };
    let download = Hud::Download { path: PathBuf::from("C:\\sample\\report.pdf"), name: "Financial Report Q3 2026.pdf".into(), more: 2 };
    let wifi = Hud::Wifi { name: "Home 5G".into(), connected: true, internet: true };
    let wifi_limited = Hud::Wifi { name: "Sunset Cafe".into(), connected: true, internet: false };
    let mood = |mood| Eyes { mood, ..Eyes::default() };
    let mut few_pages = Pages::default();
    few_pages.set_shown(Page::Water, false);
    few_pages.set_shown(Page::System, false);
    few_pages.move_by(Page::Tasks, -1);
    let timer_done = Hud::Timer { minutes: 5 };
    let used_up = vec![
        Limits {
            kind: AgentKind::Claude,
            meters: vec![
                Meter { window: "5h".into(), used: 100, resets_at: SAMPLE_NOW + 72 * 60 },
                Meter { window: "Week".into(), used: 64, resets_at: SAMPLE_NOW + 2 * 86_400 },
            ],
        },
        data.limits[0].clone(),
    ];
    let video = PlayerView { name: "Budi Santoso".into(), line: "YouTube · Lagu untuk kamu yang sedang belajar".into(), ready: false };
    let video_ready = PlayerView { ready: true, ..video.clone() };
    let agent_waiting = Hud::Agent {
        kind: AgentKind::Claude,
        title: "Billing API backfill".into(),
        project: "billing-api".into(),
        detail: "permission prompt".into(),
        needs_input: true,
        pid: 0,
    };
    let agent_done = Hud::Agent {
        kind: AgentKind::Codex,
        title: "Migrate auth to passkeys".into(),
        project: "accounts".into(),
        detail: String::new(),
        needs_input: false,
        pid: 0,
    };
    let running = TimerView {
        running: true,
        active: true,
        clock: "03:42".into(),
        short: "4m".into(),
        progress: 0.26,
        total_minutes: 5,
    };
    let muted_controls = ControlsView {
        volume: Some((0.3, true)),
        brightness: None,
        mic: Some(true),
        quiet_minutes: Some(42),
        city: None,
    };
    let targeted = ShelfFx { targeted: true, dash: 1.3, ..ShelfFx::default() };
    let sharing = ShelfFx { share_targeted: true, dash: 2.2, ..ShelfFx::default() };
    let waiting = ShelfFx { targeted: false, dash: 3.1, ..ShelfFx::default() };
    let dropped = ShelfFx {
        glow: Some(0.3),
        tiles: [(3u64, (0.8f32, 0.7f32)), (1, (0.75, 0.5))].into_iter().collect(),
        ..ShelfFx::default()
    };
    let no_fx = ShelfFx::default();
    let roll = Roll { from: "61".into(), t: 0.45, up: true };
    let charging = Hud::Power { plugged: true, percent: Some(80), low: false };
    let calendar = sample_calendar();
    let mut other_day = sample_calendar();
    other_day.days.iter_mut().for_each(|d| d.selected = d.offset == 2);
    other_day.events.clear();
    let lyrics = sample_lyrics(1.0);
    let lyrics_scrolling = sample_lyrics(0.4);
    let searching = LyricsView { lines: Default::default(), scroll: 1.0, status: Some(LyricStatus::Searching) };
    let seeking = MediaView { seeking: true, progress: Some(0.62), elapsed: "1:46".into(), remaining: "-1:07".into(), ..sample().media };
    let agenda_due = Hud::Agenda { text: "Team meeting".into(), time: "09.00".into() };
    let prayer_soon = Hud::Prayer { name: "Asr", time: "14:51".into(), ahead: 10 };
    let prayer_now = Hud::Prayer { name: "Maghrib", time: "17:49".into(), ahead: 0 };
    let no_city = PrayerView { city: None, next: None, ..sample_prayer() };
    let message_locked = Hud::Message {
        app: String::new(),
        aumid: "sample!App".into(),
        sender: "WhatsApp".into(),
        text: "New message".into(),
        browser: false,
    };

    // The mascot in the pill is only visible while no focus session runs.
    let idle = FocusView { running: false, active: false, ..sample().focus };
    let states = [
        State::new("pill", View::Pill, Page::Home),
        State { media: true, ..State::new("pill_music", View::Pill, Page::Home) },
        State { media: true, lyric: Some(lyric), ..State::new("pill_lyric", View::LyricPill, Page::Home) },
        State::new("slim", View::Slim, Page::Home),
        State { hud: Some(&volume), ..State::new("hud_volume", View::Hud, Page::Home) },
        State { hud: Some(&bluetooth), ..State::new("hud_bluetooth", View::Card, Page::Home) },
        State { hud: Some(&water_hud), ..State::new("hud_water", View::Hud, Page::Home) },
        State { hud: Some(&camera), ..State::new("hud_camera", View::Hud, Page::Home) },
        State { hovered: Some(Hit::PageDot(Page::Tasks)), ..State::new("page_clock", View::Panel, Page::Home) },
        State { media: true, hovered: Some(Hit::Media(Button::PlayPause)), ..State::new("page_media", View::Panel, Page::Home) },
        State { media: true, lyrics_button: Some(false), hovered: Some(Hit::MediaSeek), ..State::new("player_seek_hover", View::Panel, Page::Home) },
        State { media: true, lyrics_button: Some(false), hovered: Some(Hit::LyricsToggle), ..State::new("player_lyrics_button", View::Panel, Page::Home) },
        State { media: true, lyrics_button: Some(true), lyrics: Some(&lyrics), lyric_mix: 1.0, ..State::new("player_lyrics", View::PanelWide, Page::Home) },
        State { media: true, lyrics_button: Some(true), lyrics: Some(&lyrics), lyric_mix: 0.5, size: Some((560.0, 176.0)), ..State::new("player_lyrics_growing", View::PanelWide, Page::Home) },
        State { media: true, lyrics_button: Some(true), lyrics: Some(&lyrics_scrolling), lyric_mix: 1.0, ..State::new("player_lyrics_scroll", View::PanelWide, Page::Home) },
        State { media: true, lyrics_button: Some(true), lyrics: Some(&searching), lyric_mix: 1.0, ..State::new("player_lyrics_searching", View::PanelWide, Page::Home) },
        State { media: true, lyrics_button: Some(true), calendar: Some(&calendar), lyrics: Some(&lyrics), lyric_mix: 1.0, weather: Some("29° Clear"), ..State::new("player_lyrics_calendar", View::PanelWide, Page::Home) },
        State { media: true, lyrics_button: Some(true), calendar: Some(&calendar), lyrics: Some(&lyrics), lyric_mix: 0.45, ..State::new("player_lyrics_swap", View::PanelWide, Page::Home) },
        State { theme: Theme::Frost, media: true, lyrics_button: Some(true), lyrics: Some(&lyrics), lyric_mix: 1.0, ..State::new("player_lyrics_frost", View::PanelWide, Page::Home) },
        State { theme: Theme::Paper, media: true, lyrics_button: Some(true), lyrics: Some(&lyrics), lyric_mix: 1.0, ..State::new("player_lyrics_paper", View::PanelWide, Page::Home) },
        State { media: true, battery: Some((64, true)), ..State::new("player_charging", View::Panel, Page::Home) },
        State { hovered: Some(Hit::ShelfItem(2)), ..State::new("page_shelf", View::Panel, Page::Shelf) },
        State { hovered: Some(Hit::FocusReset), ..State::new("page_focus", View::Panel, Page::Focus) },
        State { hovered: Some(Hit::WaterDrink), ..State::new("page_water", View::Panel, Page::Water) },
        State { hovered: Some(Hit::TaskToggle(0)), ..State::new("page_tasks", View::Panel, Page::Tasks) },
        State::new("page_system", View::Panel, Page::System),
        State { agents: true, ..State::new("page_agents", View::PanelTall, Page::Agents) },
        State { agents: true, hovered: Some(Hit::AgentRow(0)), ..State::new("page_agents_hover", View::PanelTall, Page::Agents) },
        State { agents: true, hovered: Some(Hit::AgentSwitch(0)), ..State::new("page_agents_open", View::PanelTall, Page::Agents) },
        State { agents: true, ..State::new("page_agents_waiting", View::PanelTall, Page::Agents) },
        State { agents: true, limits: true, ..State::new("page_agents_limits", View::PanelTall, Page::Agents) },
        State { agents: true, limits: true, hovered: Some(Hit::AgentLimits), ..State::new("page_agents_limits_hover", View::PanelTall, Page::Agents) },
        State { agents: true, limits: true, ..State::new("page_agents_used_up", View::PanelTall, Page::Agents) },
        State { limits: true, ..State::new("page_agents_empty_limits", View::PanelTall, Page::Agents) },
        State { limits: true, ..State::new("page_clock_glance_limit", View::Panel, Page::Home) },
        State { dpi: 144, ..State::new("page_tasks_150", View::Panel, Page::Tasks) },
        State { hovered: Some(Hit::Command(Command::ToggleWeather)), ..State::new("page_settings", View::PanelTall, Page::Settings) },
        State { theme: Theme::Paper, ..State::new("page_settings_paper", View::PanelTall, Page::Settings) },
        State { hovered: Some(Hit::Command(Command::ToggleTako)), ..State::new("page_settings_features", View::PanelTall, Page::Settings) },
        State::new("page_settings_prayer", View::PanelTall, Page::Settings),
        State { agents: true, ..State::new("page_settings_agents", View::PanelTall, Page::Settings) },
        State { hovered: Some(Hit::Command(Command::PrayerLead(15))), ..State::new("page_prayer", View::Panel, Page::Prayer) },
        State { theme: Theme::Paper, ..State::new("page_prayer_paper", View::Panel, Page::Prayer) },
        State { hovered: Some(Hit::CityInput), ..State::new("page_prayer_setup", View::Panel, Page::Prayer) },
        State { city_draft: Some("Yogya"), ..State::new("page_prayer_setup_typing", View::Panel, Page::Prayer) },
        State { hud: Some(&prayer_soon), ..State::new("hud_prayer_soon", View::Card, Page::Home) },
        State { hud: Some(&prayer_now), ..State::new("hud_prayer_now", View::Card, Page::Home) },
        State { agents: true, hovered: Some(Hit::PageDot(Page::Water)), ..State::new("page_clock_glance", View::Panel, Page::Home) },
        State { theme: Theme::Ocean, ..State::new("theme_ocean", View::Panel, Page::Focus) },
        State { theme: Theme::Ember, media: true, ..State::new("theme_ember", View::Panel, Page::Home) },
        State { theme: Theme::Paper, hovered: Some(Hit::TaskToggle(0)), ..State::new("theme_paper", View::Panel, Page::Tasks) },
        State { theme: Theme::Paper, ..State::new("theme_paper_pill", View::Pill, Page::Home) },
        State { media: true, lyric: Some(long_lyric), ..State::new("pill_lyric_long", View::LyricPill, Page::Home) },
        State { hud: Some(&brightness), ..State::new("hud_brightness", View::Hud, Page::Home) },
        State { hud: Some(&caps), ..State::new("hud_capslock", View::Hud, Page::Home) },
        State { hud: Some(&num), ..State::new("hud_numlock", View::Hud, Page::Home) },
        State { hud: Some(&message), ..State::new("hud_message", View::Card, Page::Home) },
        State { hud: Some(&tako_hud), ..State::new("hud_tako", View::Card, Page::Home) },
        State { hud: Some(&tako_media), ..State::new("hud_tako_media", View::Card, Page::Home) },
        State { hud: Some(&message_locked), locked: true, ..State::new("hud_message_locked", View::Card, Page::Home) },
        State { media: true, locked: true, ..State::new("page_locked", View::Panel, Page::Home) },
        State { theme: Theme::Frost, ..State::new("theme_frost_pill", View::Pill, Page::Home) },
        State { theme: Theme::Frost, media: true, ..State::new("theme_frost", View::Panel, Page::Home) },
        State { theme: Theme::Frost, hud: Some(&message), ..State::new("theme_frost_hud", View::Card, Page::Home) },
        State { eyes: mood(Mood::Happy), ..State::new("mood_happy", View::Pill, Page::Home) },
        State { eyes: mood(Mood::Sleepy), ..State::new("mood_sleepy", View::Pill, Page::Home) },
        State { eyes: mood(Mood::Surprised), ..State::new("mood_surprised", View::Pill, Page::Home) },
        State { eyes: Eyes { look: (1.0, 1.0), ..Eyes::default() }, ..State::new("mood_look", View::Pill, Page::Home) },
        State { eyes: mood(Mood::Happy), ..State::new("mood_happy_panel", View::Panel, Page::Home) },
        State { eyes: Eyes { look: (-1.0, 0.6), mood: Mood::Surprised, open: 1.0 }, ..State::new("mood_surprised_panel", View::Panel, Page::Home) },
        State { hud: Some(&screenshot), ..State::new("hud_screenshot", View::Card, Page::Home) },
        State { hud: Some(&download), ..State::new("hud_download", View::Card, Page::Home) },
        State { hud: Some(&wifi), ..State::new("hud_wifi", View::Card, Page::Home) },
        State { hud: Some(&wifi_limited), ..State::new("hud_wifi_limited", View::Card, Page::Home) },
        State { pages: few_pages, ..State::new("pages_custom", View::Panel, Page::Tasks) },
        State { dpi: 84, ..State::new("size_small", View::Pill, Page::Home) },
        State { dpi: 110, media: true, ..State::new("size_large", View::Panel, Page::Home) },
        State { hovered: Some(Hit::VolumeSlider), ..State::new("page_controls", View::Panel, Page::Controls) },
        State { city_draft: Some("Yogya"), ..State::new("page_controls_city", View::Panel, Page::Controls) },
        State::new("page_controls_muted", View::Panel, Page::Controls),
        State { hovered: Some(Hit::TimerPreset(5)), ..State::new("page_timer", View::Panel, Page::Timer) },
        State { hovered: Some(Hit::TimerMode(Page::Timer)), ..State::new("page_focus_modes", View::Panel, Page::Focus) },
        State::new("page_timer_idle", View::Panel, Page::Timer),
        State::new("pill_timer", View::Pill, Page::Home),
        State { hud: Some(&timer_done), ..State::new("hud_timer", View::Hud, Page::Home) },
        State { hud: Some(&agent_waiting), ..State::new("hud_agent_waiting", View::Card, Page::Home) },
        State { hud: Some(&agent_done), ..State::new("hud_agent_done", View::Card, Page::Home) },
        State { weather: Some("28° Overcast · Jakarta"), ..State::new("page_clock_weather", View::Panel, Page::Home) },
        State { size: Some((188.0, 34.0)), shadow: Some(0.6), ..State::new("pill_peek", View::Pill, Page::Home) },
        State { fx: Some(&waiting), ..State::new("drop_waiting", View::Panel, Page::Shelf) },
        State { fx: Some(&targeted), ..State::new("drop_targeted", View::Panel, Page::Shelf) },
        State { fx: Some(&dropped), ..State::new("shelf_dropped", View::Panel, Page::Shelf) },
        State { hud: Some(&volume), roll: Some(&roll), ..State::new("strip_volume", View::Strip, Page::Home) },
        State { hud: Some(&brightness), ..State::new("strip_brightness", View::Strip, Page::Home) },
        State { hud: Some(&caps), ..State::new("strip_capslock", View::Strip, Page::Home) },
        State { hud: Some(&charging), ..State::new("strip_power", View::Strip, Page::Home) },
        State { hud: Some(&volume), roll: Some(&roll), ..State::new("hud_volume_roll", View::Hud, Page::Home) },
        State { media: true, calendar: Some(&calendar), weather: Some("29° Clear"), ..State::new("home_wide_media", View::PanelWide, Page::Home) },
        State { calendar: Some(&calendar), hovered: Some(Hit::AgendaRow(2)), ..State::new("home_wide_clock", View::PanelWide, Page::Home) },
        State { calendar: Some(&other_day), agenda_draft: Some("09.00 Mee"), battery: Some((34, true)), ..State::new("home_wide_typing", View::PanelWide, Page::Home) },
        State { hud: Some(&agenda_due), ..State::new("hud_agenda", View::Card, Page::Home) },
        State { download: Some("12.4 MB"), size: Some((236.0, 30.0)), ..State::new("pill_download", View::Pill, Page::Home) },
        State { fx: Some(&sharing), ..State::new("drop_share", View::Panel, Page::Shelf) },
        State { media: true, lyrics_button: Some(false), ..State::new("player_seeking", View::Panel, Page::Home) },
        State { welcome: true, hovered: Some(Hit::WelcomeDone), ..State::new("welcome", View::Panel, Page::Home) },
        State { welcome: true, theme: Theme::Paper, ..State::new("welcome_paper", View::Panel, Page::Home) },
        State { hovered: Some(Hit::GoogleOpen), ..State::new("page_mail", View::Panel, Page::Mail) },
        State { hovered: Some(Hit::TakoRow(1)), ..State::new("page_tako", View::PanelTall, Page::Tako) },
        State { hovered: Some(Hit::TakoSection(TakoSection::Goal)), ..State::new("page_tako_leaderboard", View::PanelTall, Page::Tako) },
        State::new("page_tako_milestone", View::PanelTall, Page::Tako),
        State::new("page_tako_song_share", View::PanelTall, Page::Tako),
        State::new("page_tako_poll", View::PanelTall, Page::Tako),
        State::new("page_tako_timer", View::PanelTall, Page::Tako),
        State::new("page_tako_vip_queue", View::PanelTall, Page::Tako),
        State::new("page_tako_empty", View::PanelTall, Page::Tako),
        State { hovered: Some(Hit::TakoKey), ..State::new("page_tako_setup", View::PanelTall, Page::Tako) },
        State::new("page_settings_tako", View::PanelTall, Page::Settings),
        State { player: Some(&video), ..State::new("player_video_loading", View::Player, Page::Home) },
        State { player: Some(&video_ready), hovered: Some(Hit::PlayerClose), ..State::new("player_video", View::Player, Page::Home) },
        State { player: Some(&video), theme: Theme::Paper, ..State::new("player_video_paper", View::Player, Page::Home) },
        State { hovered: Some(Hit::GoogleDisconnect), ..State::new("page_mail_hover", View::Panel, Page::Mail) },
        State { mail_setup: true, ..State::new("page_mail_setup", View::Panel, Page::Mail) },
        State { mail_setup: true, mail_editing: Some(0), ..State::new("page_mail_setup_typing", View::Panel, Page::Mail) },
        State { hovered: Some(Hit::PluginRow(0)), ..State::new("page_plugins", View::Panel, Page::Plugins) },
        State { plugins_empty: true, ..State::new("page_plugins_empty", View::Panel, Page::Plugins) },
        State { plugins_failed: true, hovered: Some(Hit::PluginsFolder), ..State::new("page_plugins_failed", View::Panel, Page::Plugins) },
    ];
    for state in states {
        let mut renderer = Renderer::new(state.dpi, state.theme).unwrap();
        if state.media {
            renderer.set_art(Some(&gradient_art()));
        }
        // The second shelf tile gets a fake thumbnail, the rest use icons.
        renderer.set_thumbnail(&Thumbnail { id: 2, width: 64, height: 40, pixels: gradient(64, 40) });
        data.tasks.draft = (state.name == "page_tasks_150").then(|| "Prepare meeting notes".into());
        let (width, height) = match (state.view, state.lyric, state.size) {
            (_, _, Some(size)) => size,
            (View::LyricPill, Some(line), _) => renderer.lyric_size(line),
            _ => state.view.size(),
        };
        data.shelf.drop_hover = state.name.starts_with("drop");
        let fill = match state.hud {
            Some(Hud::Volume { level, .. } | Hud::Brightness { level }) => *level,
            _ => 0.0,
        };
        let mut alpha = [0.0; CONTENTS];
        if let Some(i) = state.view.content() {
            alpha[i] = 1.0;
        }
        let scene = Scene {
            width,
            height,
            alpha,
            layout: [(width, height); CONTENTS],
            eyes: state.eyes,
            shadow: state.shadow.unwrap_or(if state.view.is_panel() { 1.0 } else { 0.0 }),
            time: "19:21",
            date: "Tue, 20 Oct",
            greeting: "Good evening",
            battery: state.battery,
            media: state.media.then_some(if state.name == "player_seeking" { &seeking } else { &data.media }),
            lyric: state.lyric,
            lyrics: state.lyrics,
            lyric_mix: state.lyric_mix,
            lyrics_button: state.lyrics_button,
            hud: state.hud,
            hud_compact: state.view == View::Strip,
            hud_roll: state.roll,
            hud_fill: fill,
            page: state.page,
            pages: state.pages,
            page_from: None,
            shelf: &data.shelf,
            shelf_fx: state.fx.unwrap_or(&no_fx),
            tasks: &data.tasks,
            focus: if state.name.starts_with("mood") || state.name == "size_small" { &idle } else { &data.focus },
            water: &data.water,
            prayer: if state.name.starts_with("page_prayer_setup") { &no_city } else { &data.prayer },
            privacy: &data.privacy,
            system: Some(&data.system),
            uptime: "3 h 12 m",
            hovered: state.hovered,
            pressed: None,
            seconds: 1.3,
            hover: 1.0,
            locked: state.locked,
            timer: if state.name.contains("timer") && !state.name.ends_with("idle") { &running } else { &data.timer },
            controls: if state.name.ends_with("muted") { &muted_controls } else { &data.controls },
            weather: state.weather,
            city_draft: state.city_draft,
            calendar: state.calendar,
            agenda_draft: state.agenda_draft,
            download: state.download,
            agents: if state.agents { &data.agents } else { &[] },
            agent_list: &agent_list(if state.agents { &data.agents } else { &[] }, agent_filter(state.name)),
            agent_counts: agent_counts(if state.agents { &data.agents } else { &[] }),
            agent_filter: agent_filter(state.name),
            agent_open: state.name.ends_with("_open").then_some(0),
            agent_hooks: agents::HookStatus::default(),
            agent_limits: if !state.limits {
                &[]
            } else if state.name.ends_with("_used_up") {
                &used_up
            } else {
                &data.limits
            },
            unix: SAMPLE_NOW,
            mail: &if state.mail_setup {
                MailView {
                    account: None,
                    status: None,
                    rows: Vec::new(),
                    setup: true,
                    id: String::new(),
                    secret: String::new(),
                    editing: state.mail_editing,
                    connecting: false,
                }
            } else {
                data.mail.clone()
            },
            plugins: &if state.plugins_empty {
                PluginsView { rows: Vec::new(), problems: Vec::new(), count: 0, empty: true }
            } else if state.plugins_failed {
                PluginsView {
                    rows: data.plugins.rows[..1].to_vec(),
                    problems: vec![plugin_row("ci", "printed nothing", "ci.ps1", "")],
                    count: 2,
                    empty: false,
                }
            } else {
                data.plugins.clone()
            },
            tako: &tako_view(&data.tako, state.name),
            player: state.player,
            agent_scroll: 0,
            welcome: state.welcome,
            settings: Settings::default(),
            settings_section: settings_section(state.name),
        };
        renderer.draw(&scene).unwrap();
        let _ = unsafe { GdiFlush() };
        let (w, h) = (renderer.surface.width, renderer.surface.height);
        write_bmp(&dir.join(format!("{}.bmp", state.name)), w, h, renderer.surface.pixels());
        // With CREST_PREVIEW_RAW set, also keep the premultiplied BGRA pixels for docs images.
        if std::env::var_os("CREST_PREVIEW_RAW").is_some() {
            let bytes: Vec<u8> = renderer.surface.pixels().iter().flat_map(|px| px.to_le_bytes()).collect();
            std::fs::write(dir.join(format!("{}.raw", state.name)), bytes).unwrap();
        }
    }
}

fn scene_for<'a>(data: &'a Sample, view: View, page: Page, media: bool) -> Scene<'a> {
    let (width, height) = view.size();
    let mut alpha = [0.0; CONTENTS];
    if let Some(i) = view.content() {
        alpha[i] = 1.0;
    }
    Scene {
        width,
        height,
        alpha,
        layout: [(width, height); CONTENTS],
        eyes: Eyes::default(),
        shadow: 0.0,
        time: "19:21",
        date: "",
        greeting: "",
        battery: None,
        media: media.then_some(&data.media),
        lyric: None,
        lyrics: None,
        lyric_mix: 0.0,
        lyrics_button: None,
        hud: None,
        hud_compact: false,
        hud_roll: None,
        hud_fill: 0.0,
        page,
        pages: Settings::default().pages,
        page_from: None,
        shelf: &data.shelf,
        shelf_fx: &data.shelf_fx,
        tasks: &data.tasks,
        focus: &data.focus,
        water: &data.water,
        prayer: &data.prayer,
        privacy: &data.privacy,
        system: None,
        uptime: "",
        hovered: None,
        pressed: None,
        seconds: 0.0,
        hover: 1.0,
        locked: false,
        timer: &data.timer,
        controls: &data.controls,
        weather: None,
        city_draft: None,
        calendar: None,
        agenda_draft: None,
        download: None,
        agents: &data.agents,
        agent_list: &data.agents,
        agent_counts: agent_counts(&data.agents),
        agent_filter: AgentFilter::All,
        agent_open: None,
        agent_hooks: agents::HookStatus::default(),
        agent_limits: &data.limits,
        unix: SAMPLE_NOW,
        mail: &data.mail,
        plugins: &data.plugins,
        tako: &data.tako,
        player: None,
        agent_scroll: 0,
        welcome: false,
        settings: Settings::default(),
        settings_section: SettingsSection::default(),
    }
}

/// Redrawing only the eyes (blinking, glancing, changing mood) must match a full redraw exactly:
/// no part of the eyes is left outside their small box.
#[test]
fn eye_patch_covers_every_expression() {
    let data = sample();
    let mut variants = vec![Eyes::default()];
    for mood in [Mood::Normal, Mood::Sleepy, Mood::Happy, Mood::Surprised] {
        for look in [(-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0)] {
            variants.push(Eyes { open: 1.0, look, mood });
        }
    }
    // The eyes only show in the pill while no focus session runs.
    let idle = FocusView { running: false, ..sample().focus };
    for dpi in [96, 144] {
        let mut renderer = Renderer::new(dpi, Theme::Ink).unwrap();
        for eyes in &variants {
            let mut scene = scene_for(&data, View::Pill, Page::Home, false);
            scene.focus = &idle;
            scene.eyes = *eyes;
            renderer.draw(&scene).unwrap();
            let _ = unsafe { GdiFlush() };
            let full = renderer.surface.pixels().to_vec();

            scene.eyes = Eyes::default();
            renderer.draw(&scene).unwrap();
            renderer.draw_eyes(176.0, 30.0, *eyes).unwrap();
            let _ = unsafe { GdiFlush() };
            let patched = renderer.surface.pixels();
            let worst = full
                .iter()
                .zip(patched.iter())
                .map(|(a, b)| (0..4).map(|i| ((a >> (i * 8)) & 0xFF).abs_diff((b >> (i * 8)) & 0xFF)).max().unwrap_or(0))
                .max()
                .unwrap_or(0);
            assert!(worst <= 2, "eyes {eyes:?} at {dpi} DPI leave their box (diff {worst})");
        }
    }
}

/// The shadow only falls below the shape: beside it there must be no dark
/// band, since it looks like a box behind the shape.
#[test]
fn shadow_only_below_shape() {
    let data = sample();
    let mut renderer = Renderer::new(96, Theme::Ink).unwrap();
    let mut scene = scene_for(&data, View::Panel, Page::Home, false);
    scene.shadow = 1.0;
    renderer.draw(&scene).unwrap();
    let _ = unsafe { GdiFlush() };
    let width = renderer.surface.width as usize;
    let (w, h) = View::Panel.size();
    let x0 = (CANVAS.0 / 2.0 - w / 2.0) as usize;
    let pixels = renderer.surface.pixels();
    let alpha = |x: usize, y: usize| pixels[y * width + x] >> 24;
    for y in 2..(h as usize - 2) {
        for x in (x0 - 12)..(x0 - 1) {
            assert_eq!(alpha(x, y), 0, "pixel ({x}, {y}) beside the shape is not transparent");
        }
    }
    assert!(alpha((CANVAS.0 / 2.0) as usize, h as usize + 4) > 0, "shadow exists below the shape");

    // The Frost theme body is see-through: the shadow must not show inside the panel.
    let mut glass = Renderer::new(96, Theme::Frost).unwrap();
    let mut frame = |shadow: f32| {
        let mut scene = scene_for(&data, View::Panel, Page::Home, false);
        scene.shadow = shadow;
        glass.draw(&scene).unwrap();
        let _ = unsafe { GdiFlush() };
        glass.surface.pixels().to_vec()
    };
    let (plain, shaded) = (frame(0.0), frame(1.0));
    let inside = (0..h as usize).flat_map(|y| (0..width).map(move |x| y * width + x));
    assert!(inside.clone().all(|i| plain[i] == shaded[i]), "shadow visible behind the glass body");
    let below = h as usize * width + width / 2;
    assert_ne!(plain[below + 4 * width], shaded[below + 4 * width], "shadow exists below the glass shape");
}

/// The lyric pill widens with its line, then wraps to two lines if it still does not fit.
#[test]
fn lyric_pill_fits_line() {
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    let (w, h) = renderer.lyric_size("♪");
    assert!((LYRIC_MIN_W..LYRIC_MIN_W + 24.0).contains(&w) && h == View::LyricPill.size().1, "size {w}x{h}");
    let (w, h) = renderer.lyric_size("Every second I still miss you, oh-oh, until dawn");
    assert!(w > LYRIC_MIN_W && w <= LYRIC_MAX_W && w % 24.0 == 0.0, "width {w}");
    assert_eq!(h, View::LyricPill.size().1);
    let (w, h) = renderer.lyric_size(&"really very long ".repeat(8));
    assert_eq!((w, h), (LYRIC_MAX_W, LYRIC_TWO_LINES_H));
}

/// Click areas are recorded as drawn and can be found again.
#[test]
fn hits_follow_drawing() {
    let data = sample();
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    renderer.draw(&scene_for(&data, View::Panel, Page::Water, false)).unwrap();
    let width = View::Panel.size().0;
    let left = CANVAS.0 / 2.0 - width / 2.0 + MARGIN_FOR_TEST;
    assert_eq!(renderer.hit_at(left + 20.0, tokens::ROW_Y + tokens::CONTENT_SHIFT), Some(Hit::WaterDrink));
    // Walking the tab bar from the left, the first tab is Home and the current page has one too.
    let tabs: Vec<Hit> = (0..CANVAS.0 as usize)
        .filter_map(|x| renderer.hit_at(x as f32, tokens::TABS_Y))
        .fold(Vec::new(), |mut seen, hit| {
            if seen.last() != Some(&hit) {
                seen.push(hit);
            }
            seen
        });
    assert_eq!(tabs.first(), Some(&Hit::PageDot(Page::Home)));
    assert!(tabs.contains(&Hit::PageDot(Page::Water)) && tabs.len() == Settings::default().pages.visible().count() + 1, "{tabs:?}");
    assert_eq!(tabs.last(), Some(&Hit::PageDot(Page::Settings)), "the settings gear ends the tab bar");
    assert_eq!(renderer.hit_at(2.0, 2.0), None);
}

const MARGIN_FOR_TEST: f32 = tokens::INSET;


/// The thin line shown while a window is maximized keeps the privacy dot while the mic is in use.
#[test]
fn slim_line_keeps_privacy_dot() {
    let data = sample();
    let probe = |privacy: &Usage| {
        let mut renderer = Renderer::new(96, Theme::Ink).unwrap();
        let mut scene = scene_for(&data, View::Slim, Page::Home, false);
        scene.privacy = privacy;
        renderer.draw(&scene).unwrap();
        let _ = unsafe { GdiFlush() };
        let width = renderer.surface.width as usize;
        let (w, h) = View::Slim.size();
        let x = (CANVAS.0 / 2.0 + w / 2.0 - 8.0) as usize;
        renderer.surface.pixels()[(h / 2.0) as usize * width + x]
    };
    let red = |px: u32| (px >> 16) & 0xFF;
    // Sample: Discord is using the microphone, so the dot is orange.
    assert!(red(probe(&data.privacy)) > 150, "privacy dot not visible");
    // The plain line is the theme body, not a fixed colour: read it from the palette.
    let body = palette(Theme::Ink).body;
    assert_eq!(
        red(probe(&Usage::default())),
        (body.r * 255.0).round() as u32,
        "without camera/mic the line is plain"
    );
}

/// Rewrite `packaging/crest.ico` from the tray icon mascot (the crest.exe icon in the Start
/// menu, Explorer, and Settings > Apps). Run with
/// `cargo test write_app_icon -- --ignored` after changing the mascot.
/// Raw straight-alpha BGRA dumps of the app icon at the package logo sizes, written to
/// `target/logos/<name>.raw`; `packaging/logos.ps1` turns them into the PNGs under
/// `packaging/package/assets`.
#[test]
#[ignore]
fn write_package_logos() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("logos");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, size) in [("Square44x44Logo", 44), ("StoreLogo", 50), ("Square150x150Logo", 150)] {
        let mut surface = icon_surface(size).unwrap();
        let bytes: Vec<u8> = surface.pixels().iter().flat_map(|px| px.to_le_bytes()).collect();
        std::fs::write(dir.join(format!("{name}.raw")), bytes).unwrap();
    }
}

#[test]
#[ignore]
fn write_app_icon() {
    const SIZES: [i32; 8] = [16, 20, 24, 32, 40, 48, 64, 128];
    let images: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&size| {
            let mut surface = icon_surface(size).unwrap();
            let pixels = surface.pixels().to_vec();
            // Icon image: BITMAPINFOHEADER, BGRA pixels from the bottom row up, then the AND mask
            // (all zero; transparency comes from alpha).
            let mut image = Vec::new();
            for value in [40u32, size as u32, size as u32 * 2] {
                image.extend(value.to_le_bytes());
            }
            image.extend(1u16.to_le_bytes());
            image.extend(32u16.to_le_bytes());
            image.extend([0u8; 24]);
            for row in (0..size as usize).rev() {
                for px in &pixels[row * size as usize..(row + 1) * size as usize] {
                    image.extend(px.to_le_bytes());
                }
            }
            let mask_row = (size as usize).div_ceil(32) * 4;
            image.extend(vec![0u8; mask_row * size as usize]);
            image
        })
        .collect();
    let mut ico = Vec::new();
    ico.extend([0u16, 1, SIZES.len() as u16].iter().flat_map(|v| v.to_le_bytes()));
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (&size, image) in SIZES.iter().zip(&images) {
        ico.extend([size as u8, size as u8, 0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((image.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in &images {
        ico.extend(image);
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging").join("crest.ico");
    std::fs::write(&path, ico).unwrap();
}

/// The lyrics button, song bar, and play button are clickable where they appear; the song bar
/// turns the click position into a song position.
#[test]
fn player_hits() {
    let data = sample();
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    let width = View::Panel.size().0;
    let x0 = CANVAS.0 / 2.0 - width / 2.0;
    let (l, r) = (x0 + tokens::INSET, x0 + width - tokens::INSET);
    let mut scene = scene_for(&data, View::Panel, Page::Home, true);
    scene.lyrics_button = Some(false);
    renderer.draw(&scene).unwrap();
    // Page coordinates of the button row (`ROW_Y - 2`) and the song bar (108), plus the shift.
    let (buttons, bar) = (tokens::ROW_Y - 2.0 + tokens::CONTENT_SHIFT, 108.0 + tokens::CONTENT_SHIFT);
    assert_eq!(renderer.hit_at(l + 10.0, buttons), Some(Hit::LyricsToggle));
    assert_eq!(renderer.hit_at((l + r) / 2.0, buttons), Some(Hit::Media(Button::PlayPause)));
    assert_eq!(renderer.hit_at((l + r) / 2.0, bar), Some(Hit::MediaSeek));
    let start = renderer.slider_value(Hit::MediaSeek, l + 42.0).unwrap();
    let end = renderer.slider_value(Hit::MediaSeek, r - 48.0).unwrap();
    assert!(start < 0.01 && end > 0.99, "song bar {start}..{end}");

    // Lyrics feature off: no button. An app that cannot seek: the bar is not clickable.
    let fixed = MediaView { can_seek: false, ..sample().media };
    let mut scene = scene_for(&data, View::Panel, Page::Home, true);
    scene.media = Some(&fixed);
    renderer.draw(&scene).unwrap();
    assert_eq!(renderer.hit_at(l + 10.0, buttons), None);
    assert_eq!(renderer.hit_at((l + r) / 2.0, bar), None);
}

/// Lyrics replace the calendar: once fully slid in, the calendar is no longer clickable;
/// while sliding, the right column is not clickable at all.
#[test]
fn lyrics_replace_calendar() {
    let data = sample();
    let calendar = sample_calendar();
    let lyrics = sample_lyrics(1.0);
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    let width = View::PanelWide.size().0;
    let x1 = CANVAS.0 / 2.0 + width / 2.0;
    // Third day of the calendar strip (middle of the right column), and the first agenda row.
    let day = (x1 - MARGIN_FOR_TEST - 100.0, 63.0);
    let mut scene = scene_for(&data, View::PanelWide, Page::Home, true);
    scene.calendar = Some(&calendar);
    scene.lyrics = Some(&lyrics);
    for (mix, expected) in [(0.0, true), (0.5, false), (1.0, false)] {
        scene.lyric_mix = mix;
        renderer.draw(&scene).unwrap();
        let hit = renderer.hit_at(day.0, day.1);
        assert_eq!(matches!(hit, Some(Hit::CalendarDay(_))), expected, "slide {mix}: {hit:?}");
    }
}

/// The glow at the bottom of the panel follows the cover color, and is absent without music.
#[test]
fn glow_follows_album_colour() {
    let data = sample();
    let (_, h) = View::Panel.size();
    let probe = |media: bool| {
        let mut renderer = Renderer::new(96, Theme::Ink).unwrap();
        renderer.draw(&scene_for(&data, View::Panel, Page::Home, media)).unwrap();
        let _ = unsafe { GdiFlush() };
        let width = renderer.surface.width as usize;
        // Just above the bottom edge, left of center (not over buttons or page dots).
        let x = (CANVAS.0 / 2.0 - 120.0) as usize;
        renderer.surface.pixels()[(h as usize - 6) * width + x]
    };
    let channel = |px: u32, shift: u32| (px >> shift) & 0xFF;
    let (plain, glowing) = (probe(false), probe(true));
    // Read the body colour from the theme rather than pinning a literal, so a palette change
    // does not read as a broken glow.
    let body = palette(Theme::Ink).body;
    let expected =
        ((body.r * 255.0).round() as u32) << 16 | ((body.g * 255.0).round() as u32) << 8 | (body.b * 255.0).round() as u32;
    assert_eq!(plain & 0xFF_FFFF, expected, "without music the body keeps its plain color");
    // The sample cover is light blue (0x4FC3F7): blue strongest, red weakest.
    assert!(channel(glowing, 0) > 30 && channel(glowing, 0) > channel(glowing, 16), "glow {glowing:08X}");
}

/// The agenda × button must be reachable: a cursor moving from the circle across the row text
/// keeps that row hovered, so the × at the right end does not vanish before it is clicked.
#[test]
fn agenda_remove_reachable() {
    let data = sample();
    let calendar = sample_calendar();
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    let width = View::PanelWide.size().0;
    let x0 = CANVAS.0 / 2.0 - width / 2.0;
    // Calendar column: after Home's left column and its gap.
    let inner = width - 2.0 * tokens::INSET;
    let (l, r) = (x0 + tokens::INSET + tokens::home_left(inner) + 24.0, x0 + width - tokens::INSET);
    // First agenda row of the calendar column (see `calendar()`: rows start at 98), on the canvas.
    let y = 98.0 + tokens::CONTENT_SHIFT;
    let mut scene = scene_for(&data, View::PanelWide, Page::Home, false);
    scene.calendar = Some(&calendar);
    renderer.draw(&scene).unwrap();
    assert_eq!(renderer.hit_at(l + 6.0, y), Some(Hit::AgendaToggle(1)));
    assert_eq!(renderer.hit_at(l + 80.0, y), Some(Hit::AgendaRow(1)));
    assert_eq!(renderer.hit_at(r - 8.0, y), Some(Hit::AgendaRow(1)));
    assert_eq!(renderer.hit_at(r - 8.0, y + tokens::AGENDA_ROW_H), Some(Hit::AgendaRow(2)));
    // Row hovered: × replaces the time and is clickable, other rows are unaffected.
    scene.hovered = Some(Hit::AgendaRow(1));
    renderer.draw(&scene).unwrap();
    assert_eq!(renderer.hit_at(r - 8.0, y), Some(Hit::AgendaRemove(1)));
    assert_eq!(renderer.hit_at(l + 80.0, y), Some(Hit::AgendaRow(1)));
    assert_eq!(renderer.hit_at(r - 8.0, y + tokens::AGENDA_ROW_H), Some(Hit::AgendaRow(2)));
}

/// Agents page: session rows stay inside the panel without overlapping.
#[test]
fn agents_page_rows_fit() {
    let data = sample();
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    let (width, height) = View::PanelTall.size();
    let x0 = CANVAS.0 / 2.0 - width / 2.0;
    let (l, r) = (x0 + tokens::INSET, x0 + width - tokens::INSET);
    let mut scene = scene_for(&data, View::PanelTall, Page::Agents, false);
    renderer.draw(&scene).unwrap();
    // The rows sit right of the limits column, under the filter keys, 40 DIP each, and every
    // visible row ends inside the deck.
    let cl = l + 150.0 + 24.0;
    let list_top = tokens::CONTENT_SHIFT + tokens::DECK_TOP + 16.0 + 10.0 + 18.0;
    for row in 0..crate::feeds::agents::VISIBLE {
        let cy = list_top + 20.0 + 40.0 * row as f32;
        assert_eq!(renderer.hit_at(cl + 60.0, cy), Some(Hit::AgentRow(row)), "row {row}");
    }
    let rows_end = list_top + 40.0 * crate::feeds::agents::VISIBLE as f32;
    assert!(rows_end <= height - tokens::DECK_INSET - 6.0, "rows ({rows_end}) run out of the deck");
    assert_eq!(renderer.hit_at(cl + 4.0, list_top - 18.0), Some(Hit::AgentFilter(AgentFilter::All)));

    // The limits column on the left, the hooks link at its foot, and Refresh in the caption.
    let first_meter = list_top - 18.0 + 22.0 + 18.0;
    assert_eq!(renderer.hit_at(l + 20.0, first_meter), Some(Hit::AgentLimits));
    let foot = height - tokens::DECK_INSET - 16.0;
    assert_eq!(renderer.hit_at(l + 10.0, foot), Some(Hit::AgentHooks));
    assert_eq!(renderer.hit_at(r - 8.0, tokens::TABS_Y), Some(Hit::AgentsRefresh));

    // An open row shows its actions; Switch for a session with a window.
    scene.agent_open = Some(0);
    renderer.draw(&scene).unwrap();
    let actions = list_top + 40.0 + 17.0 - 3.0;
    assert_eq!(renderer.hit_at(r - 10.0, actions), Some(Hit::AgentSwitch(0)));

    // Without sessions the limits and the hooks link stay.
    scene.agents = &[];
    scene.agent_list = &[];
    scene.agent_open = None;
    renderer.draw(&scene).unwrap();
    assert_eq!(renderer.hit_at(l + 20.0, first_meter), Some(Hit::AgentLimits));
    assert_eq!(renderer.hit_at(l + 10.0, foot), Some(Hit::AgentHooks));
}

/// Compare the cost of a full frame with redrawing only the wave.
/// Run with `cargo test --release -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_wave() {
    let data = sample();
    let mut renderer = Renderer::new(96, Theme::Ink).unwrap();
    renderer.set_art(Some(&gradient_art()));
    let scene = scene_for(&data, View::Pill, Page::Home, true);
    let runs = 500;
    let start = std::time::Instant::now();
    for _ in 0..runs {
        renderer.draw(&scene).unwrap();
    }
    let full = start.elapsed() / runs;
    let start = std::time::Instant::now();
    for i in 0..runs {
        let (w, h) = View::Pill.size();
        renderer.draw_wave(w, h, i as f64 * 0.08, None).unwrap();
    }
    let wave = start.elapsed() / runs;
    println!("full frame: {full:?}, wave only: {wave:?}");
}

fn gradient(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[230, (x * 255 / width) as u8, (y * 255 / height) as u8, 255]);
        }
    }
    pixels
}

/// Fake cover: blue to purple gradient.
fn gradient_art() -> Art {
    Art { id: 1, pixels: gradient(ART_PX, ART_PX), accent: Some(0x4FC3F7) }
}

/// 32-bit BMP; premultiplied pixels composited over a gray background so the black shape shows.
fn write_bmp(path: &Path, w: i32, h: i32, pixels: &[u32]) {
    const BG: u32 = 0xB4;
    let mut data = Vec::with_capacity(54 + pixels.len() * 4);
    data.extend_from_slice(b"BM");
    data.extend_from_slice(&(54 + pixels.len() as u32 * 4).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&54u32.to_le_bytes());
    data.extend_from_slice(&40u32.to_le_bytes());
    data.extend_from_slice(&w.to_le_bytes());
    data.extend_from_slice(&(-h).to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&32u16.to_le_bytes());
    data.extend_from_slice(&[0u8; 24]);
    for &px in pixels {
        let a = px >> 24;
        let over = |c: u32| (c + BG * (255 - a) / 255).min(255) as u8;
        data.extend_from_slice(&[over(px & 0xFF), over((px >> 8) & 0xFF), over((px >> 16) & 0xFF), 255]);
    }
    std::fs::write(path, data).unwrap();
}

/// Every clickable element on the Mail and Plugins pages lands where it is drawn: inside the
/// panel, above the bottom edge, and not on top of the neighbouring element.
#[test]
fn new_pages_fit_and_hit_where_drawn() {
    let data = sample();
    // Page content starts under the tab row and must end inside the 176 DIP panel.
    fn bottom_of(data: &Sample, page: Page, hit: Hit) -> Option<f32> {
        let renderer = Renderer::new(96, Theme::Ink).unwrap();
        renderer.draw(&scene_for(data, View::Panel, page, false)).unwrap();
        renderer.hit_rect(hit).map(|r| r.bottom)
    }
    let open = bottom_of(&data, Page::Mail, Hit::GoogleOpen).expect("the Gmail link is clickable");
    assert!(open <= View::Panel.size().1, "the Gmail link ends at {open}, past the panel");
    // The setup boxes and the Connect button must not overlap, and the button must fit.
    let mut setup = scene_for(&data, View::Panel, Page::Mail, false);
    let box_data = MailView {
        account: None,
        status: None,
        rows: Vec::new(),
        setup: true,
        id: String::new(),
        secret: String::new(),
        editing: None,
        connecting: false,
    };
    setup.mail = &box_data;
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    renderer.draw(&setup).unwrap();
    let id = renderer.hit_rect(Hit::GoogleClient(0)).expect("client id box");
    let secret = renderer.hit_rect(Hit::GoogleClient(1)).expect("client secret box");
    // Connect only works once both boxes hold something valid; empty, it is drawn but dimmed.
    assert!(renderer.hit_rect(Hit::GoogleConnect).is_none(), "Connect works with empty boxes");
    assert!(renderer.hit_rect(Hit::Paste(PasteTarget::GoogleId)).is_some(), "an empty box offers Paste");
    let filled = MailView { id: "247995060423-test.apps.googleusercontent.com".into(), secret: "GOCSPX-sample-secret".into(), ..box_data.clone() };
    setup.mail = &filled;
    renderer.draw(&setup).unwrap();
    let connect = renderer.hit_rect(Hit::GoogleConnect).expect("connect button");
    assert!(id.bottom < secret.top, "the boxes overlap");
    assert!(connect.bottom <= id.top, "the button runs into the boxes");
    let deck = tokens::DECK_BOTTOM + tokens::CONTENT_SHIFT;
    assert!(secret.bottom <= deck, "the secret box leaves the deck");

    // Plugin rows: three fit, and the last one clears the folder link.
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    renderer.draw(&scene_for(&data, View::Panel, Page::Plugins, false)).unwrap();
    let last = renderer.hit_rect(Hit::PluginRow(1)).expect("second plugin row");
    let folder = renderer.hit_rect(Hit::PluginsFolder).expect("folder link");
    assert!(last.bottom <= folder.top, "the third row runs into the folder link");
}

/// The Prayer page's controls and the taller Settings page's last row land inside their deck,
/// and the reminder choices do not run into the method button.
#[test]
fn prayer_and_settings_fit_their_panels() {
    let data = sample();
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    renderer.draw(&scene_for(&data, View::Panel, Page::Prayer, false)).unwrap();
    let last_lead = renderer.hit_rect(Hit::Command(Command::PrayerLead(30))).expect("30 min choice");
    let method = renderer.hit_rect(Hit::Command(Command::PrayerMethod(crate::tools::prayer::Method::Mwl))).expect("method button");
    assert!(last_lead.right < method.left, "the reminder choices overlap the method button");
    assert!(method.bottom <= tokens::DECK_BOTTOM + tokens::CONTENT_SHIFT, "the method button leaves the deck");

    // Every Settings section: its last control ends inside the deck (with the page padding),
    // to the right of the sidebar, and inside the panel's right padding.
    let (width, height) = View::PanelTall.size();
    let deck = tokens::DECK_BOTTOM + height - View::Panel.size().1 + tokens::CONTENT_SHIFT;
    let last = [
        (SettingsSection::Look, Hit::Command(Command::ToggleCompactHud)),
        (SettingsSection::Features, Hit::Command(Command::ToggleScreenshots)),
        (SettingsSection::Prayer, Hit::Command(Command::PrayerLead(30))),
        (SettingsSection::Agents, Hit::AgentHook(crate::feeds::agents::Provider::Copilot)),
    ];
    for (section, hit) in last {
        let mut scene = scene_for(&data, View::PanelTall, Page::Settings, false);
        scene.settings_section = section;
        let renderer = Renderer::new(96, Theme::Ink).unwrap();
        renderer.draw(&scene).unwrap();
        let area = renderer.hit_rect(hit).unwrap_or_else(|| panic!("{section:?}: last control"));
        assert!(area.bottom <= deck - 12.0, "{section:?}: the last row ends at {}, past the deck at {deck}", area.bottom);
        // The sidebar key starts 6 DIP before the content edge; rows may bleed 6 DIP past the other.
        let sidebar = renderer.hit_rect(Hit::SettingsSection(section)).expect("sidebar key");
        assert!(sidebar.right < area.left, "{section:?}: a control runs under the sidebar");
        assert!(area.right - sidebar.left <= width - 2.0 * tokens::INSET + 12.0, "{section:?}: a control leaves the panel");
    }
}

/// A saved client id with no secret yet must still show both boxes: the secret could otherwise
/// never be filled in, and the page would offer a Connect button that cannot work.
#[test]
fn mail_setup_shows_both_boxes_until_connected() {
    let data = sample();
    let waiting = MailView {
        account: None,
        status: Some("Google did not send you back".into()),
        rows: Vec::new(),
        setup: true,
        id: "247995060423-test.apps.googleusercontent.com".into(),
        secret: "GOCSPX-sample-secret".into(),
        editing: None,
        connecting: false,
    };
    let mut scene = scene_for(&data, View::Panel, Page::Mail, false);
    scene.mail = &waiting;
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    renderer.draw(&scene).unwrap();
    let id = renderer.hit_rect(Hit::GoogleClient(0)).expect("the client id box is drawn");
    let secret = renderer.hit_rect(Hit::GoogleClient(1)).expect("the secret box is drawn");
    let button = renderer.hit_rect(Hit::GoogleConnect).expect("a Connect button is drawn");
    assert!(id.bottom < secret.top && button.bottom <= id.top, "the rows overlap");
    assert!(secret.bottom <= tokens::DECK_BOTTOM + tokens::CONTENT_SHIFT, "the boxes leave the deck");

    // While waiting for the browser, the same spot must cancel instead of starting a second try.
    let connecting = MailView { connecting: true, ..waiting.clone() };
    scene.mail = &connecting;
    renderer.draw(&scene).unwrap();
    let cancel = renderer.hit_rect(Hit::GoogleConnect).expect("the waiting button is clickable");
    assert_eq!(
        renderer.hit_at((cancel.left + cancel.right) / 2.0, (cancel.top + cancel.bottom) / 2.0),
        Some(Hit::GoogleConnect)
    );
}

/// Text drawn on a primary or status fill must stay readable in every theme. The dark themes use
/// light pastel accents, where white text fails badly (1.7–3.3:1) and ink passes (6–12:1), so
/// this pins the rule instead of leaving it to whoever edits the palette next.
#[test]
fn labels_stay_readable_on_every_fill() {
    fn luminance(c: D2D1_COLOR_F) -> f32 {
        let channel = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    }
    fn contrast(a: D2D1_COLOR_F, b: D2D1_COLOR_F) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        let (hi, lo) = (x.max(y), x.min(y));
        (hi + 0.05) / (lo + 0.05)
    }
    // 4.5:1 is WCAG AA for normal text; these labels are 12–14 DIP semibold.
    for theme in Theme::ALL {
        let p = palette(theme);
        // The primary fill takes `on_accent`, which is ink on the dark themes and white on Paper.
        let ratio = contrast(p.on_accent, p.accent);
        assert!(ratio >= 4.5, "{theme:?}: label on the primary fill is only {ratio:.2}:1");
        // Status fills are light in every theme, so they always take ink.
        for (what, fill) in [("green", GREEN), ("amber", ORANGE), ("red", RED), ("blue", BLUE)] {
            let ratio = contrast(ON_STATUS, fill);
            assert!(ratio >= 4.5, "{theme:?}: label on {what} is only {ratio:.2}:1");
        }
        // Body text on the body fill, the other pairing every page relies on.
        let ratio = contrast(p.text, p.body);
        assert!(ratio >= 7.0, "{theme:?}: body text is only {ratio:.2}:1");
    }
}

/// An empty state keeps its title and hint inside the area it is given, and a badge stays on one
/// line: both are drawn on pages a user sees with no content, where clipping is obvious.
#[test]
fn empty_state_and_badge_stay_inside_their_areas() {
    let renderer = Renderer::new(96, Theme::Ink).unwrap();
    // The empty state is centred in its box; a title wider than the box must still be laid out
    // inside it rather than escaping into the neighbouring column.
    let area = rect(40.0, 40.0, 240.0, 100.0);
    let title = "No plugins yet";
    let width = renderer.text_width(title, &renderer.fonts.label);
    assert!(width <= area.right - area.left, "title ({width}) fits its 200 DIP box");
    // A badge grows with its text, so a long count must not run past the panel edge.
    let badge = renderer.text_width("99", &renderer.fonts.tiny) + 12.0;
    assert!(badge < 40.0, "a two-digit badge is compact ({badge})");
    // The page guarantees, in the same units the renderer uses.
    assert!(tokens::ROW_Y + tokens::CONTROL_H / 2.0 <= View::Panel.size().1);
}

/// The tab bar must measure the same whichever page is current. It used to reserve space for
/// the current page's name and for that page's caption, so the row widened and narrowed as the
/// user moved between pages ("Nothing unread" on Mail vs "Next reminder 19:37" on Water).
#[test]
fn tab_bar_width_does_not_depend_on_the_current_page() {
    let data = sample();
    let width = View::Panel.size().0;
    let left = CANVAS.0 / 2.0 - width / 2.0;
    let right_edge = |page: Page| {
        let renderer = Renderer::new(96, Theme::Ink).unwrap();
        let mut scene = scene_for(&data, View::Panel, page, false);
        scene.page = page;
        renderer.draw(&scene).unwrap();
        renderer.hit_rect(Hit::PageDot(Page::Settings)).unwrap_or_else(|| panic!("settings tab missing on {page:?}")).right
    };
    let pages = [Page::Home, Page::Controls, Page::Agents, Page::Mail, Page::Plugins];
    let widths: Vec<f32> = pages.iter().map(|&page| right_edge(page)).collect();
    for (&page, &edge) in pages.iter().zip(&widths) {
        assert!(edge - left <= width, "{page:?}: the tab bar runs past the panel");
        assert!((edge - widths[0]).abs() < 0.5, "{page:?}: the tab bar ends at {edge}, but Home ends at {}", widths[0]);
    }
}

