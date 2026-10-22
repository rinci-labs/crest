//! Draws the shape with software-mode Direct2D into a bitmap, then pushes it to a layered
//! window. No GPU is used at all, so a discrete GPU stays asleep and RAM use stays small.
//! As a bonus, transparent pixels are click-through, so only the shape catches the mouse.
//!
//! This module holds the core and the basic drawing helpers; each page's layout is in `pages`.

pub mod motion;
mod pages;
pub mod tokens;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::PathBuf;

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT,
    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_GRADIENT_STOP, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE, D2D1_ARC_SIZE_LARGE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_CUSTOM,
    D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_LINE_JOIN_ROUND,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1BitmapBrush, ID2D1Brush,
    ID2D1DCRenderTarget, ID2D1Factory, ID2D1PathGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
    DWriteCreateFactory,
    IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateBitmap,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush,
    HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, HICON, ICONINFO, UPDATELAYEREDWINDOWINFO, ULW_ALPHA,
    UpdateLayeredWindowIndirect,
};
use windows::core::{BOOL, PCWSTR, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::feeds::agents::{self, Session};
use crate::system::bluetooth::DeviceKind;
use crate::feeds::media::{ART_PX, Art};
use crate::system::privacy::Usage;
use crate::shell::settings::{Settings, Size, Theme};
use crate::shell::tray::Command;
use crate::system::stats::Stats;
use crate::system::thumbnail::Thumbnail;
use crate::tools::{Page, Pages};
use crate::tools::focus::Phase;
use crate::tools::shelf::Shelf;
use crate::tools::tasks::Tasks;

/// The shape. Sizes are in DIP (1 DIP = 1 pixel at 100% display scale).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// Thin line while the active window is maximized.
    Slim,
    Pill,
    /// Wide pill showing the lyric line currently being sung.
    LyricPill,
    /// Short notice: volume, power, Bluetooth, focus, water, privacy.
    Hud,
    /// Compact notice: the shape widens sideways at pill height (volume,
    /// brightness, Caps/Num Lock, charger).
    Strip,
    /// Open panel holding the widget pages.
    Panel,
    /// Wide Home: music/clock on the left, calendar on the right.
    PanelWide,
    /// Taller panel for Settings, so its groups get room to breathe instead of being packed
    /// into the page height every other page uses.
    PanelTall,
}

/// Number of contents that can fade in/out; the order matches [`View::content`].
pub const CONTENTS: usize = 4;

impl View {
    pub const fn size(self) -> (f32, f32) {
        match self {
            View::Slim => (132.0, 7.0),
            View::Pill => (176.0, 30.0),
            View::LyricPill => (360.0, 30.0),
            View::Hud => (320.0, 42.0),
            View::Strip => (300.0, 30.0),
            View::Panel => (440.0, tokens::PANEL_H),
            View::PanelWide => (660.0, tokens::PANEL_H),
            View::PanelTall => (440.0, tokens::PANEL_H + 48.0),
        }
    }

    /// Open panel, normal or wide.
    pub const fn is_panel(self) -> bool {
        matches!(self, View::Panel | View::PanelWide | View::PanelTall)
    }

    /// Index of the content shown in this shape; the thin line holds nothing.
    pub const fn content(self) -> Option<usize> {
        match self {
            View::Slim => None,
            View::Pill => Some(0),
            View::LyricPill => Some(1),
            View::Hud | View::Strip => Some(2),
            View::Panel | View::PanelWide | View::PanelTall => Some(3),
        }
    }
}

/// Short notice shown briefly in the shape.
pub enum Hud {
    Volume { level: f32, muted: bool },
    Power { plugged: bool, percent: Option<u32>, low: bool },
    Bluetooth { id: String, name: String, connected: bool, battery: Option<u8>, kind: DeviceKind },
    Focus { finished: Phase },
    Water { ml: u32, goal: u32 },
    /// An app started using the camera (`camera`) or the microphone.
    Privacy { camera: bool, app: String },
    /// Screen brightness, 0..1.
    Brightness { level: f32 },
    /// Caps Lock (`caps`) or Num Lock turned on/off.
    LockKey { caps: bool, on: bool },
    /// Incoming message; its logo is looked up by `aumid` (see [`Renderer::set_logo`]).
    Message { app: String, aumid: String, sender: String, text: String, browser: bool },
    /// Agenda item due now, e.g. "Team meeting" at "09.00".
    Agenda { text: String, time: String },
    /// New screenshot landed on the shelf; `id` is the shelf item for its thumbnail, `more` = other screenshots.
    Screenshot { id: u64, name: String, more: usize },
    /// Download finished; `more` = other files that finished at the same time.
    Download { path: PathBuf, name: String, more: usize },
    Wifi { name: String, connected: bool, internet: bool },
    /// Quick timer ran out after `minutes` minutes.
    Timer { minutes: u32 },
    /// A coding agent needs input (`needs_input`) or just finished.
    Agent { kind: agents::Kind, title: String, project: String, detail: String, needs_input: bool, pid: u32 },
    /// A new unread mail arrived; `more` counts the others that came with it.
    Mail { from: String, subject: String, initials: String, more: usize },
    /// Prayer `name` at `time` ("15:12") is `ahead` minutes away; 0 = the azan is now.
    Prayer { name: &'static str, time: String, ahead: u32 },
}

/// Mascot expression.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mood {
    #[default]
    Normal,
    /// Night time: eyes half closed.
    Sleepy,
    /// Water goal reached, focus session done: "^ ^" eyes.
    Happy,
    /// Low battery, camera just started: round eyes.
    Surprised,
}

/// Mascot eye state.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Eyes {
    /// 1 = open, 0 = closed (blinking).
    pub open: f32,
    /// Gaze direction, each -1..1 (left..right, up..down).
    pub look: (f32, f32),
    pub mood: Mood,
}

impl Default for Eyes {
    fn default() -> Self {
        Self { open: 1.0, look: (0.0, 0.0), mood: Mood::Normal }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Previous,
    PlayPause,
    Next,
}

/// Clickable/hoverable part of the panel. Registered while drawing, so its position always
/// matches what is visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    PageDot(Page),
    Media(Button),
    /// Lyrics button under the player: show/hide the lyrics column.
    LyricsToggle,
    /// Song progress bar (click/drag to seek, see [`Renderer::slider_value`]).
    MediaSeek,
    /// Background of the shelf tile row (for the mouse wheel).
    ShelfStrip,
    ShelfItem(u64),
    ShelfRemove(u64),
    ShelfClear,
    FocusToggle,
    FocusReset,
    FocusPhase(Phase),
    WaterDrink,
    WaterUndo,
    /// Background of the task list (for the mouse wheel).
    TaskList,
    TaskToggle(usize),
    TaskRemove(usize),
    TaskInput,
    TasksClearDone,
    /// Volume and brightness sliders (the value comes from the click position, see [`Renderer::slider_value`]).
    VolumeSlider,
    BrightnessSlider,
    VolumeMute,
    MicToggle,
    QuietToggle,
    CityInput,
    TimerToggle,
    TimerCancel,
    TimerAdd,
    TimerPreset(u32),
    /// Day in the calendar strip, as a day offset from today.
    CalendarDay(i32),
    /// Background of the date strip and agenda list (for the mouse wheel).
    CalendarStrip,
    AgendaList,
    /// One agenda row: only for hover, so the × button stays visible while the cursor
    /// moves from the circle to the right end of the row.
    AgendaRow(u64),
    AgendaToggle(u64),
    AgendaRemove(u64),
    AgendaInput,
    /// "Share" zone while a file is dragged onto the shape.
    DropShare,
    /// "Got it" button on the first-run welcome.
    WelcomeDone,
    /// Rescan agent sessions now, without waiting for the next scheduled scan.
    AgentsRefresh,
    /// Install optional lifecycle hooks for supported coding agents.
    AgentHooks,
    /// Install the optional lifecycle hook for one provider from Settings.
    AgentHook(crate::feeds::agents::Provider),
    /// Reveal one agent's project folder in Explorer.
    AgentFolder(usize),
    /// Background of the agent list (for the mouse wheel).
    AgentList,
    /// Rescan the plugin folder.
    PluginsReload,
    /// Open the plugin folder in Explorer.
    PluginsFolder,
    /// Background of the plugin list (for the mouse wheel).
    PluginList,
    /// One plugin row; opens its URL when it has one.
    PluginRow(usize),
    /// Connect the Google account on the Mail page.
    GoogleConnect,
    /// Box on the Mail page: 0 = client id, 1 = client secret.
    GoogleClient(u8),
    /// Forget the connected Google account.
    GoogleDisconnect,
    /// Open Gmail in the browser.
    GoogleOpen,
    /// One unread mail row (by index); opens Gmail. Separate from [`Hit::GoogleOpen`] so only
    /// the row under the cursor lights up.
    MailRow(usize),
    /// One agent row, by index into [`Scene::agents`]: click switches to its window.
    AgentRow(usize),
    /// Focus | Timer switch on the shared timer tab.
    TimerMode(Page),
    /// A switch on the settings page; handled exactly like the same tray menu choice.
    Command(Command),
}

impl Hit {
    /// Backgrounds only catch the mouse wheel, not clicks.
    pub fn clickable(self) -> bool {
        !matches!(
            self,
            Hit::ShelfStrip
                | Hit::TaskList
                | Hit::CalendarStrip
                | Hit::AgendaList
                | Hit::AgendaRow(_)
                | Hit::AgentList
                | Hit::PluginList
        )
    }
}

/// Media data ready to draw.
pub struct MediaView {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// 0..1, or `None` if the duration is unknown (e.g. a live stream).
    pub progress: Option<f32>,
    pub elapsed: String,
    pub remaining: String,
    pub can_previous: bool,
    pub can_next: bool,
    /// The progress bar can be dragged to seek; `seeking` = being dragged.
    pub can_seek: bool,
    pub seeking: bool,
    pub accent: Option<u32>,
}

/// Lyrics column beside the player: five lines around the one currently being sung.
pub struct LyricsView {
    /// Two previous lines, the current line (middle), and two next lines; empty when
    /// outside the song.
    pub lines: [String; 5],
    /// Scroll to the new line, 0..1 (1 = settled).
    pub scroll: f32,
    /// Placeholder text instead of lyrics, e.g. "Searching for lyrics…".
    pub status: Option<&'static str>,
}

pub struct FocusView {
    pub phase: Phase,
    pub running: bool,
    /// Started (running or paused midway).
    pub active: bool,
    /// Time left, e.g. "24:59".
    pub clock: String,
    pub progress: f32,
    pub sessions: u32,
    pub minutes_left: u32,
}

pub struct TimerView {
    pub running: bool,
    /// Running or paused.
    pub active: bool,
    /// Time left, e.g. "04:59".
    pub clock: String,
    /// Short time left for the pill, e.g. "5m" or "45s".
    pub short: String,
    pub progress: f32,
    pub total_minutes: u32,
}

/// Calendar column on the wide Home.
pub struct CalendarView {
    /// Month and year of the selected day, e.g. "Oct" and "2026".
    pub month: String,
    pub year: String,
    /// Five consecutive days, the selected day in the middle.
    pub days: Vec<DayView>,
    /// Agenda and feed events of the selected day.
    pub events: Vec<CalendarEntry>,
    pub scroll: usize,
}

pub struct DayView {
    /// Day offset from today.
    pub offset: i32,
    pub name: &'static str,
    pub number: u32,
    pub today: bool,
    pub selected: bool,
    pub has_events: bool,
}

/// Notice number rolling to a new value (e.g. volume 62 → 63).
pub struct Roll {
    /// Previous text.
    pub from: String,
    /// Progress, 0..1.
    pub t: f32,
    /// Value went up: the old number rolls up and out, the new one comes in from below.
    pub up: bool,
}

/// Value text of the volume ("62", "Muted") or brightness ("70") notice.
pub fn hud_value(hud: &Hud) -> Option<String> {
    match hud {
        Hud::Volume { muted: true, .. } => Some("Muted".to_string()),
        Hud::Volume { level, .. } | Hud::Brightness { level } => Some(format!("{}", (level * 100.0).round())),
        _ => None,
    }
}

/// Shelf animation: the drop area while a file is dragged, the glow after dropping, and tiles
/// appearing or disappearing.
#[derive(Default)]
pub struct ShelfFx {
    /// A file is being dragged right over the shelf drop area, or over the Share zone.
    pub targeted: bool,
    pub share_targeted: bool,
    /// Offset of the marching dashed line (in stroke widths).
    pub dash: f32,
    /// Glow after a file is dropped, 0..1 (1 = done).
    pub glow: Option<f32>,
    /// Scale and opacity of tiles that are appearing or disappearing.
    pub tiles: HashMap<u64, (f32, f32)>,
}

/// Contents of the quick controls page; `None` means unavailable on this machine.
pub struct ControlsView {
    /// Volume 0..1 and muted.
    pub volume: Option<(f32, bool)>,
    /// Brightness 0..100.
    pub brightness: Option<u32>,
    /// Microphone muted.
    pub mic: Option<bool>,
    /// Minutes left in quiet mode.
    pub quiet_minutes: Option<u32>,
    /// Label of the weather city button; `None` when weather is off.
    pub city: Option<String>,
}

/// Unread mail rows on the Mail page.
#[derive(Clone)]
pub struct MailView {
    /// Signed-in address, or `None` when no account is connected.
    pub account: Option<String>,
    /// Status line instead of rows: "Not connected", "Checking…", an error.
    pub status: Option<String>,
    /// Unread messages, newest first.
    pub rows: Vec<MailRow>,
    /// The setup boxes are shown (no account yet).
    pub setup: bool,
    /// Text of the client id box.
    pub id: String,
    /// Text of the client secret box.
    pub secret: String,
    /// Box currently being typed into: 0 = client id, 1 = client secret.
    pub editing: Option<u8>,
    /// A connection attempt is running.
    pub connecting: bool,
}

#[derive(Clone)]
pub struct MailRow {
    pub from: String,
    pub subject: String,
    pub age: String,
    /// Up to two letters for the sender's avatar circle.
    pub initials: String,
}

/// Plugin rows on the Plugins page.
#[derive(Clone)]
pub struct PluginsView {
    /// Rows from every plugin, in display order.
    pub rows: Vec<crate::feeds::plugins::Row>,
    /// Plugins that failed, shown below the rows.
    pub problems: Vec<crate::feeds::plugins::Row>,
    /// Number of plugins found in the folder.
    pub count: usize,
    /// The folder could not be read or is empty.
    pub empty: bool,
}

/// One calendar entry on Home, from the agenda or a subscribed feed.
pub struct CalendarEntry {
    /// Agenda id for a typed entry; `None` for a feed event, which is read-only.
    pub id: Option<u64>,
    /// "09.00", or empty for all-day.
    pub time: String,
    pub text: String,
    pub done: bool,
    /// Name of the feed the event came from; `None` for a typed entry.
    pub source: Option<String>,
    /// The event is running now.
    pub now: bool,
}

pub struct WaterView {
    pub ml: u32,
    pub goal: u32,
    /// Last seven days (ml and the day's initial), today rightmost.
    pub week: Vec<(u32, char)>,
    /// Time of the next reminder, e.g. "19:37".
    pub next: Option<String>,
}

/// One tile on the Prayer page.
pub struct PrayerTime {
    pub name: &'static str,
    /// "04:23", or "--:--" when the sun never gets there today.
    pub time: String,
    pub passed: bool,
    /// The coming prayer, lit with the accent.
    pub next: bool,
}

pub struct PrayerView {
    /// Saved city; `None` until one is set (the page then asks for it).
    pub city: Option<String>,
    /// City search status, e.g. "Searching for city…".
    pub status: Option<String>,
    /// Fajr, Sunrise, Dhuhr, Asr, Maghrib, Isha.
    pub times: Vec<PrayerTime>,
    /// Next prayer with an azan: name, time, and "in 1 h 12 min".
    pub next: Option<(&'static str, String, String)>,
    pub method: crate::tools::prayer::Method,
    pub lead: u32,
}

/// What to draw in one frame.
pub struct Scene<'a> {
    pub width: f32,
    pub height: f32,
    /// Opacity of each content: pill, lyric pill, HUD, panel.
    pub alpha: [f32; CONTENTS],
    pub eyes: Eyes,
    /// Strength of the shadow under the shape, 0..1 (full when open, faint when hovered).
    pub shadow: f32,
    pub time: &'a str,
    pub date: &'a str,
    pub greeting: &'a str,
    /// Battery percent and whether it is charging; `None` on a PC without a battery.
    pub battery: Option<(u32, bool)>,
    pub media: Option<&'a MediaView>,
    /// Lyric line currently being sung, if lyrics are on and found.
    pub lyric: Option<&'a str>,
    /// Lyrics column beside the player (lyrics button on).
    pub lyrics: Option<&'a LyricsView>,
    /// Home's right column slides from calendar (0) to lyrics (1).
    pub lyric_mix: f32,
    /// Lyrics button: `None` = hidden (lyrics feature off), `Some(on)`.
    pub lyrics_button: Option<bool>,
    pub hud: Option<&'a Hud>,
    /// Notice drawn compact (see [`View::Strip`]).
    pub hud_compact: bool,
    pub hud_roll: Option<&'a Roll>,
    /// Fill of the volume/brightness bar easing smoothly to its new value, 0..1.
    pub hud_fill: f32,
    pub page: Page,
    /// Page order and hidden pages, for the indicator dots.
    pub pages: Pages,
    /// Previous page during a transition: (page, progress 0..1, direction ±1).
    pub page_from: Option<(Page, f32, f32)>,
    pub shelf: &'a Shelf,
    pub shelf_fx: &'a ShelfFx,
    pub tasks: &'a Tasks,
    pub focus: &'a FocusView,
    pub water: &'a WaterView,
    pub prayer: &'a PrayerView,
    pub privacy: &'a Usage,
    /// Latest measurement; `None` before the first one arrives.
    pub system: Option<&'a Stats>,
    /// Time since boot, e.g. "3 h 12 m".
    pub uptime: &'a str,
    /// Machine locked: the panel shows only clock/music, without page navigation.
    pub locked: bool,
    pub timer: &'a TimerView,
    pub controls: &'a ControlsView,
    /// Weather for the clock page, e.g. "28° Cloudy · Jakarta".
    pub weather: Option<&'a str>,
    /// City name being typed.
    pub city_draft: Option<&'a str>,
    /// Download in progress shown in the pill, e.g. "12.4 MB" or "2 · 30.1 MB".
    pub download: Option<&'a str>,
    /// Calendar column; `None` = plain Home (calendar off or on the lock screen).
    pub calendar: Option<&'a CalendarView>,
    /// Agenda item being typed.
    pub agenda_draft: Option<&'a str>,
    /// Running coding agent sessions, most recent activity first.
    pub agents: &'a [Session],
    pub agent_hooks: agents::HookStatus,
    pub mail: &'a MailView,
    pub plugins: &'a PluginsView,
    /// First agent row shown.
    pub agent_scroll: usize,
    /// The panel holds the first-run welcome instead of a page.
    pub welcome: bool,
    /// Current settings, for the settings page.
    pub settings: Settings,
    pub hovered: Option<Hit>,
    /// Running seconds, for the music wave animation.
    pub seconds: f64,
    /// How far the hovered element's highlight has faded in, 0..1 (see `Renderer::hover`).
    pub hover: f32,
}

/// Theme colors. Green/red/orange/water blue stay the same in every theme because they carry meaning.
#[derive(Clone, Copy)]
struct Palette {
    /// Shape body.
    body: D2D1_COLOR_F,
    /// `--foreground`: main text and icons; also the base of translucent highlights
    /// (see `Renderer::tint`).
    text: D2D1_COLOR_F,
    /// `--muted-foreground`: secondary text.
    soft: D2D1_COLOR_F,
    /// `--muted-foreground` at rest: captions and labels.
    muted: D2D1_COLOR_F,
    /// `--border`/disabled text.
    dim: D2D1_COLOR_F,
    /// `--secondary`: a filled, non-accented surface.
    badge: D2D1_COLOR_F,
    eye: D2D1_COLOR_F,
    /// `--input`: the fill of a text field.
    placeholder: D2D1_COLOR_F,
    /// `--primary`.
    accent: D2D1_COLOR_F,
    /// `--primary-foreground`: text and icons on a primary fill.
    on_accent: D2D1_COLOR_F,
    /// Frost theme: thin bright edge and text shadow so it stays readable over the wallpaper.
    glass: bool,
}

/// Button styles, mirroring rust-ui's button variants (`default`, `secondary`, `outline`,
/// `ghost`, `link`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Primary,
    /// A primary fill in a status colour (water blue) instead of the theme accent.
    OnBlue,
    Secondary,
    Ghost,
    Link,
}

/// Contrast rule, measured on the pastel primaries: `on_accent` is ink (6–12:1 on every dark
/// theme's accent), never white (1.7–3.3:1). Status colours (green/amber/red/blue) are light in
/// every theme, so they also take ink labels.
///
/// Theme colors, named after the shadcn/rust-ui semantic tokens (`--background`,
/// `--foreground`, `--muted-foreground`, `--primary`, `--primary-foreground`, `--border`,
/// `--input`) so a page reads the same way as the component library's documentation.
///
/// Dark themes share zinc's text scale and differ only in the body tint and the primary color;
/// the Paper theme is the same scale inverted.
fn palette(theme: Theme) -> Palette {
    let dark = |body, dim, badge, placeholder, accent| Palette {
        glass: false,
        body: rgb(body, 1.0),
        text: rgb(0xFAFAFA, 1.0),
        soft: rgb(0xD4D4D8, 1.0),
        muted: rgb(0x8F8F99, 1.0),
        dim: rgb(dim, 1.0),
        badge: rgb(badge, 1.0),
        eye: rgb(0xFAFAFA, 1.0),
        placeholder: rgb(placeholder, 1.0),
        accent: rgb(accent, 1.0),
        // Every dark-theme primary is a light pastel (#8B7CFF, #7DD3FC, #2DD4BF, #FB923C):
        // dark ink on it reaches 6–12:1, while white text manages only 1.7–3.3:1.
        on_accent: rgb(0x09090B, 1.0),
    };
    match theme {
        // zinc-950 / iris
        Theme::Ink => dark(0x09090B, 0x52525B, 0x27272A, 0x18181B, 0x8B7CFF),
        // zinc-900 / sky-300
        Theme::Slate => dark(0x18181B, 0x52525B, 0x27272A, 0x1F1F23, 0x7DD3FC),
        // teal-950 / teal-400
        Theme::Ocean => dark(0x042F2E, 0x3B5866, 0x134E4A, 0x0B3B38, 0x2DD4BF),
        // stone-950 / orange-400
        Theme::Ember => dark(0x0C0A09, 0x57534E, 0x292524, 0x1C1917, 0xFB923C),
        Theme::Frost => Palette {
            body: rgb(0x18181F, 0.6),
            text: rgb(0xFFFFFF, 1.0),
            soft: rgb(0xE4E4E7, 1.0),
            muted: rgb(0xB4B4BC, 1.0),
            dim: rgb(0x71717A, 1.0),
            badge: rgb(0x5B5B66, 1.0),
            eye: rgb(0xFFFFFF, 1.0),
            placeholder: rgb(0xFFFFFF, 0.14),
            accent: rgb(0xA99DFF, 1.0),
            on_accent: rgb(0x09090B, 1.0),
            glass: true,
        },
        // paper: zinc-900 text on zinc-50, iris primary with white text.
        Theme::Paper => Palette {
            glass: false,
            body: rgb(0xFAFAFA, 1.0),
            text: rgb(0x18181B, 1.0),
            soft: rgb(0x3F3F46, 1.0),
            muted: rgb(0x71717A, 1.0),
            dim: rgb(0xA1A1AA, 1.0),
            badge: rgb(0xE4E4E7, 1.0),
            eye: rgb(0x18181B, 1.0),
            placeholder: rgb(0xF4F4F5, 1.0),
            accent: rgb(0x6D5DF6, 1.0),
            on_accent: rgb(0xFFFFFF, 1.0),
        },
    }
}

/// Bottom corner radius of the tab; grows with the shape (closed → open).
const RADIUS_MIN: f32 = 12.0;
const RADIUS_MAX: f32 = tokens::RADIUS_ISLAND;
/// Reach of the shadow under the shape.
const SHADOW: f32 = 16.0;
/// A shape this short counts as the thin line (see `View::Slim`).
const SLIM_MAX_H: f32 = 12.0;
/// The lyric pill widens with the line up to this limit; if still too short, it wraps to two lines.
const LYRIC_MIN_W: f32 = 300.0;
const LYRIC_MAX_W: f32 = 480.0;
const LYRIC_TWO_LINES_H: f32 = 44.0;
/// Room left and right of the lyric text for the cover and wave.
const LYRIC_SIDES: f32 = 84.0;
/// Window canvas: fits the largest shape and the overshoot when the spring bounces.
const CANVAS: (f32, f32) = {
    let (w, _) = View::PanelWide.size();
    let (_, h) = View::PanelTall.size();
    let widest = if LYRIC_MAX_W > w { LYRIC_MAX_W } else { w };
    (widest + 40.0, h + SHADOW + 6.0)
};

const CLEAR: D2D1_COLOR_F = rgb(0x000000, 0.0);
const WHITE: D2D1_COLOR_F = rgb(0xFFFFFF, 1.0);
const GREEN: D2D1_COLOR_F = rgb(0x34D399, 1.0);
const RED: D2D1_COLOR_F = rgb(0xF87171, 1.0);
const ORANGE: D2D1_COLOR_F = rgb(0xFBBF24, 1.0);
/// Water and Bluetooth blue.
const BLUE: D2D1_COLOR_F = rgb(0x60A5FA, 1.0);
/// Dark text on bright fills. Every status colour (green, amber, red, blue) is light in every
/// theme, where ink reaches 7.2–11.9:1 and white only 1.7–2.8:1, so status fills always take ink.
const INK: D2D1_COLOR_F = rgb(0x09090B, 1.0);
/// Ink for a label on a status-coloured fill, in every theme (see [`INK`]).
const ON_STATUS: D2D1_COLOR_F = INK;
/// App icon: iris tile with the mascot's round eyes.
const ICON_BG: D2D1_COLOR_F = rgb(0x8B7CFF, 1.0);
const ICON_EYE: D2D1_COLOR_F = rgb(0xFFFFFF, 1.0);

// Glyphs from the built-in Windows icon font (Segoe Fluent Icons / Segoe MDL2 Assets).
const ICON_HOME: char = '\u{E80F}';
const ICON_PLAY: char = '\u{F5B0}';
const ICON_PAUSE: char = '\u{F8AE}';
const ICON_PREVIOUS: char = '\u{F8AC}';
const ICON_NEXT: char = '\u{F8AD}';
/// Speech bubble: outlined while the lyrics column is hidden, filled while shown.
const ICON_LYRICS: char = '\u{E90A}';
const ICON_LYRICS_ON: char = '\u{ED0C}';
const ICON_MUTE: char = '\u{E74F}';
const ICON_VOLUME: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
const ICON_BLUETOOTH: char = '\u{E702}';
const ICON_HEADPHONE: char = '\u{E7F6}';
const ICON_KEYBOARD: char = '\u{E765}';
const ICON_PHONE: char = '\u{E8EA}';
const ICON_MUSIC: char = '\u{EC4F}';
const ICON_FOLDER: char = '\u{E8B7}';
const ICON_DOCUMENT: char = '\u{E8A5}';
const ICON_STOPWATCH: char = '\u{E916}';
const ICON_REFRESH: char = '\u{E72C}';
const ICON_CLOSE: char = '\u{E711}';
const ICON_CHECK: char = '\u{E73E}';
const ICON_TASKS: char = '\u{E9D5}';
const ICON_SYSTEM: char = '\u{E9D9}';
const ICON_CAMERA: char = '\u{E722}';
const ICON_MICROPHONE: char = '\u{E720}';
const ICON_BRIGHTNESS: char = '\u{E706}';
const ICON_MESSAGE: char = '\u{E8BD}';
const ICON_DOWNLOAD: char = '\u{E896}';
const ICON_WIFI: char = '\u{E701}';
const ICON_TIMER: char = '\u{E121}';
/// Gear, for the settings page.
const ICON_SETTINGS: char = '\u{E713}';
const ICON_CALENDAR: char = '\u{E787}';
const ICON_SHARE: char = '\u{E72D}';
/// Globe: marks settings that use the internet.
const ICON_GLOBE: char = '\u{E774}';
/// Terminal, for the agents page.
const ICON_TERMINAL: char = '\u{E756}';
const ICON_CHEVRON_UP: char = '\u{E70E}';
/// Envelope, for the Mail page.
const ICON_MAIL: char = '\u{E715}';
/// Open envelope: the hover action that opens Gmail.
const ICON_MAIL_OPEN: char = '\u{E8F1}';
/// Puzzle piece, for the Plugins page.
const ICON_PLUGIN: char = '\u{EA86}';
/// Notifications off (Segoe Fluent "RingerSilent"), for quiet mode.
const ICON_QUIET: char = '\u{E7ED}';
/// The weather city tile uses the globe: the place is looked up online.
const ICON_LOCATION: char = ICON_GLOBE;
/// Crescent moon (Segoe Fluent "QuietHours"), for prayer times.
const ICON_PRAYER: char = '\u{E708}';
const ICON_CHEVRON_DOWN: char = '\u{E70D}';
/// Battery 0..10 and charging battery 0..10, consecutive starting at these code points.
const ICON_BATTERY: u32 = 0xEBA0;
const ICON_BATTERY_CHARGING: u32 = 0xEBAB;

pub struct Renderer {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    target: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    /// Round line caps, for progress rings.
    round: ID2D1StrokeStyle,
    /// Dashed line, for the file drop area.
    dashed: ID2D1StrokeStyle,
    fonts: Fonts,
    surface: Surface,
    dpi: u32,
    pal: Palette,
    /// Album cover in use, with its id so it is not rebuilt.
    art: Option<(u64, ID2D1BitmapBrush)>,
    /// Shelf thumbnails per item id: brush, width, height.
    thumbs: HashMap<u64, (ID2D1BitmapBrush, f32, f32)>,
    /// Chat app logos per AppUserModelID.
    logos: HashMap<String, ID2D1BitmapBrush>,
    /// Clickable areas from the last full frame.
    hits: RefCell<Vec<(D2D_RECT_F, Hit)>>,
    /// Only record click areas once the panel has settled in place.
    recording: Cell<bool>,
    /// Fade-in of the hovered element's highlight for this frame, 0..1.
    hover: Cell<f32>,
    /// Scale of the panel/notice content and its center: content grows in from the top.
    zoom: Cell<(f32, f32)>,
    /// Last shape body with its parameters; rebuilt only when the shape changes.
    shape: RefCell<Option<(ShapeKey, ID2D1PathGeometry)>>,
    /// Marching dashed line, with the offset currently in use.
    marching: RefCell<Option<(f32, ID2D1StrokeStyle)>>,
    /// Vertical shift of the page being drawn (`tokens::CONTENT_SHIFT`), added to its click areas.
    shift: Cell<f32>,
    /// UTF-16 conversion buffer, reused so drawing text does not allocate.
    wide: RefCell<Vec<u16>>,
}

/// Dimensions that determine the shape body geometry.
#[derive(Clone, Copy, PartialEq)]
struct ShapeKey {
    width: f32,
    height: f32,
    radius: f32,
}

struct Fonts {
    pill_time: IDWriteTextFormat,
    greeting: IDWriteTextFormat,
    big_time: IDWriteTextFormat,
    date: IDWriteTextFormat,
    big_num: IDWriteTextFormat,
    caption: IDWriteTextFormat,
    caption_end: IDWriteTextFormat,
    caption_center: IDWriteTextFormat,
    tiny: IDWriteTextFormat,
    title: IDWriteTextFormat,
    body: IDWriteTextFormat,
    /// Like `body` but without "…", for text being typed.
    input: IDWriteTextFormat,
    /// Lyric line in the pill (centered, trimmed with "…").
    lyric: IDWriteTextFormat,
    /// Two-line lyric in the pill.
    lyric_wrap: IDWriteTextFormat,
    /// Current and surrounding lines in the Apple Music-style lyrics column.
    lyric_active: IDWriteTextFormat,
    lyric_context: IDWriteTextFormat,
    label: IDWriteTextFormat,
    label_end: IDWriteTextFormat,
    label_center: IDWriteTextFormat,
    icon_9: IDWriteTextFormat,
    icon_11: IDWriteTextFormat,
    icon_13: IDWriteTextFormat,
    icon_16: IDWriteTextFormat,
    icon_20: IDWriteTextFormat,
    icon_22: IDWriteTextFormat,
}

impl Fonts {
    fn new(dwrite: &IDWriteFactory) -> Result<Self> {
        let display = w!("Segoe UI Variable Display");
        let text = w!("Segoe UI Variable Text");
        let icons = icon_family(dwrite);
        let (lead, center, end) =
            (DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_TRAILING);
        let middle = DWRITE_PARAGRAPH_ALIGNMENT_CENTER;
        let (normal, semi) = (DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD);
        let f = |family, weight, size, align, trim| text_format(dwrite, family, weight, size, align, middle, trim);
        let wrapped = |size, align| -> Result<IDWriteTextFormat> {
            let format = text_format(dwrite, text, semi, size, align, middle, true)?;
            unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)? };
            Ok(format)
        };
        let icon = |size| text_format(dwrite, icons, normal, size, center, middle, false);
        Ok(Self {
            pill_time: f(display, semi, 13.0, center, false)?,
            greeting: f(text, normal, 13.0, lead, false)?,
            big_time: f(display, semi, 40.0, lead, false)?,
            date: f(text, normal, 14.0, lead, false)?,
            big_num: f(display, semi, 34.0, lead, false)?,
            // One type scale for every page; see `tokens`.
            caption: f(text, normal, tokens::TYPE_CAPTION, lead, true)?,
            caption_end: f(text, normal, tokens::TYPE_CAPTION, end, true)?,
            caption_center: f(text, normal, tokens::TYPE_CAPTION, center, true)?,
            tiny: f(text, normal, tokens::TYPE_TINY, center, true)?,
            title: f(text, semi, tokens::TYPE_TITLE, lead, true)?,
            body: f(text, normal, tokens::TYPE_BODY, lead, true)?,
            input: f(text, normal, tokens::TYPE_BODY, lead, false)?,
            lyric: f(text, semi, tokens::TYPE_BODY, center, true)?,
            lyric_wrap: wrapped(tokens::TYPE_CAPTION, center)?,
            lyric_active: wrapped(tokens::TYPE_LYRIC_ACTIVE, lead)?,
            lyric_context: f(text, normal, tokens::TYPE_LYRIC_CONTEXT, lead, true)?,
            label: f(text, semi, tokens::TYPE_LABEL, lead, true)?,
            label_end: f(display, semi, tokens::TYPE_LABEL, end, false)?,
            label_center: f(text, semi, tokens::TYPE_BODY, center, false)?,
            icon_9: icon(9.0)?,
            icon_11: icon(11.0)?,
            icon_13: icon(13.0)?,
            icon_16: icon(16.0)?,
            icon_20: icon(20.0)?,
            icon_22: icon(22.0)?,
        })
    }
}

impl Renderer {
    pub fn new(dpi: u32, theme: Theme) -> Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = dc_target(&factory, dpi as f32)?;
            let brush = target.CreateSolidColorBrush(&WHITE, None)?;
            let stroke = |dash_style, dashes: Option<&[f32]>| {
                let props = D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: dash_style,
                    dashOffset: 0.0,
                };
                factory.CreateStrokeStyle(&props, dashes)
            };
            let round = stroke(D2D1_DASH_STYLE_SOLID, None)?;
            let dashed = stroke(D2D1_DASH_STYLE_CUSTOM, Some(&[3.0, 3.0]))?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let (width, height) = canvas_px(dpi);
            Ok(Self {
                fonts: Fonts::new(&dwrite)?,
                factory,
                dwrite,
                target,
                brush,
                round,
                dashed,
                surface: Surface::new(width, height)?,
                dpi,
                pal: palette(theme),
                art: None,
                thumbs: HashMap::new(),
                logos: HashMap::new(),
                hits: RefCell::new(Vec::new()),
                recording: Cell::new(false),
                hover: Cell::new(1.0),
                zoom: Cell::new((1.0, 0.0)),
                shape: RefCell::new(None),
                marching: RefCell::new(None),
                shift: Cell::new(0.0),
                wide: RefCell::new(Vec::with_capacity(128)),
            })
        }
    }

    pub fn set_dpi(&mut self, dpi: u32) -> Result<()> {
        if dpi == self.dpi {
            return Ok(());
        }
        let (width, height) = canvas_px(dpi);
        self.surface = Surface::new(width, height)?;
        unsafe { self.target.SetDpi(dpi as f32, dpi as f32) };
        self.dpi = dpi;
        Ok(())
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.pal = palette(theme);
    }

    /// Window size in physical pixels.
    pub fn size(&self) -> SIZE {
        SIZE { cx: self.surface.width, cy: self.surface.height }
    }

    /// Physical pixels per DIP.
    pub fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    /// Panel part at a point (DIP, canvas coordinates), from the last full frame.
    pub fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits
            .borrow()
            .iter()
            .rev()
            .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .map(|(_, hit)| *hit)
    }

    /// Where a clickable area was recorded in the last full frame, for tests and layout checks.
    #[cfg(test)]
    pub fn hit_rect(&self, hit: Hit) -> Option<D2D_RECT_F> {
        self.hits.borrow().iter().find(|(_, h)| *h == hit).map(|(r, _)| *r)
    }

    /// Slider value (0..1) for position `x` (DIP), from the slider's place in the last full frame.
    /// Positions outside the slider clamp to its ends, so it can still be dragged all the way.
    pub fn slider_value(&self, hit: Hit, x: f32) -> Option<f32> {
        let hits = self.hits.borrow();
        let (area, _) = hits.iter().find(|(_, h)| *h == hit)?;
        Some(((x - area.left) / (area.right - area.left).max(1.0)).clamp(0.0, 1.0))
    }

    /// Replace the album cover. The bitmap is rebuilt only if the cover is actually new.
    pub fn set_art(&mut self, art: Option<&Art>) {
        let Some(art) = art else {
            self.art = None;
            return;
        };
        if self.art.as_ref().is_some_and(|(id, _)| *id == art.id) {
            return;
        }
        self.art = match self.bitmap_brush(ART_PX, ART_PX, &art.pixels) {
            Ok(brush) => Some((art.id, brush)),
            Err(e) => {
                error_log!("failed to create cover: {e}");
                None
            }
        };
    }

    pub fn set_thumbnail(&mut self, thumb: &Thumbnail) {
        match self.bitmap_brush(thumb.width, thumb.height, &thumb.pixels) {
            Ok(brush) => {
                self.thumbs.insert(thumb.id, (brush, thumb.width as f32, thumb.height as f32));
            }
            Err(e) => error_log!("failed to create thumbnail: {e}"),
        }
    }

    /// Drop thumbnails of items no longer on the shelf.
    pub fn retain_thumbnails(&mut self, keep: impl Fn(u64) -> bool) {
        self.thumbs.retain(|id, _| keep(*id));
    }

    /// Store a chat app logo (once per app).
    pub fn set_logo(&mut self, aumid: &str, size: u32, pixels: &[u8]) {
        if self.logos.contains_key(aumid) {
            return;
        }
        match self.bitmap_brush(size, size, pixels) {
            Ok(brush) => {
                self.logos.insert(aumid.to_string(), brush);
            }
            Err(e) => error_log!("failed to create app logo: {e}"),
        }
    }

    /// Lyric pill size for one line: widens with the text, rounded up to 24 DIP so it does not
    /// jitter slightly on every line; too long → maximum width, two lines.
    pub fn lyric_size(&self, line: &str) -> (f32, f32) {
        let needed = self.text_width(line, &self.fonts.lyric) + LYRIC_SIDES;
        if needed <= LYRIC_MAX_W {
            let width = (needed.max(LYRIC_MIN_W) / 24.0).ceil() * 24.0;
            (width.min(LYRIC_MAX_W), View::LyricPill.size().1)
        } else {
            (LYRIC_MAX_W, LYRIC_TWO_LINES_H)
        }
    }

    fn bitmap_brush(&self, width: u32, height: u32, pixels: &[u8]) -> Result<ID2D1BitmapBrush> {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            let bitmap = self.target.CreateBitmap(
                D2D_SIZE_U { width, height },
                Some(pixels.as_ptr().cast()),
                width * 4,
                &props,
            )?;
            self.target.CreateBitmapBrush(&bitmap, None, None)
        }
    }

    pub fn draw(&self, scene: &Scene) -> Result<()> {
        let cx = CANVAS.0 / 2.0;
        let (w, h) = (scene.width.max(24.0), scene.height.max(4.0));
        // Opening from pill to panel: 0 at pill height, 1 at panel height. Derived from the shape
        // sizes, so changing the panel height does not silently change the corners.
        let grow = ((h - View::Pill.size().1) / (View::Panel.size().1 - View::Pill.size().1)).clamp(0.0, 1.0);
        let radius = lerp(RADIUS_MIN, RADIUS_MAX, grow).min(h).min(w / 2.0 - 1.0).max(0.0).round();
        let (x0, x1) = (cx - w / 2.0, cx + w / 2.0);
        let key = ShapeKey { width: w, height: h, radius };
        let mut cached = self.shape.borrow_mut();
        if cached.as_ref().is_none_or(|(old, _)| *old != key) {
            *cached = self.shape_path(cx, w, h, radius).ok().map(|geometry| (key, geometry));
        }
        let Some((_, shape)) = cached.as_ref() else { return Ok(()) };
        let [pill, lyric, hud, panel] = scene.alpha;
        self.hits.borrow_mut().clear();
        self.recording.set(panel >= 0.999 && scene.page_from.is_none());
        self.hover.set(scene.hover.clamp(0.0, 1.0));

        unsafe {
            let bounds = RECT { left: 0, top: 0, right: self.surface.width, bottom: self.surface.height };
            self.target.BindDC(self.surface.dc, &bounds)?;
            self.target.BeginDraw();
            self.target.Clear(Some(&CLEAR));
            if scene.shadow > 0.01 {
                // Only below the bottom edge of the shape. The Frost theme body is see-through, so a
                // shadow behind it would show as a dark band inside the panel.
                self.target.PushAxisAlignedClip(&rect(0.0, h, CANVAS.0, CANVAS.1), D2D1_ANTIALIAS_MODE_ALIASED);
                self.shadow(cx, w, h, radius, scene.shadow);
                self.target.PopAxisAlignedClip();
            }
            self.brush.SetColor(&self.pal.body);
            self.target.FillGeometry(shape, &self.brush, None::<&ID2D1Brush>);
            if panel > 0.01 {
                self.panel_glow(scene, shape, h, panel);
            }
            if self.pal.glass {
                self.brush.SetColor(&rgb(0xFFFFFF, 0.24));
                self.target.DrawGeometry(shape, &self.brush, 1.0, None::<&ID2D1StrokeStyle>);
            } else {
                // A restrained rim separates the black island from dark wallpapers and gives
                // light themes the same crisp, nested construction.
                self.brush.SetColor(&self.tint(tokens::HAIRLINE));
                self.target.DrawGeometry(shape, &self.brush, 1.0, None::<&ID2D1StrokeStyle>);
            }

            // Content is clipped to the shape body so it never draws outside it.
            self.target.PushAxisAlignedClip(&rect(x0, 0.0, x1, h), D2D1_ANTIALIAS_MODE_ALIASED);
            if pill > 0.01 {
                self.draw_pill(scene, x0, x1, h, pill);
            }
            if lyric > 0.01 {
                self.draw_lyric_pill(scene, x0, x1, h, lyric);
            }
            if hud > 0.01
                && let Some(data) = scene.hud
            {
                self.draw_hud(scene, data, x0, x1, h, hud);
            }
            if panel > 0.01 {
                self.draw_panel(scene, x0, x1, panel);
            }
            // Thin line (window maximized) with no content: the privacy dot stays at its right end.
            if h < SLIM_MAX_H && scene.alpha.iter().all(|a| *a < 0.01) {
                self.privacy_dot(scene.privacy, x1 - 8.0, h / 2.0, 1.0);
            }
            self.target.PopAxisAlignedClip();
            self.target.EndDraw(None, None)
        }
    }

    /// Redraw only the music wave in a settled pill (plain or lyric, sized `w`×`h`). Called ~12
    /// times per second while music plays, so it only touches this small box, not the whole
    /// canvas. Returns the changed area (pixels) for [`Renderer::present`].
    pub fn draw_wave(&self, w: f32, h: f32, seconds: f64, color: Option<u32>) -> Result<RECT> {
        let (cx, cy) = (CANVAS.0 / 2.0 + w / 2.0 - 22.0, h / 2.0);
        self.patch(cx, cy, 10.0, 8.0, || self.wave(cx, cy, seconds, accent(color, self.pal.text), 1.0))
    }

    /// Redraw only the mascot eyes in a settled pill (sized `w`×`h`), for blinking and glancing.
    /// The box is large enough for the farthest glance and the round eyes.
    pub fn draw_eyes(&self, w: f32, h: f32, eyes: Eyes) -> Result<RECT> {
        let (cx, cy) = (CANVAS.0 / 2.0 - w / 2.0 + 18.0, h / 2.0);
        self.patch(cx, cy, 13.0, 7.5, || self.eyes(cx, cy, EyeSize::PILL, eyes, 1.0))
    }

    /// Redraw a small box centered at (`cx`, `cy`) that lies entirely inside the shape body:
    /// cleared with the body color, then filled by `paint`. Returns its pixel area.
    fn patch(&self, cx: f32, cy: f32, half_w: f32, half_h: f32, paint: impl FnOnce()) -> Result<RECT> {
        let s = self.scale();
        let dirty = RECT {
            left: ((cx - half_w) * s).floor() as i32,
            top: ((cy - half_h) * s).floor() as i32,
            right: ((cx + half_w) * s).ceil() as i32,
            bottom: ((cy + half_h) * s).ceil() as i32,
        };
        let (dx, dy) = (dirty.left as f32 / s, dirty.top as f32 / s);
        unsafe {
            self.target.BindDC(self.surface.dc, &dirty)?;
            self.target.BeginDraw();
            self.target.SetTransform(&translation(-dx, -dy));
            self.target.Clear(Some(&self.pal.body));
            paint();
            self.target.SetTransform(&translation(0.0, 0.0));
            self.target.EndDraw(None, None)?;
        }
        Ok(dirty)
    }

    /// Push the bitmap to the window. Position and size are set at the same time; `dirty` limits
    /// the copy to the changed area.
    pub fn present(&self, hwnd: HWND, pos: POINT, dirty: Option<&RECT>) -> Result<()> {
        let size = self.size();
        let source = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: HDC::default(),
            pptDst: &pos,
            psize: &size,
            hdcSrc: self.surface.dc,
            pptSrc: &source,
            crKey: COLORREF(0),
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: dirty.map_or(std::ptr::null(), |r| r as *const RECT),
        };
        unsafe { UpdateLayeredWindowIndirect(hwnd, &info) }.ok()
    }

    /// Shape body: a tab hanging from the screen edge with straight sides and circular bottom
    /// corners. Edges sit on whole pixels so the sides stay crisp.
    fn shape_path(&self, cx: f32, w: f32, h: f32, radius: f32) -> Result<ID2D1PathGeometry> {
        let (x0, x1, h) = ((cx - w / 2.0).round(), (cx + w / 2.0).round(), h.round());
        unsafe {
            let geometry = self.factory.CreatePathGeometry()?;
            let sink = geometry.Open()?;
            sink.BeginFigure(pt(x0, 0.0), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(pt(x0, h - radius));
            sink.AddArc(&arc(x0 + radius, h, radius, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            sink.AddLine(pt(x1 - radius, h));
            sink.AddArc(&arc(x1, h - radius, radius, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            sink.AddLine(pt(x1, 0.0));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            Ok(geometry)
        }
    }

    /// Soft shadow under the shape: several thin layers reaching lower, imitating a blur (the
    /// software render target has no blur effect). Only below the bottom edge: each layer's left
    /// and right edges stay inside the body, so there is no dark band along the sides.
    fn shadow(&self, cx: f32, w: f32, h: f32, radius: f32, strength: f32) {
        const LAYERS: usize = 6;
        let per_layer = if self.pal.glass { 0.03 } else { 0.055 };
        for i in 0..LAYERS {
            let spread = 1.5 + i as f32 * (SHADOW - 1.5) / (LAYERS - 1) as f32;
            let inset = (radius * 0.6 - spread * 0.4).max(0.0);
            let area = rect(cx - w / 2.0 + inset, h - radius - 2.0, cx + w / 2.0 - inset, h + spread);
            self.rounded(area, radius, rgb(0x000000, per_layer), strength);
        }
    }

    /// Color glow from the bottom of the panel, reflecting the song cover: the cover color in
    /// the music player, orange while the timer runs, red/green while a focus session runs.
    /// Fades along with page transitions.
    fn panel_glow(&self, s: &Scene, shape: &ID2D1PathGeometry, h: f32, alpha: f32) {
        // Red/orange are brighter than cover colors, so they are made slightly fainter.
        let color = |page: Page| match page {
            Page::Home => s.media.and_then(|m| m.accent).map(|c| (rgb(c, 1.0), 1.0)),
            Page::Timer if s.timer.active => Some((ORANGE, 0.7)),
            Page::Focus if s.focus.active => Some((phase_color(s.focus.phase), 0.7)),
            _ => None,
        };
        let pages = match s.page_from {
            Some((from, t, _)) => {
                let eased = 1.0 - (1.0 - t).powi(3);
                [(color(from), 1.0 - eased), (color(s.page), eased)]
            }
            None => [(None, 0.0), (color(s.page), 1.0)],
        };
        // In the Paper theme the glow is fainter so dark text stays readable.
        let strength = if self.pal.text.r < 0.5 { 0.2 } else if self.pal.glass { 0.36 } else { 0.45 };
        for (color, weight) in pages {
            let Some((color, factor)) = color.filter(|_| weight > 0.01) else { continue };
            let k = strength * factor * weight * alpha;
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: with_alpha(color, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.6, color: with_alpha(color, k * 0.3) },
                D2D1_GRADIENT_STOP { position: 1.0, color: with_alpha(color, k) },
            ];
            unsafe {
                let Ok(collection) = self.target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                else {
                    return;
                };
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(0.0, h * 0.32), endPoint: pt(0.0, h) };
                let Ok(brush) = self.target.CreateLinearGradientBrush(&props, None, &collection) else { return };
                self.target.FillGeometry(shape, &brush, None::<&ID2D1Brush>);
            }
        }
    }

    /// Main text color made translucent, for highlights and bar tracks. Works for dark themes
    /// (faint white) and the Paper theme (faint black).
    fn tint(&self, alpha: f32) -> D2D1_COLOR_F {
        with_alpha(self.pal.text, alpha)
    }

    /// Record a clickable area (only while the panel is settled, see `recording`).
    fn hit(&self, area: D2D_RECT_F, hit: Hit) {
        if self.recording.get() {
            let dy = self.shift.get();
            self.hits.borrow_mut().push((rect(area.left, area.top + dy, area.right, area.bottom + dy), hit));
        }
    }

    /// A pair of capsule eyes glancing toward `look`. Blinking shrinks their height; the mood
    /// changes their shape.
    fn eyes(&self, cx: f32, cy: f32, size: EyeSize, eyes: Eyes, alpha: f32) {
        let EyeSize { w: eye_w, h: eye_h, gap } = size;
        let (dx, dy) = (eyes.look.0 * eye_w * 0.8, eyes.look.1 * eye_h * 0.16);
        let gap = if eyes.mood == Mood::Surprised { gap * 1.15 } else { gap };
        for x in [cx - gap / 2.0 + dx, cx + gap / 2.0 + dx] {
            let y = cy + dy;
            let (w, h, y) = match eyes.mood {
                Mood::Happy => {
                    self.happy_eye(x, y, eye_w, eye_h, alpha);
                    continue;
                }
                Mood::Surprised => (eye_h * 0.62, (eye_h * 0.78 * eyes.open).max(eye_w * 0.45), y),
                // Drooping lids: flat, wider eyes, slightly lower.
                Mood::Sleepy => (eye_w * 1.3, (eye_h * 0.24 * eyes.open).max(eye_w * 0.4), y + eye_h * 0.18),
                Mood::Normal => (eye_w, (eye_h * eyes.open).max(eye_w * 0.45), y),
            };
            self.rounded(rect(x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0), w.min(h) / 2.0, self.pal.eye, alpha);
        }
    }

    /// Smiling eye: a "^" arc with round ends.
    fn happy_eye(&self, x: f32, y: f32, eye_w: f32, eye_h: f32, alpha: f32) {
        let r = eye_h * 0.42;
        let base = y + r * 0.45;
        unsafe {
            let Ok(geometry) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = geometry.Open() else { return };
            sink.BeginFigure(pt(x - r, base), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&arc(x + r, base, r, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.brush.SetColor(&with_alpha(self.pal.eye, alpha));
                self.target.DrawGeometry(&geometry, &self.brush, eye_w * 0.5, &self.round);
            }
        }
    }

    /// Album cover with rounded corners; a box with a music note if there is none yet.
    fn art(&self, x: f32, y: f32, size: f32, radius: f32, alpha: f32) {
        let area = rect(x, y, x + size, y + size);
        match &self.art {
            Some((_, brush)) => self.fill_bitmap(brush, area, size / ART_PX as f32, radius, alpha),
            None => {
                self.rounded(area, radius, self.pal.placeholder, alpha);
                let font = if size > 30.0 { &self.fonts.icon_22 } else { &self.fonts.icon_13 };
                self.glyph(ICON_MUSIC, font, x + size / 2.0, y + size / 2.0, self.pal.muted, alpha);
            }
        }
    }

    /// Chat app logo with rounded corners; a message icon if the logo is not there yet.
    fn logo(&self, aumid: &str, cx: f32, cy: f32, size: f32, alpha: f32) {
        let area = rect(cx - size / 2.0, cy - size / 2.0, cx + size / 2.0, cy + size / 2.0);
        match self.logos.get(aumid) {
            Some(brush) => self.fill_bitmap(brush, area, size / crate::feeds::messages::LOGO_PX as f32, size * 0.24, alpha),
            None => {
                self.circle(cx, cy, size / 2.0, GREEN, alpha);
                self.glyph(ICON_MESSAGE, &self.fonts.icon_13, cx, cy, WHITE, alpha);
            }
        }
    }

    /// File thumbnail inside a `size` box, keeping its aspect ratio.
    fn thumb(&self, id: u64, is_dir: bool, cx: f32, top: f32, size: f32, alpha: f32) {
        match self.thumbs.get(&id) {
            Some((brush, w, h)) => {
                let k = (size / w).min(size / h);
                let (dw, dh) = (w * k, h * k);
                let (x, y) = (cx - dw / 2.0, top + (size - dh) / 2.0);
                self.fill_bitmap(brush, rect(x, y, x + dw, y + dh), k, 6.0, alpha);
            }
            None => {
                let icon = if is_dir { ICON_FOLDER } else { ICON_DOCUMENT };
                self.glyph(icon, &self.fonts.icon_22, cx, top + size / 2.0, self.pal.soft, alpha);
            }
        }
    }

    fn fill_bitmap(&self, brush: &ID2D1BitmapBrush, area: D2D_RECT_F, scale: f32, radius: f32, alpha: f32) {
        unsafe {
            brush.SetTransform(&Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: area.left, M32: area.top });
            brush.SetOpacity(alpha);
            self.target.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: area, radiusX: radius, radiusY: radius }, brush);
        }
    }

    /// Four bars bouncing while music plays.
    fn wave(&self, cx: f32, cy: f32, seconds: f64, color: D2D1_COLOR_F, alpha: f32) {
        const SPEED: [f64; 4] = [7.3, 9.1, 6.4, 8.2];
        const PHASE: [f64; 4] = [0.0, 1.9, 3.3, 4.6];
        for i in 0..4 {
            let level = (0.5 + 0.5 * (seconds * SPEED[i] + PHASE[i]).sin()) as f32;
            let (x, h) = (cx - 7.5 + i as f32 * 5.0, 3.0 + 9.0 * level);
            self.rounded(rect(x - 1.3, cy - h / 2.0, x + 1.3, cy + h / 2.0), 1.3, color, alpha);
        }
    }

    /// Privacy dot: green when the camera is in use, orange when only the microphone is.
    fn privacy_dot(&self, privacy: &Usage, x: f32, y: f32, alpha: f32) {
        if privacy.active() {
            let color = if privacy.camera.is_empty() { ORANGE } else { GREEN };
            self.circle(x, y, 3.0, color, alpha);
        }
    }

    /// Progress/volume bar: dim track filled up to `fraction`.
    fn bar(&self, x0: f32, x1: f32, cy: f32, fraction: f32, color: D2D1_COLOR_F, alpha: f32) {
        self.bar_sized(x0, x1, cy, 4.0, fraction, color, alpha);
    }

    /// A track-and-fill bar (rust-ui's `Progress`); `thick` units thick, and a draggable bar
    /// thickens on hover. Every progress bar and slider track in the panel uses this one shape.
    #[allow(clippy::too_many_arguments)]
    fn bar_sized(&self, x0: f32, x1: f32, cy: f32, thick: f32, fraction: f32, color: D2D1_COLOR_F, alpha: f32) {
        let r = thick / 2.0;
        self.rounded(rect(x0, cy - r, x1, cy + r), r, self.tint(tokens::TRACK), alpha);
        let fill = x0 + (x1 - x0) * fraction.clamp(0.0, 1.0);
        if fill > x0 + 0.5 {
            self.rounded(rect(x0, cy - r, fill.max(x0 + thick), cy + r), r, color, alpha);
        }
    }

    /// Progress ring filling clockwise from the top.
    fn ring(&self, cx: f32, cy: f32, ring: Ring, progress: f32, color: D2D1_COLOR_F, alpha: f32) {
        let Ring { radius, thickness } = ring;
        self.circle_outline(cx, cy, radius, thickness, self.tint(tokens::TRACK), alpha);
        let p = progress.clamp(0.0, 1.0);
        if p >= 0.999 {
            self.circle_outline(cx, cy, radius, thickness, color, alpha);
            return;
        }
        if p <= 0.001 {
            return;
        }
        let angle = p * TAU - FRAC_PI_2;
        let size = if p > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL };
        let end = (cx + radius * angle.cos(), cy + radius * angle.sin());
        unsafe {
            let Ok(geometry) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = geometry.Open() else { return };
            sink.BeginFigure(pt(cx, cy - radius), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&arc(end.0, end.1, radius, D2D1_SWEEP_DIRECTION_CLOCKWISE, size));
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.brush.SetColor(&with_alpha(color, alpha));
                self.target.DrawGeometry(&geometry, &self.brush, thickness, &self.round);
            }
        }
    }

    /// Water drop icon.
    fn drop_icon(&self, cx: f32, cy: f32, size: f32, color: D2D1_COLOR_F, alpha: f32) {
        let r = size * 0.3;
        let (bx, by) = (cx, cy + size * 0.16);
        let top = pt(cx, cy - size * 0.5);
        unsafe {
            let Ok(geometry) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = geometry.Open() else { return };
            sink.BeginFigure(top, D2D1_FIGURE_BEGIN_FILLED);
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: pt(cx + size * 0.06, cy - size * 0.3),
                point2: pt(bx + r, by - r),
                point3: pt(bx + r, by),
            });
            sink.AddArc(&arc(bx, by + r, r, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            sink.AddArc(&arc(bx - r, by, r, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: pt(bx - r, by - r),
                point2: pt(cx - size * 0.06, cy - size * 0.3),
                point3: top,
            });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_ok() {
                self.brush.SetColor(&with_alpha(color, alpha));
                self.target.FillGeometry(&geometry, &self.brush, None::<&ID2D1Brush>);
            }
        }
    }

    /// Pill button with text, e.g. "+ Drink 250 ml".
    /// A row surface: no fill at rest, a soft fill while the cursor is on it. Mirrors rust-ui's
    /// list-row/card hover treatment so every list on every page behaves the same.
    fn list_row(&self, area: D2D_RECT_F, radius: f32, hovered: bool, alpha: f32) {
        // Rows sit on the page deck, which is already a surface; a second resting plate per row
        // stacked surfaces and made lists look busy, so rows only fill while hovered.
        if hovered {
            self.rounded(area, radius, self.tint(tokens::FILL_SOFT), alpha * self.hover.get());
        }
    }

    /// A card/tile surface: a quiet filled plate, brighter when hovered.
    fn card(&self, area: D2D_RECT_F, radius: f32, hovered: bool, alpha: f32) {
        let fill = if hovered { lerp(tokens::FILL_SUBTLE, tokens::FILL_SOFT, self.hover.get()) } else { tokens::FILL_SUBTLE };
        self.rounded(area, radius, self.tint(fill), alpha);
    }

    /// A circular ghost icon button, the shape rust-ui uses for its icon-size buttons.
    fn icon_button(&self, cx: f32, cy: f32, radius: f32, hovered: bool, alpha: f32) {
        if hovered {
            self.circle(cx, cy, radius, self.tint(tokens::FILL_SOFT), alpha * self.hover.get());
        }
    }

    /// A hairline separator, vertical (`from`/`to` are y) or horizontal.
    fn separator(&self, x: f32, from: f32, to: f32, alpha: f32) {
        self.rounded(rect(x - 0.5, from, x + 0.5, to), 0.5, self.tint(tokens::DIVIDER), alpha);
    }

    /// An empty state, as rust-ui's `Empty`: a short title, an optional hint under it, and an
    /// optional glyph above. Every page without content uses this, so "nothing here" looks the
    /// same everywhere instead of four different hand-placed strings.
    fn empty_state(&self, area: D2D_RECT_F, icon: Option<char>, title: &str, hint: &str, alpha: f32) {
        let p = &self.pal;
        let (l, r) = (area.left, area.right);
        let centre = (area.top + area.bottom) / 2.0;
        let has_hint = !hint.is_empty();
        // Icon disc, title, and hint as one block centred in `area`, 8 DIP between the disc and
        // the title and 2 between the title and the hint.
        let disc = if icon.is_some() { 30.0 + 8.0 } else { 0.0 };
        let text_h = if has_hint { 18.0 + 2.0 + 16.0 } else { 18.0 };
        let top = centre - (disc + text_h) / 2.0;
        let title_y = top + disc + 9.0;
        let hint_y = title_y + 9.0 + 2.0 + 8.0;
        if let Some(icon) = icon {
            let cx = (l + r) / 2.0;
            self.circle(cx, top + 15.0, 15.0, self.tint(tokens::FILL_SUBTLE), alpha);
            self.glyph(icon, &self.fonts.icon_16, cx, top + 15.0, p.muted, alpha);
        }
        self.text(title, &self.fonts.label_center, rect(l, title_y - 9.0, r, title_y + 9.0), p.soft, alpha);
        if has_hint {
            self.text(hint, &self.fonts.caption_center, rect(l, hint_y - 8.0, r, hint_y + 8.0), p.muted, alpha);
        }
    }

    /// A badge, as rust-ui's `Badge`: a compact count or label on a secondary surface. `tone`
    /// picks the fill when the badge carries meaning (waiting = amber), otherwise it is neutral.
    fn badge(&self, cx: f32, cy: f32, text: &str, tone: Option<D2D1_COLOR_F>, alpha: f32) {
        let p = &self.pal;
        let width = self.text_width(text, &self.fonts.tiny) + 12.0;
        let area = rect(cx - width / 2.0, cy - 8.0, cx + width / 2.0, cy + 8.0);
        match tone {
            Some(color) => {
                // Status fills are light in every theme, so their label is ink (see `ON_STATUS`).
                self.rounded(area, 4.0, color, alpha);
                self.text(text, &self.fonts.tiny, area, ON_STATUS, alpha);
            }
            None => {
                self.rounded(area, 4.0, p.badge, alpha);
                self.text(text, &self.fonts.tiny, area, if self.pal.text.r > 0.5 { p.body } else { p.text }, alpha);
            }
        }
    }

    /// An inline alert, as rust-ui's `Alert`: a tinted plate with a colored hairline, for a
    /// message that explains the page rather than decorating it (a refused sign-in, a failed
    /// plugin). `tone` is the status colour.
    fn alert(&self, area: D2D_RECT_F, text: &str, tone: D2D1_COLOR_F, alpha: f32) {
        let p = &self.pal;
        self.rounded(area, tokens::RADIUS_ROW, with_alpha(tone, 0.10), alpha);
        self.outline(area, tokens::RADIUS_ROW, 1.0, false, with_alpha(tone, 0.35), alpha);
        let inner = rect(area.left + 10.0, area.top, area.right - 10.0, area.bottom);
        self.text(text, &self.fonts.caption, inner, p.text, alpha);
    }

    /// A small capsule button, following rust-ui's button variants. `pressed` = hovered.
    fn chip(&self, area: D2D_RECT_F, text: &str, variant: Style, pressed: bool, alpha: f32) {
        let p = &self.pal;
        // How far the hover highlight has come in (0 at rest).
        let lift = if pressed { self.hover.get() } else { 0.0 };
        let radius = (area.bottom - area.top) / 2.0;
        let (label, color) = match variant {
            Style::Primary => (text, p.on_accent),
            // Blue is light in every theme, so its label is ink even on the paper theme.
            Style::OnBlue => (text, ON_STATUS),
            Style::Secondary => (text, if pressed { p.text } else { p.soft }),
            Style::Ghost => (text, if pressed { p.text } else { p.muted }),
            Style::Link => (text, if pressed { p.text } else { p.accent }),
        };
        match variant {
            Style::Primary => self.rounded(area, radius, blend(p.accent, WHITE, 0.14 * lift), alpha),
            Style::OnBlue => self.rounded(area, radius, blend(BLUE, WHITE, 0.14 * lift), alpha),
            Style::Secondary => {
                let fill = lerp(tokens::FILL_SUBTLE, tokens::FILL_SOFT, lift);
                self.rounded(area, radius, self.tint(fill), alpha);
            }
            Style::Ghost => {
                if pressed {
                    self.rounded(area, radius, self.tint(tokens::FILL_SUBTLE), alpha * lift);
                }
            }
            Style::Link => {}
        }
        self.text(label, &self.fonts.label_center, area, color, alpha);
    }

    fn rounded(&self, area: D2D_RECT_F, radius: f32, color: D2D1_COLOR_F, alpha: f32) {
        unsafe {
            self.brush.SetColor(&with_alpha(color, alpha));
            self.target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: area, radiusX: radius, radiusY: radius },
                &self.brush,
            );
        }
    }

    /// Dashed line marching around `area`; `offset` keeps advancing during the animation.
    /// The stroke style is reused and only rebuilt when the offset changes.
    fn marching(&self, area: D2D_RECT_F, radius: f32, width: f32, offset: f32, color: D2D1_COLOR_F, alpha: f32) {
        let offset = offset.rem_euclid(5.0);
        let mut cached = self.marching.borrow_mut();
        let stale = cached.as_ref().is_none_or(|(old, _)| (*old - offset).abs() > 0.01);
        if stale {
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_CUSTOM,
                dashOffset: -offset,
            };
            let style = unsafe { self.factory.CreateStrokeStyle(&props, Some(&[2.0, 3.0])) };
            *cached = style.ok().map(|style| (offset, style));
        }
        let Some((_, style)) = cached.as_ref() else { return };
        unsafe {
            self.brush.SetColor(&with_alpha(color, alpha));
            let shape = D2D1_ROUNDED_RECT { rect: area, radiusX: radius, radiusY: radius };
            self.target.DrawRoundedRectangle(&shape, &self.brush, width, style);
        }
    }

    fn outline(&self, area: D2D_RECT_F, radius: f32, width: f32, dashed: bool, color: D2D1_COLOR_F, alpha: f32) {
        let style = if dashed { &self.dashed } else { &self.round };
        unsafe {
            self.brush.SetColor(&with_alpha(color, alpha));
            self.target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: area, radiusX: radius, radiusY: radius },
                &self.brush,
                width,
                style,
            );
        }
    }

    fn circle(&self, cx: f32, cy: f32, radius: f32, color: D2D1_COLOR_F, alpha: f32) {
        unsafe {
            self.brush.SetColor(&with_alpha(color, alpha));
            self.target.FillEllipse(
                &D2D1_ELLIPSE { point: pt(cx, cy), radiusX: radius, radiusY: radius },
                &self.brush,
            );
        }
    }

    fn circle_outline(&self, cx: f32, cy: f32, radius: f32, width: f32, color: D2D1_COLOR_F, alpha: f32) {
        unsafe {
            self.brush.SetColor(&with_alpha(color, alpha));
            self.target.DrawEllipse(
                &D2D1_ELLIPSE { point: pt(cx, cy), radiusX: radius, radiusY: radius },
                &self.brush,
                width,
                None::<&ID2D1StrokeStyle>,
            );
        }
    }

    fn glyph(&self, icon: char, format: &IDWriteTextFormat, cx: f32, cy: f32, color: D2D1_COLOR_F, alpha: f32) {
        let mut buf = [0u16; 2];
        let encoded = icon.encode_utf16(&mut buf);
        self.text_utf16(encoded, format, rect(cx - 16.0, cy - 16.0, cx + 16.0, cy + 16.0), color, alpha);
    }

    fn text(&self, s: &str, format: &IDWriteTextFormat, area: D2D_RECT_F, color: D2D1_COLOR_F, alpha: f32) {
        // Displayed text is short (labels, clock, day names), so the reused buffer almost
        // always fits without allocating.
        let mut wide = self.wide.borrow_mut();
        wide.clear();
        wide.extend(s.encode_utf16());
        self.text_utf16(&wide, format, area, color, alpha);
    }

    fn text_utf16(&self, s: &[u16], format: &IDWriteTextFormat, area: D2D_RECT_F, color: D2D1_COLOR_F, alpha: f32) {
        unsafe {
            if self.pal.glass {
                // Faint shadow so text stays readable over a bright wallpaper.
                let shadow = rect(area.left, area.top + 0.8, area.right, area.bottom + 0.8);
                self.brush.SetColor(&rgb(0x000000, 0.45 * alpha));
                self.target.DrawText(s, format, &shadow, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            self.brush.SetColor(&with_alpha(color, alpha));
            self.target.DrawText(
                s,
                format,
                &area,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Struck-through text, for finished tasks.
    fn text_struck(&self, s: &str, format: &IDWriteTextFormat, area: D2D_RECT_F, color: D2D1_COLOR_F, alpha: f32) {
        let mut buffer = self.wide.borrow_mut();
        buffer.clear();
        buffer.extend(s.encode_utf16());
        let wide = &*buffer;
        unsafe {
            let Ok(layout) = self.dwrite.CreateTextLayout(wide, format, area.right - area.left, area.bottom - area.top)
            else {
                return;
            };
            let _ = layout.SetStrikethrough(true, DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 });
            self.brush.SetColor(&with_alpha(color, alpha));
            self.target.DrawTextLayout(pt(area.left, area.top), &layout, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
    }

    /// Text rolling from the old value to the new one.
    fn rolling_text(&self, text: &str, roll: Option<&Roll>, format: &IDWriteTextFormat, area: D2D_RECT_F, color: D2D1_COLOR_F, alpha: f32) {
        let Some(roll) = roll.filter(|r| r.t < 1.0) else {
            self.text(text, format, area, color, alpha);
            return;
        };
        let eased = 1.0 - (1.0 - roll.t).powi(3);
        let shift = (area.bottom - area.top) * 0.9 * if roll.up { -1.0 } else { 1.0 };
        let moved = |dy: f32| rect(area.left, area.top + dy, area.right, area.bottom + dy);
        self.push_clip(area);
        self.text(&roll.from, format, moved(shift * eased), color, alpha * (1.0 - eased));
        self.text(text, format, moved(-shift * (1.0 - eased)), color, alpha * eased);
        self.pop_clip();
    }

    /// Text width in DIP.
    fn text_width(&self, s: &str, format: &IDWriteTextFormat) -> f32 {
        let mut buffer = self.wide.borrow_mut();
        buffer.clear();
        buffer.extend(s.encode_utf16());
        let wide = &*buffer;
        unsafe {
            let Ok(layout) = self.dwrite.CreateTextLayout(wide, format, 10_000.0, 100.0) else { return 0.0 };
            let mut metrics = DWRITE_TEXT_METRICS::default();
            match layout.GetMetrics(&mut metrics) {
                Ok(()) => metrics.widthIncludingTrailingWhitespace,
                Err(_) => 0.0,
            }
        }
    }

    /// Shift content by (`dx`, `dy`), then scale it by `zoom` around the top center of the
    /// shape.
    fn set_offset(&self, dx: f32, dy: f32) {
        let (s, cx) = self.zoom.get();
        let m = Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: s * dx + cx * (1.0 - s), M32: s * dy };
        unsafe { self.target.SetTransform(&m) };
    }

    /// Start drawing content that grows in from the top: `scale` 1 = original size.
    fn begin_zoom(&self, scale: f32, cx: f32) {
        self.zoom.set((scale, cx));
        self.set_offset(0.0, 0.0);
    }

    fn end_zoom(&self) {
        self.zoom.set((1.0, 0.0));
        self.set_offset(0.0, 0.0);
    }

    /// Draw `paint` scaled by `scale` around (`cx`, `cy`), on top of the current transform
    /// (e.g. a shelf tile appearing or hovered).
    fn scaled(&self, cx: f32, cy: f32, scale: f32, paint: impl FnOnce()) {
        if (scale - 1.0).abs() < 0.001 {
            paint();
            return;
        }
        unsafe {
            let mut current = Matrix3x2::default();
            self.target.GetTransform(&mut current);
            let local = Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: cx * (1.0 - scale), M32: cy * (1.0 - scale) };
            self.target.SetTransform(&(local * current));
            paint();
            self.target.SetTransform(&current);
        }
    }

    fn push_clip(&self, area: D2D_RECT_F) {
        unsafe { self.target.PushAxisAlignedClip(&area, D2D1_ANTIALIAS_MODE_ALIASED) };
    }

    fn pop_clip(&self) {
        unsafe { self.target.PopAxisAlignedClip() };
    }

}

/// Tray icon: a dark square with a pair of eyes, the same mascot as in the shape.
/// Eye dimensions for one place the mascot appears.
#[derive(Clone, Copy)]
struct EyeSize {
    w: f32,
    h: f32,
    gap: f32,
}

impl EyeSize {
    const PILL: EyeSize = EyeSize { w: 5.0, h: 7.0, gap: 9.0 };
    const HEADER: EyeSize = EyeSize { w: 6.0, h: 8.0, gap: 11.0 };
    const CLOCK: EyeSize = EyeSize { w: 18.0, h: 24.0, gap: 30.0 };
}

/// Radius and stroke of a progress ring.
#[derive(Clone, Copy)]
struct Ring {
    radius: f32,
    thickness: f32,
}

impl Ring {
    const PILL: Ring = Ring { radius: 6.5, thickness: 2.2 };
    const BATTERY: Ring = Ring { radius: 5.0, thickness: 2.0 };
    const LARGE: Ring = Ring { radius: 30.0, thickness: 5.0 };
    const WATER: Ring = Ring { radius: 26.0, thickness: 5.0 };
    const STAT: Ring = Ring { radius: 20.0, thickness: 4.0 };
}

/// Page caption on the right of the tab row (clickable when `right_hit` is set). The page's
/// name shows on its tab.
struct Header<'a> {
    right: &'a str,
    right_hit: Option<Hit>,
}

pub fn make_icon(size: i32) -> Result<HICON> {
    let surface = icon_surface(size)?;
    unsafe {
        // The bitmap must be deselected from the DC before it can build an icon.
        SelectObject(surface.dc, surface.old);
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: surface.bitmap,
        };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// Mascot icon sized `size`×`size`, with straight alpha as Windows icons use.
/// Used by the tray icon and, through a test, the `packaging/crest.ico` file for crest.exe.
fn icon_surface(size: i32) -> Result<Surface> {
    let s = size as f32;
    unsafe {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let target = dc_target(&factory, 96.0)?;
        let mut surface = Surface::new(size, size)?;
        target.BindDC(surface.dc, &RECT { left: 0, top: 0, right: size, bottom: size })?;
        target.BeginDraw();
        target.Clear(Some(&CLEAR));
        let brush = target.CreateSolidColorBrush(&ICON_BG, None)?;
        let corner = (s * 0.3).round();
        target.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT { rect: rect(0.0, 0.0, s, s), radiusX: corner, radiusY: corner },
            &brush,
        );
        brush.SetColor(&ICON_EYE);
        // Eyes: slightly tall ovals on whole-pixel boxes, mirrored around the center.
        let (eye_w, eye_h) = ((s * 0.19).round().max(3.0), (s * 0.27).round().max(4.0));
        let gap = (s * 0.15).round().max(2.0);
        let top = ((s - eye_h) / 2.0 + s * 0.04).round();
        let left = ((s - 2.0 * eye_w - gap) / 2.0).round();
        for x in [left, left + eye_w + gap] {
            target.FillEllipse(
                &D2D1_ELLIPSE {
                    point: pt(x + eye_w / 2.0, top + eye_h / 2.0),
                    radiusX: eye_w / 2.0,
                    radiusY: eye_h / 2.0,
                },
                &brush,
            );
        }
        target.EndDraw(None, None)?;
        let _ = GdiFlush();

        // Windows icons use straight alpha, while Direct2D produces premultiplied alpha.
        for px in surface.pixels() {
            let a = *px >> 24;
            if a != 0 && a != 255 {
                let un = |c: u32| ((c * 255 + a / 2) / a).min(255);
                *px = (a << 24) | (un((*px >> 16) & 0xFF) << 16) | (un((*px >> 8) & 0xFF) << 8) | un(*px & 0xFF);
            }
        }
        Ok(surface)
    }
}

/// In-memory 32-bit (BGRA) bitmap that serves as the window's drawing canvas.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    width: i32,
    height: i32,
}

impl Surface {
    fn new(width: i32, height: i32) -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // negative = first row on top
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) => bitmap,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let old = SelectObject(dc, bitmap.into());
            Ok(Self { dc, bitmap, old, bits: bits.cast(), width, height })
        }
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

fn dc_target(factory: &ID2D1Factory, dpi: f32) -> Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        // Software: does not load the GPU driver, plenty for a shape this small.
        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: dpi,
        dpiY: dpi,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe {
        let target = factory.CreateDCRenderTarget(&props)?;
        // ClearType cannot be used over a transparent background.
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        Ok(target)
    }
}

/// Windows 11 has Segoe Fluent Icons; Windows 10 uses Segoe MDL2 Assets with the same code points.
fn icon_family(dwrite: &IDWriteFactory) -> PCWSTR {
    let fluent = w!("Segoe Fluent Icons");
    unsafe {
        let mut collection = None;
        if dwrite.GetSystemFontCollection(&mut collection, false).is_ok()
            && let Some(collection) = collection
        {
            let (mut index, mut exists) = (0u32, BOOL(0));
            if collection.FindFamilyName(fluent, &mut index, &mut exists).is_ok() && exists.as_bool() {
                return fluent;
            }
        }
    }
    w!("Segoe MDL2 Assets")
}

fn text_format(
    dwrite: &IDWriteFactory,
    family: PCWSTR,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
    align: DWRITE_TEXT_ALIGNMENT,
    paragraph: DWRITE_PARAGRAPH_ALIGNMENT,
    trim: bool,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let format = dwrite.CreateTextFormat(
            family,
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-US"),
        )?;
        format.SetTextAlignment(align)?;
        format.SetParagraphAlignment(paragraph)?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        if trim {
            // Text that is too long is trimmed with "…".
            let sign = dwrite.CreateEllipsisTrimmingSign(&format)?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            format.SetTrimming(&trimming, &sign)?;
        }
        Ok(format)
    }
}

fn canvas_px(dpi: u32) -> (i32, i32) {
    let scale = dpi as f32 / 96.0;
    ((CANVAS.0 * scale).ceil() as i32, (CANVAS.1 * scale).ceil() as i32)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn translation(x: f32, y: f32) -> Matrix3x2 {
    Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: x, M32: y }
}

fn pt(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F { left, top, right, bottom }
}

fn arc(x: f32, y: f32, radius: f32, sweep: D2D1_SWEEP_DIRECTION, size: D2D1_ARC_SIZE) -> D2D1_ARC_SEGMENT {
    D2D1_ARC_SEGMENT {
        point: pt(x, y),
        size: D2D_SIZE_F { width: radius, height: radius },
        rotationAngle: 0.0,
        sweepDirection: sweep,
        arcSize: size,
    }
}

const fn rgb(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a,
    }
}

fn with_alpha(color: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * alpha, ..color }
}

/// Mix of two colors: `t` = 0 is `from`, 1 is `to`.
fn blend(from: D2D1_COLOR_F, to: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: lerp(from.r, to.r, t), g: lerp(from.g, to.g, t), b: lerp(from.b, to.b, t), a: lerp(from.a, to.a, t) }
}

/// Album cover accent color, or `fallback` if the cover has no color.
fn accent(color: Option<u32>, fallback: D2D1_COLOR_F) -> D2D1_COLOR_F {
    color.map_or(fallback, |hex| rgb(hex, 1.0))
}

fn phase_color(phase: Phase) -> D2D1_COLOR_F {
    match phase {
        Phase::Focus => RED,
        Phase::Break => GREEN,
    }
}

/// Load color: accent when calm, orange when busy, red when nearly full.
fn load_color(percent: f32, calm: D2D1_COLOR_F) -> D2D1_COLOR_F {
    if percent >= 85.0 {
        RED
    } else if percent >= 60.0 {
        ORANGE
    } else {
        calm
    }
}

#[cfg(test)]
mod tests;
