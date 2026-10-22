//! The Crest window: state, mouse/keyboard input, timers, and the message loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, Sleep};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MOD_ALT, MOD_NOREPEAT, MOD_WIN, RegisterHotKey, ReleaseCapture, SetCapture, SetFocus,
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, UnregisterHotKey, VK_CAPITAL, VK_NUMLOCK,
};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK,
    RIDEV_REMOVE, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetWindowThreadProcessId,
    WM_HOTKEY,
    GWL_EXSTYLE, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowLongPtrW,
    HWND_TOPMOST, IDC_ARROW, IDC_HAND, IsWindow, KillTimer, LoadCursorW, MA_NOACTIVATE, MSG,
    PBT_APMRESUMEAUTOMATIC, PBT_POWERSETTINGCHANGE, PM_REMOVE, PeekMessageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SM_CXDRAG, SM_CXSMICON, SM_CYDRAG,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursor,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    RI_KEY_BREAK, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CHAR, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_INPUT,
    WM_LBUTTONDBLCLK, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_POWERBROADCAST, WM_QUIT, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMECHANGE, WM_TIMER, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, PCWSTR, Result, w};

use crate::feeds::agents;
use crate::feeds::google::{self, Mail};
use crate::feeds::plugins::{self, Plugins};
use crate::ui::motion::Spring;
use crate::system::desktop::{self, Mode, Watcher};
use crate::system::dnd::{self, DRAG_DROP, DRAG_ENTER, DRAG_LEAVE, DRAG_OVER, DropRegistration};
use crate::feeds::lyrics::{self, Lyrics};
use crate::feeds::media::{self, Snapshot};
use crate::system::power::{self, Power};
use crate::system::privacy::Usage;
use crate::ui::{
    self, Button, CONTENTS, CalendarEntry, CalendarView, ControlsView, DayView, Eyes, FocusView, Hit, Hud, LyricsView,
    MailRow, MailView, MediaView, Mood, PluginsView, PrayerTime, PrayerView, Renderer, Roll, Scene, ShelfFx, TimerView, View, WaterView, hud_value,
};
use crate::system::services::{self, Event, Services, Task};
use crate::shell::settings::{Messages, Settings, Size};
use crate::system::sound::{self, Sound};
use crate::system::watch::{self, Watch};
use crate::{shell::identity, shell::install, feeds::messages, system::network, system::share};
use crate::system::stats::Stats;
use crate::shell::tray::{self, Command, MenuState, PageCommand, PageMenu, Tray};
use crate::tools::Page;
use crate::tools::focus::{self, Focus};
use crate::tools::shelf::{self, Shelf};
use crate::tools::tasks::{self, Tasks};
use crate::tools::agenda::{self, Agenda, Date};
use crate::tools::calfeed;
use crate::tools::feeds::{self as calfeeds, Feeds};
use crate::tools::timer::{self, Timer};
use crate::tools::water::{GOAL_ML, Water};
use crate::feeds::weather::{self, City, Weather};
use crate::{shell::autostart, system::clock};

mod config;
mod clock_text;
mod commands;
mod events;
mod files;
mod frame;
mod hud;
mod input;
mod integrations;
mod mascot;
mod playback;
mod prayer;
mod schedule;
mod views;

use config::*;


thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Animation frame count; written to the debug log every minute to monitor load.
static FRAMES: AtomicU32 = AtomicU32::new(0);

pub fn run() -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(Error::from_thread());
        }
        // Layered: free-form shape + transparency. Tool window: not on the taskbar/Alt-Tab.
        // No-activate: clicking the panel does not steal focus from the app in use.
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("Crest"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        let mut app = App::new(hwnd)?;
        app.mode = desktop::current_mode(hwnd, app.monitor, &app.monitor_rect);
        app.apply_visibility();
        app.wake_up();
        app.update_prayer();
        app.refresh();
        app.schedule_clock();
        app.set_timer(TIMER_POLL, POLL_MS);
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        // Service events that arrived before the state was ready have not been processed yet.
        let _ = PostMessageW(Some(hwnd), services::WM_SERVICE, WPARAM(0), LPARAM(0));

        let mut msg = MSG::default();
        'pump: loop {
            if with_app(|app| app.animating()).unwrap_or(false) {
                // While animating: drain all messages, draw one frame, wait for the next screen refresh.
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'pump;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                with_app(|app| app.tick());
                if DwmFlush().is_err() {
                    Sleep(7);
                }
            } else {
                // While idle: sleep until a message arrives. This is what keeps CPU at 0%.
                if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Dropping App removes the tray icon and hooks and stops services before the window is destroyed.
        APP.with(|cell| cell.borrow_mut().take());
        let _ = DestroyWindow(hwnd);
        Ok(())
    }
}

/// Access the app state. Returns `None` if the state is already borrowed, e.g. when
/// Windows sends a nested message in the middle of an API call.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // The tray menu and drag-out have their own message loops, so they run outside the app state.
    if msg == tray::WM_TRAY {
        let event = (lparam.0 & 0xFFFF) as u32;
        if (event == WM_LBUTTONUP || event == WM_RBUTTONUP)
            && let Some(state) = with_app(|app| app.menu_state())
            && let Some(command) = tray::show_menu(hwnd, state)
        {
            with_app(|app| app.on_command(command));
        }
        return LRESULT(0);
    }
    if msg == WM_START_DRAG {
        if let Some(path) = with_app(|app| app.drag_path()).flatten() {
            let _ = dnd::drag_out(hwnd, &path);
            with_app(|app| app.after_drag());
        }
        return LRESULT(0);
    }
    if msg == WM_RBUTTONUP
        && let Some(menu) = with_app(|app| app.page_menu(lparam)).flatten()
    {
        let command = tray::show_page_menu(hwnd, &menu);
        with_app(|app| app.after_page_menu(menu.page, command));
        return LRESULT(0);
    }
    match with_app(|app| app.handle(msg, wparam, lparam)).flatten() {
        Some(result) => result,
        None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

struct App {
    hwnd: HWND,
    renderer: Renderer,
    tray: Tray,
    // Kept only so the hooks and registrations stay alive until the app exits.
    _watcher: Watcher,
    _power: Power,
    _drop: Option<DropRegistration>,
    services: Services,
    monitor: HMONITOR,
    monitor_rect: RECT,
    /// The shape being animated toward.
    view: View,
    width: Spring,
    height: Spring,
    /// Size when the shape transition started, to measure how far it has progressed.
    from: (f32, f32),
    /// Opacity of each content; see [`View::content`].
    alpha: [f32; CONTENTS],
    page: Page,
    /// Page transition in progress: source page, start time, direction.
    page_anim: Option<(Page, f64, f32)>,
    blink_since: Option<f64>,
    expanded: bool,
    hovering: bool,
    tracking_leave: bool,
    hovered: Option<Hit>,
    mode: Mode,
    user_hidden: bool,
    shown: bool,
    media: Option<Snapshot>,
    /// Last HUD content; kept while the HUD fades out.
    hud: Option<Hud>,
    hud_visible: bool,
    plugged: Option<bool>,
    battery: Option<u32>,
    shelf: Shelf,
    focus: Focus,
    water: Water,
    tasks: Tasks,
    settings: Settings,
    /// Apps currently using the camera/microphone; `privacy_known` is false until the first event.
    privacy: Usage,
    privacy_known: bool,
    /// Last fetched lyrics, and the key of the track whose lyrics are being fetched.
    lyrics: Option<Lyrics>,
    lyric_pending: Option<String>,
    /// Last drawn lyric line, to know when a redraw is needed.
    lyric_shown: Option<usize>,
    /// Right column of Home: 0 = calendar, 1 = lyrics (slides smoothly between them).
    lyric_mix: f32,
    /// The lyrics column started scrolling to the next line at this time.
    lyric_scroll: Option<f64>,
    /// Lyrics column line (-1 = before the first line) and track second last drawn.
    lyric_drawn: Option<isize>,
    second_drawn: Option<u64>,
    /// Position (0..1) of the track progress bar being dragged.
    seek_preview: Option<f32>,
    /// The shape is moving between two panel sizes (e.g. the lyrics column slides in): content stays visible.
    panel_resize: bool,
    system: Option<Stats>,
    /// The service thread has been asked to measure the system.
    system_watch: bool,
    /// Target shape size (the lyrics pill follows the lyric line's size).
    target: (f32, f32),
    /// The laptop is locked: the panel stays on the lock screen with limited content.
    locked: bool,
    /// Last known brightness (the first event is only the initial state).
    brightness: Option<u32>,
    /// Last Caps Lock and Num Lock state.
    lock_keys: (bool, bool),
    /// Crest listens to raw keyboard input (only for Caps/Num Lock).
    raw_keyboard: bool,
    /// Crest is installed as a package, so message notifications can be read if allowed.
    messages_available: bool,
    /// The user denied permission to read notifications.
    messages_denied: bool,
    /// The Win+Alt+N hotkey is registered.
    hotkey: bool,
    /// The cursor touches the pill: it grows slightly before the panel opens.
    peek: bool,
    /// Drop zone currently right under the dragged file.
    drop_over: Option<DropZone>,
    /// Windows Share window; created on first use.
    share: Option<share::Share>,
    /// When a file was last dropped, for the glow.
    drop_glow: Option<f64>,
    /// Current and target fill of the volume/brightness HUD bar (slides smoothly).
    hud_fill: f32,
    hud_fill_target: f32,
    /// HUD number currently rolling: old text, start time, up/down.
    hud_roll: Option<(String, f64, bool)>,
    /// Shelf tiles appearing (`false`) or disappearing (`true`), and when that started.
    tile_fx: HashMap<u64, (f64, bool)>,
    /// Current shadow strength (see [`Scene::shadow`]).
    shadow: f32,
    /// Current and target gaze direction (see [`Eyes::look`]).
    gaze: (f32, f32),
    /// The hovered element's highlight fading in, 0..1; restarts when the hover moves.
    hover_mix: f32,
    /// Focus or Timer: which half of the shared timer tab was used last.
    timer_tab: Page,
    gaze_target: (f32, f32),
    /// Last cursor position and when it last moved.
    cursor: POINT,
    cursor_moved: Instant,
    /// Interval of the active gaze timer.
    gaze_poll: Option<u32>,
    /// The mascot's temporary expression and when it ends.
    mood: Option<(Mood, Instant)>,
    /// The panel's right-click menu is open; do not close the panel yet.
    menu_open: bool,
    /// Last Wi-Fi state; `None` until the first event (the initial state is not announced).
    network: Option<network::State>,
    /// Screenshots and Downloads folder watchers.
    screenshots: Option<Watch>,
    downloads: Option<Watch>,
    timer: Timer,
    agenda: Agenda,
    /// Day selected in the calendar, as an offset from today.
    cal_offset: i32,
    /// Accumulated two-finger swipe on the pill, when it last moved, and when the track last changed.
    swipe: (i32, f64, f64),
    /// When the mascot started waking up (panel just appeared, after unlock or resume from sleep).
    wake: Option<f64>,
    /// Download in progress: file count and total bytes.
    downloading: Option<(usize, u64)>,
    /// Last speaker volume (0..1, muted) and microphone mute, for the controls page.
    volume: Option<(f32, bool)>,
    mic: Option<bool>,
    /// Slider being dragged.
    slider_drag: Option<Hit>,
    /// Crest quiet mode until this time.
    quiet_until: Option<Instant>,
    city: Option<City>,
    /// City name being typed.
    city_draft: Option<String>,
    /// City search status, e.g. "Searching for city…".
    city_status: Option<String>,
    weather: Option<(Weather, Instant)>,
    /// Today's prayer times for the saved city; `None` until a city is set.
    prayer: Option<crate::tools::prayer::Day>,
    /// Today's key ("2026-10-19"), for the water log and daily reset.
    day: String,
    water_due: Instant,
    /// Shelf tile being pressed and its start point, to detect a drag.
    press: Option<(u64, POINT)>,
    /// Tile being dragged out.
    drag_item: Option<u64>,
    /// Window active before task editing began; focus returns here.
    edit_return: Option<HWND>,
    /// First half of a UTF-16 character (e.g. emoji) waiting for its pair.
    pending_surrogate: Option<u16>,
    /// Leftover mouse wheel rotation short of a full step (touchpads send small steps).
    wheel: i32,
    /// Interval of the active ambient timer.
    ambient: Option<u32>,
    epoch: Instant,
    last_frame: f64,
    /// When the last full draw happened (drawing only the waveform does not update clock/ring).
    last_full: f64,
    rng: u32,
    taskbar_created: u32,
    /// Message from the installer to exit cleanly (see `install::stop_running`).
    quit_message: u32,
    /// The first-run welcome is pending or shown.
    welcome: bool,
    /// The Administrator step for messages is running; its result is only accepted
    /// during this time, not from any other program at any time.
    identity_pending: bool,
    /// Last read coding agent sessions, plus their scanner.
    /// Watches coding agents while the Agents page is enabled.
    agent_listener: Option<agents::Listener>,
    agent_rows: Vec<agents::Session>,
    agent_hooks: agents::HookStatus,
    /// First agent row shown; the rest scroll with the mouse wheel.
    agent_scroll: usize,
    /// The first rows after start are not compared, so old states do not raise notices.
    agents_seen: bool,
    /// Google account: the signed-in address, unread mail, and calendar events.
    account: Option<String>,
    mail: Vec<Mail>,
    /// Ids of the unread messages last shown, and whether the first read has happened.
    /// Together they turn a refresh into "these are new" without a second API call.
    mail_known: Vec<String>,
    mail_seen: bool,
    mail_status: Option<String>,
    /// A sign-in is waiting for the browser to come back.
    connecting: bool,
    google_events: Vec<google::CalendarEvent>,
    /// Next token refresh; calendar and mail share it.
    google_due: Option<Instant>,
    /// Google client id and secret shown on the Mail page; kept until Connect is pressed.
    google_id: String,
    google_secret: String,
    /// Text box being typed into: the client id, the client secret, or a feed address.
    edit_field: Option<Field>,
    /// Text of the box in use, while it is being typed into.
    edit_text: String,
    /// Calendar feeds the user subscribed to.
    feeds: Feeds,
    /// Plugin rows: the folder's plugins and what they last printed.
    plugins: Plugins,
    /// The Mail and Plugins pages, built when their data changes rather than on every frame.
    mail_view: MailView,
    plugins_view: PluginsView,
    clock_text: clock_text::ClockText,
}

impl App {
    fn new(hwnd: HWND) -> Result<Self> {
        let (monitor, monitor_rect) = primary_monitor();
        let settings = Settings::load();
        let renderer = Renderer::new(scaled_dpi(monitor_dpi(monitor), settings.size), settings.theme)?;
        let icon = ui::make_icon(unsafe { GetSystemMetrics(SM_CXSMICON) })?;
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
        let drop = DropRegistration::register(hwnd)
            .inspect_err(|e| error_log!("file drag and drop unavailable: {e}"))
            .ok();
        let (w, h) = View::Pill.size();
        let mut app = Self {
            hwnd,
            renderer,
            tray: Tray::new(hwnd, icon),
            _watcher: Watcher::new(hwnd),
            _power: Power::register(hwnd),
            _drop: drop,
            services: Services::start(hwnd),
            monitor,
            monitor_rect,
            view: View::Pill,
            width: Spring::new(w, 300.0, 0.74, SHAPE_PRECISION),
            height: Spring::new(h, 300.0, 0.74, SHAPE_PRECISION),
            from: (w, h),
            alpha: [1.0, 0.0, 0.0, 0.0],
            page: Page::Home,
            page_anim: None,
            blink_since: None,
            expanded: false,
            hovering: false,
            tracking_leave: false,
            hovered: None,
            mode: Mode::Normal,
            user_hidden: false,
            shown: false,
            media: None,
            hud: None,
            hud_visible: false,
            plugged: None,
            battery: None,
            shelf: Shelf::load(),
            focus: Focus::default(),
            water: Water::load(),
            tasks: Tasks::load(),
            settings,
            privacy: Usage::default(),
            privacy_known: false,
            lyrics: None,
            lyric_pending: None,
            lyric_shown: None,
            lyric_mix: 0.0,
            lyric_scroll: None,
            lyric_drawn: None,
            second_drawn: None,
            seek_preview: None,
            panel_resize: false,
            system: None,
            system_watch: false,
            target: (w, h),
            locked: false,
            brightness: None,
            lock_keys: lock_key_states(),
            raw_keyboard: false,
            messages_available: false,
            messages_denied: false,
            hotkey: false,
            peek: false,
            drop_over: None,
            share: None,
            drop_glow: None,
            hud_fill: 0.0,
            hud_fill_target: 0.0,
            hud_roll: None,
            tile_fx: HashMap::new(),
            shadow: 0.0,
            gaze: (0.0, 0.0),
            hover_mix: 1.0,
            timer_tab: Page::Focus,
            gaze_target: (0.0, 0.0),
            cursor: POINT::default(),
            cursor_moved: Instant::now(),
            gaze_poll: None,
            mood: None,
            menu_open: false,
            network: None,
            screenshots: None,
            downloads: None,
            timer: Timer::default(),
            agenda: Agenda::load(today()),
            cal_offset: 0,
            swipe: (0, 0.0, f64::MIN),
            wake: Some(0.0),
            downloading: None,
            volume: None,
            mic: None,
            slider_drag: None,
            quiet_until: None,
            city: City::load(),
            city_draft: None,
            city_status: None,
            weather: None,
            prayer: None,
            day: clock::day_key(&clock::now()),
            water_due: Instant::now() + WATER_EVERY,
            press: None,
            drag_item: None,
            edit_return: None,
            pending_surrogate: None,
            wheel: 0,
            ambient: None,
            epoch: Instant::now(),
            last_frame: 0.0,
            last_full: 0.0,
            rng: seed | 1,
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            quit_message: unsafe { RegisterWindowMessageW(install::QUIT_MESSAGE) },
            welcome: !settings.welcomed,
            identity_pending: false,
            agent_listener: None,
            agents_seen: false,
            agent_rows: Vec::new(),
            agent_hooks: agents::installed_hooks(),
            agent_scroll: 0,
            account: None,
            mail: Vec::new(),
            mail_known: Vec::new(),
            mail_seen: false,
            mail_status: Some("Not connected".to_string()),
            connecting: false,
            google_events: Vec::new(),
            google_due: None,
            google_id: google::Client::load().map(|c| c.id).unwrap_or_default(),
            google_secret: google::Client::load().map(|c| c.secret).unwrap_or_default(),
            edit_field: None,
            edit_text: String::new(),
            // Nothing is read from the folders of features the user has switched off.
            feeds: Feeds::load(settings.calendar && settings.google_calendar),
            plugins: Plugins::load(settings.plugins),
            // Both are filled by `rebuild_views` below, before the first draw.
            mail_view: MailView {
                account: None,
                status: None,
                rows: Vec::new(),
                setup: false,
                id: String::new(),
                secret: String::new(),
                editing: None,
                connecting: false,
            },
            plugins_view: PluginsView { rows: Vec::new(), problems: Vec::new(), count: 0, empty: true },
            clock_text: Default::default(),
        };
        app.services.thumbnails(app.shelf.items.iter().map(|item| (item.id, item.path.clone())).collect());
        // Lock/unlock events, so the panel stays visible on the lock screen.
        if let Err(e) = unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } {
            error_log!("lock screen events unavailable: {e}");
        }
        app.set_raw_keyboard(app.settings.lock_keys);
        app.messages_available = messages::packaged();
        debug_log!("package identity: {}", app.messages_available);
        if app.messages_available && app.settings.messages != Messages::Off {
            messages::request_access(app.services.notifier());
        }
        app.set_hotkey(app.settings.hotkey);
        app.update_watchers();
        app.services.send(Task::WatchNetwork(app.settings.wifi));
        app.fetch_weather();
        app.load_google();
        app.fetch_calendars();
        if app.welcome {
            app.set_timer(TIMER_WELCOME, WELCOME_DELAY_MS);
        }
        app.rebuild_views();
        Ok(app)
    }

    fn handle(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEMOVE => self.on_mouse_move(lparam),
            WM_MOUSELEAVE => {
                self.on_leave();
                self.set_hovered(None);
            }
            WM_LBUTTONDOWN => self.on_press(lparam),
            WM_LBUTTONUP => self.on_click(lparam),
            WM_LBUTTONDBLCLK => self.on_double_click(lparam),
            WM_RBUTTONUP => self.on_right_click(lparam),
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => self.on_wheel(wparam, lparam, msg == WM_MOUSEHWHEEL),
            WM_CHAR => self.on_char(wparam.0 as u16),
            WM_ACTIVATE => {
                // Clicked another app while typing: keep what was typed.
                if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    self.end_edit(true, false);
                    self.refresh();
                }
                return None;
            }
            WM_SETCURSOR => {
                if !self.hovered.is_some_and(Hit::clickable) {
                    return None;
                }
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return Some(LRESULT(1));
            }
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_TIMER => self.on_timer(wparam.0),
            WM_HOTKEY => self.on_hotkey(),
            WM_INPUT => {
                self.on_raw_input(lparam);
                // Windows needs to clean up its own raw input data.
                return None;
            }
            WM_WTSSESSION_CHANGE => self.on_session_change(wparam.0 as u32),
            dnd::WM_DRAG => self.on_drag(wparam.0, lparam),
            desktop::WM_DESKTOP_CHANGED => self.set_timer(TIMER_DESKTOP, DESKTOP_SETTLE_MS),
            desktop::WM_APPBAR => self.refresh_mode(false),
            services::WM_SERVICE => self.on_service(),
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                self.place();
                self.refresh_mode(true);
            }
            WM_TIMECHANGE => self.on_clock(),
            WM_POWERBROADCAST => return self.on_power_broadcast(wparam, lparam),
            m if m == self.taskbar_created => self.tray.add(),
            m if m == self.quit_message => unsafe { PostQuitMessage(0) },
            WM_IDENTITY => self.on_identity(wparam.0),
            _ => return None,
        }
        Some(LRESULT(0))
    }
}

impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            let _ = WTSUnRegisterSessionNotification(self.hwnd);
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
        }
    }
}

/// Turn raw media data into ready-to-draw text, with a live-advancing position.
/// While the bar is dragged, the position follows `seek` (0..1).
fn media_view(s: &Snapshot, seek: Option<f32>) -> MediaView {
    let position = match seek {
        Some(fraction) if s.duration > 0.0 => fraction as f64 * s.duration,
        _ => s.position_now(),
    };
    MediaView {
        title: s.title.clone(),
        artist: s.artist.clone(),
        playing: s.playing,
        progress: (s.duration > 1.0).then(|| (position / s.duration) as f32),
        elapsed: clock::duration(position),
        remaining: format!("-{}", clock::duration(s.duration - position)),
        can_previous: s.can_previous,
        can_next: s.can_next,
        can_seek: s.can_seek && s.duration > 1.0,
        seeking: seek.is_some(),
        accent: s.art.as_ref().and_then(|art| art.accent),
    }
}

/// Point from a mouse message's `lparam`.
fn point(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as u16 as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32)
}

/// Current Caps Lock and Num Lock light state.
fn lock_key_states() -> (bool, bool) {
    unsafe { (GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0, GetKeyState(VK_NUMLOCK.0 as i32) & 1 != 0) }
}

/// Clipboard text, for Ctrl+V while typing.
fn clipboard_text() -> Option<String> {
    unsafe {
        OpenClipboard(None).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT.0 as u32).ok().and_then(|handle| {
            let memory = HGLOBAL(handle.0);
            let ptr = GlobalLock(memory) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(memory);
            Some(text)
        });
        let _ = CloseClipboard();
        text
    }
}

fn primary_monitor() -> (HMONITOR, RECT) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        (monitor, info.rcMonitor)
    }
}

/// Scale and opacity of a shelf tile appearing or disappearing, `elapsed` seconds since start.
fn tile_look(elapsed: f64, removing: bool) -> (f32, f32) {
    if removing {
        let t = (elapsed / TILE_OUT_SECS).min(1.0) as f32;
        (1.0 - 0.35 * t, 1.0 - t)
    } else {
        // Grows slightly past full size, then settles back (ease-out-back).
        let t = (elapsed / TILE_IN_SECS).min(1.0) as f32;
        let (c1, u) = (1.70158, t - 1.0);
        let back = 1.0 + (c1 + 1.0) * u * u * u + c1 * u * u;
        (0.5 + 0.5 * back, (t * 2.0).min(1.0))
    }
}

/// Bring the Crest window to the front. Right after a file drop, the source app (e.g. Explorer)
/// still holds input so Windows refuses; that app's input queue is borrowed briefly, the
/// mechanism Windows provides for switching the foreground window between apps.
fn bring_to_front(hwnd: HWND) {
    unsafe {
        if SetForegroundWindow(hwnd).as_bool() {
            return;
        }
        let front = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let me = GetCurrentThreadId();
        if front != 0 && front != me && AttachThreadInput(me, front, true).as_bool() {
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(me, front, false);
        }
    }
}

/// Zone where a dragged file will be dropped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DropZone {
    Shelf,
    Share,
}

/// Text box currently in use.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Task,
    City,
    Agenda,
    /// Google client id on the Mail page.
    GoogleId,
    /// Google client secret on the Mail page.
    GoogleSecret,
}

/// Today's date.
fn today() -> Date {
    let t = clock::now();
    Date::new(t.wYear as i32, t.wMonth as u32, t.wDay as u32)
}

/// Level of the volume/brightness HUD bar, 0..1.
fn hud_level(hud: &Hud) -> Option<f32> {
    match hud {
        Hud::Volume { level, .. } | Hud::Brightness { level } => Some(*level),
        _ => None,
    }
}

/// HUDs that still appear (and sound) while Crest is in quiet mode.
fn urgent(hud: &Hud) -> bool {
    matches!(hud, Hud::Power { low: true, .. } | Hud::Timer { .. } | Hud::Agent { needs_input: true, .. })
}

/// Drawing DPI: the screen DPI times the chosen size.
fn scaled_dpi(dpi: u32, size: Size) -> u32 {
    (dpi as f32 * size.factor()).round() as u32
}

/// The mascot's sleepy hours (23:00–04:59).
fn night(hour: u16) -> bool {
    !(5..23).contains(&hour)
}

fn monitor_dpi(monitor: HMONITOR) -> u32 {
    let (mut x, mut y) = (96, 96);
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
    x
}
