//! Timing, sizes, and ids used across the app module.

use super::*;

pub(super) const CLASS: PCWSTR = w!("CrestWindow");
/// Defined in the large `Win32_UI_Controls` feature; this one constant is enough.
pub(super) const WM_MOUSELEAVE: u32 = 0x02A3;
/// Start dragging a file out of the shelf, outside the app state (see `wndproc`).
pub(super) const WM_START_DRAG: u32 = WM_APP + 6;

pub(super) const TIMER_CLOCK: usize = 1;
pub(super) const TIMER_OPEN: usize = 2;
pub(super) const TIMER_CLOSE: usize = 3;
pub(super) const TIMER_BLINK: usize = 4;
pub(super) const TIMER_DESKTOP: usize = 5;
pub(super) const TIMER_HUD: usize = 6;
/// Music waveform, track progress, and the focus countdown.
pub(super) const TIMER_AMBIENT: usize = 7;
/// Light check for maximized windows, which have no event of their own.
pub(super) const TIMER_POLL: usize = 8;
/// A focus phase ends.
pub(super) const TIMER_FOCUS: usize = 9;
/// Shortly after Caps/Num Lock is released, once its light state has changed.
pub(super) const TIMER_LOCK_KEYS: usize = 10;
/// The mascot's eyes glance toward the cursor.
pub(super) const TIMER_GAZE: usize = 11;
/// The mascot's temporary expression (happy, surprised) ends.
pub(super) const TIMER_MOOD: usize = 12;
/// Quick timer finished.
pub(super) const TIMER_ALARM: usize = 13;
/// Refresh the weather.
pub(super) const TIMER_WEATHER: usize = 14;
/// Quiet mode ends.
pub(super) const TIMER_QUIET: usize = 15;
/// First-run welcome appears after the mascot has woken up.
pub(super) const TIMER_WELCOME: usize = 16;
/// Calendar feeds are fetched again (they change slowly and the address is a secret).
pub(super) const TIMER_CALENDARS: usize = 17;
/// Mail and calendar are refreshed while their pages (or Home) are in use.
pub(super) const TIMER_GOOGLE: usize = 19;
pub(super) const WELCOME_DELAY_MS: u32 = 1_300;
/// Result of the Administrator step for messages (see `identity`): 0 success, 1 failure,
/// 2 Administrator permission denied.
pub(super) const WM_IDENTITY: u32 = WM_APP + 7;
/// Id of the Win+Alt+N hotkey.
pub(super) const HOTKEY_ID: i32 = 1;

pub(super) const POLL_MS: u32 = 1_000;
/// Delay before opening, so it does not open when the cursor merely passes by on its way to a browser tab.
pub(super) const OPEN_DELAY_MS: u32 = 200;
pub(super) const CLOSE_DELAY_MS: u32 = 380;
/// After a file drop, the panel stays open briefly so the result is visible.
pub(super) const DROP_CLOSE_MS: u32 = 2_500;
/// Wait for windows to stop moving before checking full-screen/maximize.
pub(super) const DESKTOP_SETTLE_MS: u32 = 60;
pub(super) const BLINK_SECS: f64 = 0.16;
pub(super) const PAGE_ANIM_SECS: f64 = 0.24;
/// Size difference (DIP) that is no longer visible; below it the shape animation counts as done.
pub(super) const SHAPE_PRECISION: f32 = 0.05;
/// Opening bounces slightly; closing is smooth with no bounce.
pub(super) const OPEN_BOUNCE: f32 = 0.72;
pub(super) const CLOSE_BOUNCE: f32 = 1.0;
/// Shelf animations: new tiles appear, removed tiles fade out, glow after a file drop.
pub(super) const TILE_IN_SECS: f64 = 0.4;
pub(super) const TILE_OUT_SECS: f64 = 0.22;
pub(super) const GLOW_SECS: f64 = 0.7;
/// Time constant of the hover highlight easing in (about 150 ms to settle).
pub(super) const HOVER_TAU: f32 = 0.05;
/// How long HUD numbers take to roll to a new value.
pub(super) const ROLL_SECS: f64 = 0.22;
/// The drop area's dashed outline marches while a file is dragged (~25 frames per second).
pub(super) const DASH_MS: u32 = 40;
/// The pill grows slightly as soon as the cursor touches it, before the panel opens.
pub(super) const PEEK_GROW: (f32, f32) = (12.0, 4.0);

pub(super) const VOLUME_HUD_MS: u32 = 1_600;
pub(super) const POWER_HUD_MS: u32 = 3_000;
pub(super) const BLUETOOTH_HUD_MS: u32 = 3_500;
pub(super) const FOCUS_HUD_MS: u32 = 6_000;
pub(super) const WATER_HUD_MS: u32 = 6_000;
pub(super) const PRIVACY_HUD_MS: u32 = 3_500;
pub(super) const BRIGHTNESS_HUD_MS: u32 = 1_600;
pub(super) const LOCK_KEY_HUD_MS: u32 = 1_400;
pub(super) const MESSAGE_HUD_MS: u32 = 5_000;
/// A new unread mail notice.
pub(super) const MAIL_HUD_MS: u32 = 5_000;
pub(super) const SCREENSHOT_HUD_MS: u32 = 4_000;
pub(super) const AGENT_HUD_MS: u32 = 6_000;
pub(super) const DOWNLOAD_HUD_MS: u32 = 4_500;
pub(super) const WIFI_HUD_MS: u32 = 3_000;
/// A panel opened by hotkey closes on its own if the cursor never arrives.
pub(super) const HOTKEY_CLOSE_MS: u32 = 5_000;
/// How often the cursor position is sampled while the mascot's eyes are visible; less often
/// once the cursor has been still for a while.
pub(super) const GAZE_MS: u32 = 200;
pub(super) const GAZE_IDLE_MS: u32 = 1_000;
/// Cursor still for this long: the eyes look straight ahead again.
pub(super) const GAZE_REST: Duration = Duration::from_secs(30);
/// How long a temporary expression lasts after its HUD is gone.
pub(super) const MOOD_MS: u32 = 5_000;
pub(super) const TIMER_HUD_MS: u32 = 8_000;
pub(super) const AGENDA_HUD_MS: u32 = 8_000;
/// Prayer reminder and azan notices stay a little longer: they are easy to miss mid-task.
pub(super) const PRAYER_HUD_MS: u32 = 10_000;
/// Two-finger swipe distance on the pill (mouse wheel units) to change track, and the minimum
/// gap between changes so one gesture does not skip several tracks.
pub(super) const SWIPE_DISTANCE: i32 = 240;
pub(super) const SWIPE_COOLDOWN: f64 = 0.7;
/// The pill widens to the right during a download, to fit the size being downloaded.
pub(super) const DOWNLOAD_GROW: f32 = 60.0;
/// Mascot wake-up: eyes stay closed briefly, then open slowly.
pub(super) const WAKE_CLOSED_SECS: f64 = 0.3;
pub(super) const WAKE_OPEN_SECS: f64 = 0.45;
pub(super) const WEATHER_EVERY_MS: u32 = 30 * 60 * 1_000;
/// Mail and Google Calendar are refreshed every few minutes; the API quota is per account.
pub(super) const GOOGLE_REFRESH: Duration = Duration::from_secs(5 * 60);
/// Weather older than this is no longer shown.
pub(super) const WEATHER_STALE: Duration = Duration::from_secs(3 * 60 * 60);
pub(super) const QUIET: Duration = Duration::from_secs(60 * 60);
/// Maximum length of a typed city name.
pub(super) const CITY_MAX: usize = 40;
/// Maximum length of a typed Google client id or client secret.
pub(super) const GOOGLE_ID_MAX: usize = 400;
/// The pill waveform redraws ~12 times per second: lively enough, with almost no CPU cost.
pub(super) const WAVE_MS: u32 = 80;
pub(super) const PROGRESS_MS: u32 = 1_000;
/// How long the lyrics column takes to scroll to the next line.
pub(super) const LYRIC_SCROLL_SECS: f64 = 0.38;
/// Countdown on the focus page; finer than a second so the digits do not jump.
pub(super) const FOCUS_TICK_MS: u32 = 500;
/// The focus ring on the pill only needs occasional updates.
pub(super) const PILL_FOCUS_MS: u32 = 5_000;
/// Warn when the battery drops past these levels without a charger.
pub(super) const LOW_BATTERY: [u32; 2] = [20, 10];
/// Water reminder: every hour after the last drink, repeated every 30 minutes until you drink.
pub(super) const WATER_EVERY: Duration = Duration::from_secs(60 * 60);
pub(super) const WATER_AGAIN: Duration = Duration::from_secs(30 * 60);
/// Active hours of the water reminder (07:00–21:59).
pub(super) const WATER_HOURS: Range<u16> = 7..22;
