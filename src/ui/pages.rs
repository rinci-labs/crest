//! Layout of the shape's content: pill, HUD, and panel pages. All coordinates are in DIP
//! relative to the canvas; `x0`/`x1` are the current left/right edges of the shape body.

use super::*;
use crate::feeds::agents::State as AgentState;
use crate::system::stats::{gigabytes, rate};
use crate::tools::shelf::VISIBLE as SHELF_VISIBLE;
use crate::tools::tasks::VISIBLE as TASKS_VISIBLE;
use crate::tools::prayer;
use crate::tools::water::{GLASS_ML, liters};

/// Panel content inset from its edges; the rest is for the page arrows.
const MARGIN: f32 = tokens::INSET;
/// Room kept on the right of the tab row for the page's caption, as a share of the panel's inner
/// width and within these bounds. A single fixed number either starved the tabs on a normal
/// panel or wasted space on the wide Home, so it scales with the room actually available.
const CAPTION_SHARE: f32 = 0.20;
const CAPTION_MIN: f32 = 76.0;
const CAPTION_MAX: f32 = 168.0;

/// Width reserved for the page caption in a panel of `inner` DIP, clamped to the bounds above.
fn caption_reserve(inner: f32) -> f32 {
    (inner * CAPTION_SHARE).clamp(CAPTION_MIN, CAPTION_MAX)
}
const GUTTER: f32 = tokens::GUTTER;
const LYRIC_CENTER: f32 = tokens::LYRIC_CENTER;
const LYRIC_GAP: f32 = tokens::LYRIC_GAP;
const COLUMN_SLIDE: f32 = tokens::COLUMN_SLIDE;
const HEADER_Y: f32 = tokens::HEADER_Y;
/// The tab row's center line in page coordinates (pages are drawn `CONTENT_SHIFT` lower).
const TOP_Y: f32 = tokens::TABS_Y - tokens::CONTENT_SHIFT;
const TABS_Y: f32 = tokens::TABS_Y;
const ROW_Y: f32 = tokens::ROW_Y;
const ROW_H: f32 = tokens::ROW_H;
const AGENDA_ROW_H: f32 = tokens::AGENDA_ROW_H;
const CONTROL_H: f32 = tokens::CONTROL_H;
const TIGHT: f32 = tokens::TIGHT;
const TILE_H: f32 = tokens::TILE_H;
const TILE_PITCH: f32 = tokens::TILE_PITCH;
const PAGE_SLIDE: f32 = tokens::PAGE_SLIDE;
/// Controls page: capsule slider height, and the toggle tile row.
const SLIDER_H: f32 = 26.0;
const CONTROL_TILE_H: f32 = 40.0;
/// Gap between the sliders, and between the last slider and the tiles.
const SLIDER_GAP: f32 = 8.0;
const TILES_GAP: f32 = 8.0;
/// Room a tab's name adds after its icon slot: the name, then the same padding on its right as
/// the icon has on its left.
const TAB_LABEL_PAD: f32 = 6.0;
/// Centre of the first task row: its hover plate starts on the page padding.
const TASKS_TOP: f32 = tokens::DECK_TOP + PAD + 12.0;
/// Top of the shelf's tile row, on the page padding.
const SHELF_TOP: f32 = tokens::DECK_TOP + PAD + 2.0;
/// Gap between the prayer time tiles.
const PRAYER_GAP: f32 = 6.0;
/// Center lines of the Settings rows (theme, two switch rows, prayer, agent hooks) on the
/// taller Settings panel: one 34 DIP rhythm, with the switch rows paired a little closer.
const SETTINGS_ROWS: [f32; 5] = [57.0, 91.0, 121.0, 155.0, 189.0];
const HERO_Y: f32 = tokens::HERO_Y;
/// Music player: cover, progress bar, and button row.
const ART_TOP: f32 = 36.0;
/// (The cover is drawn at `ART_TOP + 8`, 14 DIP inside the deck.)
const ART_SIZE: f32 = 52.0;
const PROGRESS_Y: f32 = 108.0;
const CONTROLS_Y: f32 = ROW_Y - 2.0;

impl Renderer {
    pub(super) fn draw_pill(&self, s: &Scene, x0: f32, x1: f32, h: f32, a: f32) {
        let p = &self.pal;
        let cy = h / 2.0;
        let playing = s.media.filter(|m| m.playing);
        let focus = Some(s.focus).filter(|f| f.running);
        let timer = Some(s.timer).filter(|t| t.running);
        // Left: timer or focus ring, song cover, or mascot eyes.
        if let Some(t) = timer {
            self.ring(x0 + 18.0, cy, Ring::PILL, t.progress, ORANGE, a);
        } else if let Some(f) = focus {
            self.ring(x0 + 18.0, cy, Ring::PILL, f.progress, phase_color(f.phase), a);
        } else if playing.is_some() {
            self.art(x0 + 9.0, cy - 9.0, 18.0, 5.0, a);
        } else {
            self.eyes(x0 + 18.0, cy, EyeSize::PILL, s.eyes, a);
        }
        // Right: music wave, timer time left, running download, or focus minutes left; the
        // privacy dot at the far right. The pill widens during a download, the clock shifts left.
        let download = s.download.filter(|_| playing.is_none() && timer.is_none());
        match (playing, timer, download, focus) {
            (Some(m), ..) => self.wave(x1 - 22.0, cy, s.seconds, accent(m.accent, p.text), a),
            (None, Some(t), ..) => {
                self.text(&t.short, &self.fonts.caption_end, rect(x1 - 52.0, 0.0, x1 - 13.0, h), ORANGE, a);
            }
            (None, None, Some(size), _) => {
                self.glyph(ICON_DOWNLOAD, &self.fonts.icon_11, x1 - 20.0, cy, p.accent, a);
                self.text(size, &self.fonts.caption_end, rect(x1 - 100.0, 0.0, x1 - 31.0, h), p.soft, a);
            }
            (None, None, None, Some(f)) => {
                let left = format!("{}m", f.minutes_left);
                self.text(&left, &self.fonts.caption_end, rect(x1 - 52.0, 0.0, x1 - 13.0, h), phase_color(f.phase), a);
            }
            (None, None, None, None) => {}
        }
        self.privacy_dot(s.privacy, x1 - 8.0, cy, a);
        let time_right = if download.is_some() { x1 - 100.0 } else { x1 - 34.0 };
        self.text(s.time, &self.fonts.pill_time, rect(x0 + 34.0, 0.0, time_right, h), p.text, a);
    }

    /// Wide pill while lyrics are on: cover, lyric line, and wave.
    pub(super) fn draw_lyric_pill(&self, s: &Scene, x0: f32, x1: f32, h: f32, a: f32) {
        let p = &self.pal;
        let cy = h / 2.0;
        self.art(x0 + 9.0, cy - 9.0, 18.0, 5.0, a);
        if let Some(line) = s.lyric {
            // A taller pill means the line is too long for a single row.
            if h > View::LyricPill.size().1 + 4.0 {
                self.text(line, &self.fonts.lyric_wrap, rect(x0 + 40.0, 3.0, x1 - 40.0, h - 3.0), p.text, a);
            } else {
                self.text(line, &self.fonts.lyric, rect(x0 + 36.0, 0.0, x1 - 36.0, h), p.text, a);
            }
        }
        let color = s.media.and_then(|m| m.accent);
        self.wave(x1 - 22.0, cy, s.seconds, accent(color, p.text), a);
        self.privacy_dot(s.privacy, x1 - 8.0, cy, a);
    }

    pub(super) fn draw_hud(&self, s: &Scene, hud: &Hud, x0: f32, x1: f32, h: f32, a: f32) {
        // Notice content grows in slightly from the top, like the panel.
        self.begin_zoom(0.9 + 0.1 * a, (x0 + x1) / 2.0);
        if s.hud_compact {
            self.draw_hud_compact(s, hud, x0, x1, h, a);
        } else {
            self.draw_hud_card(s, hud, x0, x1, a);
        }
        self.end_zoom();
    }

    /// Compact pill-height notice: icon and name on the left, bar/value on the right.
    fn draw_hud_compact(&self, s: &Scene, hud: &Hud, x0: f32, x1: f32, h: f32, a: f32) {
        let p = &self.pal;
        let cy = h / 2.0;
        let row = |left: f32, right: f32| rect(left, cy - 9.0, right, cy + 9.0);
        let value = row(x1 - 56.0, x1 - 14.0);
        match hud {
            Hud::Volume { level, muted } => {
                let icon = if *muted { ICON_MUTE } else { volume_icon(*level) };
                self.glyph(icon, &self.fonts.icon_13, x0 + 22.0, cy, p.text, a);
                self.text("Volume", &self.fonts.label, row(x0 + 38.0, x1 - 140.0), p.text, a);
                self.bar(x1 - 132.0, x1 - 62.0, cy, s.hud_fill, if *muted { p.muted } else { p.text }, a);
                let text = hud_value(hud).unwrap_or_default();
                self.rolling_text(&text, s.hud_roll, &self.fonts.label_end, value, p.text, a);
            }
            Hud::Brightness { .. } => {
                self.glyph(ICON_BRIGHTNESS, &self.fonts.icon_13, x0 + 22.0, cy, p.text, a);
                self.text("Brightness", &self.fonts.label, row(x0 + 38.0, x1 - 140.0), p.text, a);
                self.bar(x1 - 132.0, x1 - 62.0, cy, s.hud_fill, p.text, a);
                let text = hud_value(hud).unwrap_or_default();
                self.rolling_text(&text, s.hud_roll, &self.fonts.label_end, value, p.text, a);
            }
            Hud::LockKey { caps, on } => {
                let key = rect(x0 + 13.0, cy - 8.5, x0 + 31.0, cy + 8.5);
                if *on {
                    self.rounded(key, tokens::RADIUS_KEY, p.accent, a);
                } else {
                    self.outline(key, tokens::RADIUS_KEY, 1.2, false, p.muted, a);
                }
                self.text(if *caps { "A" } else { "1" }, &self.fonts.label_center, key, if *on { WHITE } else { p.muted }, a);
                let name = if *caps { "Caps Lock" } else { "Num Lock" };
                self.text(name, &self.fonts.label, row(x0 + 40.0, x1 - 70.0), p.text, a);
                let state = if *on { "On" } else { "Off" };
                self.text(state, &self.fonts.label_end, row(x1 - 70.0, x1 - 14.0), if *on { p.accent } else { p.muted }, a);
            }
            Hud::Power { plugged, percent, low } => {
                let (icon, color, label) = power_look(*plugged, *percent, *low, p.text);
                self.glyph(icon, &self.fonts.icon_16, x0 + 24.0, cy, color, a);
                self.text(label, &self.fonts.label, row(x0 + 42.0, x1 - 60.0), p.text, a);
                if let Some(percent) = percent {
                    self.text(&format!("{percent}%"), &self.fonts.label_end, row(x1 - 60.0, x1 - 14.0), color, a);
                }
            }
            // Other notices always use the card layout.
            _ => self.draw_hud_card(s, hud, x0, x1, a),
        }
    }

    fn draw_hud_card(&self, s: &Scene, hud: &Hud, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let cy = 22.0;
        let row = |left: f32, right: f32| rect(left, cy - 10.0, right, cy + 10.0);
        match hud {
            Hud::Volume { level, muted } => {
                let icon = if *muted { ICON_MUTE } else { volume_icon(*level) };
                self.glyph(icon, &self.fonts.icon_16, x0 + 27.0, cy, p.text, a);
                self.bar(x0 + 50.0, x1 - 62.0, cy, s.hud_fill, if *muted { p.muted } else { p.text }, a);
                let value = hud_value(hud).unwrap_or_default();
                self.rolling_text(&value, s.hud_roll, &self.fonts.label_end, row(x1 - 70.0, x1 - 20.0), p.text, a);
            }
            Hud::Power { plugged, percent, low } => {
                let (icon, color, label) = power_look(*plugged, *percent, *low, p.text);
                self.glyph(icon, &self.fonts.icon_20, x0 + 28.0, cy, color, a);
                self.text(label, &self.fonts.label, row(x0 + 52.0, x1 - 80.0), p.text, a);
                if let Some(percent) = percent {
                    self.text(&format!("{percent}%"), &self.fonts.label_end, row(x1 - 80.0, x1 - 20.0), color, a);
                }
            }
            Hud::Bluetooth { name, connected, battery, kind, .. } => {
                self.circle(x0 + 28.0, cy, 13.0, if *connected { BLUE } else { p.dim }, a);
                self.glyph(ICON_BLUETOOTH, &self.fonts.icon_13, x0 + 28.0, cy, WHITE, a);
                self.text(name, &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 52.0, 22.0), p.text, a);
                let status = match (*connected, battery) {
                    (true, Some(b)) => format!("Connected · {b}%"),
                    (true, None) => "Connected".to_string(),
                    (false, _) => "Disconnected".to_string(),
                };
                self.text(&status, &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 52.0, 37.0), p.muted, a);
                let device = match kind {
                    DeviceKind::Audio => ICON_HEADPHONE,
                    DeviceKind::Input => ICON_KEYBOARD,
                    DeviceKind::Phone => ICON_PHONE,
                    DeviceKind::Other => ICON_BLUETOOTH,
                };
                self.glyph(device, &self.fonts.icon_16, x1 - 28.0, cy, p.muted, a);
            }
            Hud::Focus { finished } => {
                // The color marks the phase that starts next.
                let (color, label, value) = match finished {
                    Phase::Focus => (GREEN, "Focus done, take a break", "5 min"),
                    Phase::Break => (RED, "Break over", "Ready to focus"),
                };
                self.glyph(ICON_STOPWATCH, &self.fonts.icon_16, x0 + 27.0, cy, color, a);
                self.text(label, &self.fonts.label, row(x0 + 50.0, x1 - 90.0), p.text, a);
                self.text(value, &self.fonts.caption_end, row(x1 - 100.0, x1 - 20.0), color, a);
            }
            Hud::Water { ml, goal } => {
                self.drop_icon(x0 + 27.0, cy, 16.0, BLUE, a);
                self.text("Time to drink water", &self.fonts.label, row(x0 + 50.0, x1 - 100.0), p.text, a);
                let value = format!("{} / {} L", liters(*ml), liters(*goal));
                self.text(&value, &self.fonts.caption_end, row(x1 - 110.0, x1 - 20.0), p.muted, a);
            }
            Hud::Privacy { camera, app } => {
                let (icon, color, label) = if *camera {
                    (ICON_CAMERA, GREEN, "Camera in use")
                } else {
                    (ICON_MICROPHONE, ORANGE, "Microphone in use")
                };
                self.circle(x0 + 28.0, cy, 13.0, color, a);
                self.glyph(icon, &self.fonts.icon_13, x0 + 28.0, cy, WHITE, a);
                self.text(label, &self.fonts.label, row(x0 + 50.0, x1 - 110.0), p.text, a);
                self.text(app, &self.fonts.caption_end, row(x1 - 120.0, x1 - 20.0), p.muted, a);
            }
            Hud::Brightness { .. } => {
                self.glyph(ICON_BRIGHTNESS, &self.fonts.icon_16, x0 + 27.0, cy, p.text, a);
                self.bar(x0 + 50.0, x1 - 62.0, cy, s.hud_fill, p.text, a);
                let value = hud_value(hud).unwrap_or_default();
                self.rolling_text(&value, s.hud_roll, &self.fonts.label_end, row(x1 - 70.0, x1 - 20.0), p.text, a);
            }
            Hud::LockKey { caps, on } => {
                // Small keycap with "A" (Caps Lock) or "1" (Num Lock), lit while on.
                let key = rect(x0 + 17.0, cy - 10.0, x0 + 39.0, cy + 10.0);
                if *on {
                    self.rounded(key, tokens::RADIUS_KEY, p.accent, a);
                } else {
                    self.outline(key, tokens::RADIUS_KEY, 1.3, false, p.muted, a);
                }
                self.text(if *caps { "A" } else { "1" }, &self.fonts.label_center, key, if *on { WHITE } else { p.muted }, a);
                let name = if *caps { "Caps Lock" } else { "Num Lock" };
                self.text(name, &self.fonts.label, row(x0 + 52.0, x1 - 80.0), p.text, a);
                let state = if *on { "On" } else { "Off" };
                self.text(state, &self.fonts.label_end, row(x1 - 80.0, x1 - 20.0), if *on { p.accent } else { p.muted }, a);
            }
            Hud::Message { app, aumid, sender, text, .. } => {
                self.logo(aumid, x0 + 28.0, cy, 26.0, a);
                self.text(sender, &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 70.0, 22.0), p.text, a);
                self.text(app, &self.fonts.caption_end, rect(x1 - 90.0, 6.0, x1 - 20.0, 22.0), p.muted, a);
                self.text(text, &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 20.0, 37.0), p.soft, a);
            }
            Hud::Mail { from, subject, initials, more } => {
                // The sender's initials in a circle stand in for a logo: mail has no app icon.
                self.circle(x0 + 28.0, cy, 13.0, BLUE, a);
                self.text(initials, &self.fonts.label_center, rect(x0 + 15.0, cy - 9.0, x0 + 41.0, cy + 9.0), ON_STATUS, a);
                self.text(from, &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 70.0, 22.0), p.text, a);
                let right = if *more > 0 { format!("+{more}") } else { "Gmail".to_string() };
                self.text(&right, &self.fonts.caption_end, rect(x1 - 70.0, 6.0, x1 - 20.0, 22.0), p.muted, a);
                self.text(subject, &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 20.0, 37.0), p.soft, a);
            }
            Hud::Screenshot { id, name, more } => {
                self.thumb(*id, false, x0 + 30.0, cy - 15.0, 30.0, a);
                let title = if *more > 0 { format!("{} screenshots added to shelf", more + 1) } else { "Screenshot added to shelf".to_string() };
                self.text(&title, &self.fonts.label, rect(x0 + 54.0, 6.0, x1 - 20.0, 22.0), p.text, a);
                self.text(name, &self.fonts.caption, rect(x0 + 54.0, 22.0, x1 - 20.0, 37.0), p.muted, a);
            }
            Hud::Download { name, more, .. } => {
                self.circle(x0 + 28.0, cy, 13.0, BLUE, a);
                self.glyph(ICON_DOWNLOAD, &self.fonts.icon_13, x0 + 28.0, cy, WHITE, a);
                self.text("Download complete", &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 20.0, 22.0), p.text, a);
                let detail = if *more > 0 { format!("{name} +{more} more") } else { name.clone() };
                self.text(&detail, &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 20.0, 37.0), p.muted, a);
            }
            Hud::Wifi { name, connected, internet } => {
                self.circle(x0 + 28.0, cy, 13.0, if *connected { BLUE } else { p.dim }, a);
                self.glyph(ICON_WIFI, &self.fonts.icon_13, x0 + 28.0, cy, WHITE, a);
                let title = if name.is_empty() { "Wi-Fi" } else { name.as_str() };
                self.text(title, &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 20.0, 22.0), p.text, a);
                let (status, color) = match (*connected, *internet) {
                    (true, true) => ("Connected", p.muted),
                    (true, false) => ("Connected · no internet access", ORANGE),
                    (false, _) => ("Disconnected", p.muted),
                };
                self.text(status, &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 20.0, 37.0), color, a);
            }
            Hud::Agenda { text, time } => {
                self.circle(x0 + 28.0, cy, 13.0, p.accent, a);
                self.glyph(ICON_CALENDAR, &self.fonts.icon_13, x0 + 28.0, cy, WHITE, a);
                self.text(text, &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 76.0, 22.0), p.text, a);
                self.text("Happening now", &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 20.0, 37.0), p.muted, a);
                self.text(time, &self.fonts.label_end, rect(x1 - 76.0, 6.0, x1 - 20.0, 22.0), p.accent, a);
            }
            Hud::Prayer { name, time, ahead } => {
                // Green when the azan is now, the accent for the reminder before it.
                let (fill, on) = if *ahead == 0 { (GREEN, ON_STATUS) } else { (p.accent, p.on_accent) };
                self.circle(x0 + 28.0, cy, 13.0, fill, a);
                self.glyph(ICON_PRAYER, &self.fonts.icon_13, x0 + 28.0, cy, on, a);
                let title = if *ahead == 0 { format!("Time for {name}") } else { format!("{name} in {ahead} min") };
                self.text(&title, &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 76.0, 22.0), p.text, a);
                let line = if *ahead == 0 { "The azan is now" } else { "Time to get ready" };
                self.text(line, &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 20.0, 37.0), p.muted, a);
                self.text(time, &self.fonts.label_end, rect(x1 - 76.0, 6.0, x1 - 20.0, 22.0), fill, a);
            }
            Hud::Timer { minutes } => {
                self.glyph(ICON_TIMER, &self.fonts.icon_16, x0 + 27.0, cy, ORANGE, a);
                self.text("Timer done", &self.fonts.label, row(x0 + 50.0, x1 - 100.0), p.text, a);
                self.text(&format!("{minutes} min"), &self.fonts.caption_end, row(x1 - 100.0, x1 - 20.0), ORANGE, a);
            }
            Hud::Agent { kind, title, project, detail, needs_input, .. } => {
                let color = if *needs_input { ORANGE } else { p.accent };
                self.circle(x0 + 28.0, cy, 13.0, color, a);
                // Both fills are light (orange, accent), so the glyph is ink for contrast.
                self.glyph(if *needs_input { ICON_MESSAGE } else { ICON_CHECK }, &self.fonts.icon_13, x0 + 28.0, cy, INK, a);
                let headline = if *needs_input { format!("{} needs you", kind.label()) } else { format!("{} finished", kind.label()) };
                self.text(&headline, &self.fonts.label, rect(x0 + 50.0, 6.0, x1 - 110.0, 22.0), p.text, a);
                self.text(project, &self.fonts.caption_end, rect(x1 - 120.0, 6.0, x1 - 20.0, 22.0), p.muted, a);
                let line = if *needs_input && !detail.is_empty() { format!("{title} · {detail}") } else { title.clone() };
                self.text(&line, &self.fonts.caption, rect(x0 + 50.0, 22.0, x1 - 20.0, 37.0), p.soft, a);
            }
        }
    }

    pub(super) fn draw_panel(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        // Panel content grows in from 80% anchored at the top, while moving down
        // slightly.
        self.begin_zoom(0.8 + 0.2 * a, (x0 + x1) / 2.0);
        let dy = (1.0 - a) * -6.0;
        if s.welcome {
            self.set_offset(0.0, dy);
            self.welcome_page(s, x0, x1, a);
            self.end_zoom();
            return;
        }
        match s.page_from {
            Some((from, t, dir)) => {
                let eased = 1.0 - (1.0 - t).powi(3);
                self.page(s, from, x0, x1, a * (1.0 - eased), (-dir * PAGE_SLIDE * eased, dy));
                self.page(s, s.page, x0, x1, a * eased, (dir * PAGE_SLIDE * (1.0 - eased), dy));
            }
            None => self.page(s, s.page, x0, x1, a, (0.0, dy)),
        }
        // On the lock screen only Home (clock/music) may be opened.
        if !s.locked {
            self.navigation(s, x0, x1, a, dy);
        }
        self.end_zoom();
    }

    fn page(&self, s: &Scene, page: Page, x0: f32, x1: f32, a: f32, (dx, dy): (f32, f32)) {
        if a < 0.01 {
            return;
        }
        let dy = dy + tokens::CONTENT_SHIFT;
        self.shift.set(tokens::CONTENT_SHIFT);
        self.set_offset(dx, dy);
        self.page_deck(x0, x1, s.height, a);
        match page {
            Page::Home => self.home_page(s, x0, x1, a, dx, dy),
            Page::Controls => self.controls_page(s, x0, x1, a),
            Page::Shelf => self.shelf_page(s, x0, x1, a),
            Page::Focus => self.focus_page(s, x0, x1, a),
            Page::Timer => self.timer_page(s, x0, x1, a),
            Page::Water => self.water_page(s, x0, x1, a),
            Page::Prayer => self.prayer_page(s, x0, x1, a),
            Page::Tasks => self.tasks_page(s, x0, x1, a),
            Page::System => self.system_page(s, x0, x1, a),
            Page::Agents => self.agents_page(s, x0, x1, a),
            Page::Mail => self.mail_page(s, x0, x1, a),
            Page::Plugins => self.plugins_page(s, x0, x1, a),
            Page::Settings => self.settings_page(s, x0, x1, a),
        }
        self.shift.set(0.0);
        self.set_offset(0.0, 0.0);
    }

    /// A quiet nested deck gives every page the same physical hierarchy: the navigation floats
    /// above it, while page content sits inside a softly machined surface. This mirrors the
    /// compact card language of notch-style interfaces without spending space on extra chrome.
    /// The deck keeps the same gap to the bottom edge on the taller Settings panel; it never
    /// gets shorter than a normal page, so it does not squash while the panel opens.
    fn page_deck(&self, x0: f32, x1: f32, h: f32, a: f32) {
        let bottom = tokens::DECK_BOTTOM + (h - View::Panel.size().1).max(0.0);
        let outer = rect(x0 + tokens::DECK_INSET, tokens::DECK_TOP, x1 - tokens::DECK_INSET, bottom);
        self.rounded(outer, tokens::RADIUS_DECK, self.tint(tokens::FILL_GHOST), a);
        self.outline(outer, tokens::RADIUS_DECK, 1.0, false, self.tint(tokens::HAIRLINE), a);

        // A second, offset highlight makes the edge feel concentric instead of like a generic
        // bordered rectangle, while remaining cheap in the software renderer.
        let inner = rect(outer.left + 1.0, outer.top + 1.0, outer.right - 1.0, outer.bottom - 1.0);
        self.outline(inner, tokens::RADIUS_DECK - 1.0, 1.0, false, self.tint(tokens::FILL_GHOST), a * 0.7);
    }

    /// Home: music player or clock on the left. The right column holds the calendar, or lyrics
    /// that slide in from the side when the lyrics button is pressed (the calendar slides out).
    /// Without a calendar, the shape itself widens for the lyrics column.
    fn home_page(&self, s: &Scene, x0: f32, x1: f32, a: f32, dx: f32, dy: f32) {
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let inner = r - l;
        let mix = if s.lyrics.is_some() { s.lyric_mix.clamp(0.0, 1.0) } else { 0.0 };
        // The left column follows the panel width rather than a fixed number, so the room left
        // for calendar and lyrics stays balanced on narrow and wide screens.
        let full = if s.calendar.is_some() { tokens::home_left(inner) } else { inner };
        let right = l + lerp(full, tokens::lyric_left(inner), mix).min(inner);
        self.home_status(s, self.tabs_end(s, l, r) + GUTTER / 2.0, r, a);
        match s.media {
            Some(m) => self.media_page(s, m, l, right, a),
            None => self.clock_page(s, l, right, a),
        }
        // The divider separates the clock or player from the calendar. The lyrics card is its
        // own surface and needs none (a line beside its edge looked like a collision), so the
        // divider fades out as the lyrics slide in.
        let divider = if s.calendar.is_some() { 1.0 - mix } else { 0.0 };
        if divider > 0.01 && r - right > 40.0 {
            self.rounded(rect(right + 11.0, tokens::DECK_TOP + 16.0, right + 12.0, tokens::DECK_BOTTOM - 16.0), 0.5, self.tint(tokens::DIVIDER), a * divider);
        }
        // While sliding, the right column is not clickable (its position is not settled).
        let recording = self.recording.get();
        if mix > 0.0 && mix < 1.0 {
            self.recording.set(false);
        }
        // Wide enough for the lyrics card and its hairline, which start 12 DIP left of the column.
        self.push_clip(rect(right + 10.0, 0.0, r + 2.0, tokens::PANEL_H));
        let (cl, cr) = (right + GUTTER + 4.0, r);
        // The calendar slides out first, then lyrics slide in; without a calendar, lyrics slide
        // in while the shape widens.
        let leave = (mix * 2.0).min(1.0);
        let enter = if s.calendar.is_some() { (mix * 2.0 - 1.0).max(0.0) } else { mix };
        if let Some(cal) = s.calendar
            && leave < 0.999
        {
            self.set_offset(dx + COLUMN_SLIDE * leave, dy);
            self.calendar(s, cal, cl, cr, a * (1.0 - leave));
        }
        if let Some(lyrics) = s.lyrics
            && enter > 0.001
        {
            self.set_offset(dx + COLUMN_SLIDE * (1.0 - enter), dy);
            // Laid out at the column's final width and only clipped while the shape is still
            // widening, so lines are not rewrapped every frame.
            let padded = (x1 - x0) - 2.0 * MARGIN;
            let width = padded - tokens::lyric_left(padded) - 24.0;
            self.lyrics_column(lyrics, s.media.and_then(|media| media.accent), cl, cr.max(cl + width), a * enter);
        }
        self.set_offset(dx, dy);
        self.pop_clip();
        self.recording.set(recording);
    }

    /// Home's caption on the right of the tab row: the music wave, the time while music plays
    /// (the big clock is hidden then), the battery, and the weather. Laid out right to left in
    /// that order of importance inside the room the tabs leave; whatever does not fit is left out
    /// (the weather first shortens to "26° · City", then "26°"). The battery opens System and the
    /// weather opens Controls, where its city is set.
    fn home_status(&self, s: &Scene, left: f32, r: f32, a: f32) {
        let p = &self.pal;
        let row = |l: f32, r: f32| rect(l, TOP_Y - 8.0, r, TOP_Y + 8.0);
        let mut x = r;
        let fits = |x: f32, width: f32| x - width >= left;
        if let Some(m) = s.media.filter(|m| m.playing) {
            self.wave(r - 8.0, TOP_Y, s.seconds, accent(m.accent, p.text), a);
            x -= 16.0 + TIGHT;
        }
        if s.media.is_some() {
            let width = self.text_width(s.time, &self.fonts.label_end);
            if fits(x, width) {
                self.text(s.time, &self.fonts.label_end, row(x - width - 2.0, x), p.text, a);
                x -= width + TIGHT;
            }
        }
        // One hoverable status item: a soft key under it on hover, and its page on click.
        let item = |x: f32, width: f32, page: Page| {
            let area = rect(x - width - 6.0, TOP_Y - tokens::TAB_H / 2.0, x + 4.0, TOP_Y + tokens::TAB_H / 2.0);
            let over = s.hovered == Some(Hit::PageDot(page));
            if over {
                self.rounded(area, tokens::RADIUS_ROW, self.tint(tokens::FILL_SOFT), a * self.hover.get());
            }
            self.hit(area, Hit::PageDot(page));
            over
        };
        if let Some((percent, plugged)) = s.battery {
            let text = format!("{percent}%");
            let width = self.text_width(&text, &self.fonts.caption_end) + 18.0;
            if fits(x, width) {
                let over = item(x, width, Page::System);
                self.battery_ring(percent, plugged, x - 6.0, TOP_Y, a);
                self.text(&text, &self.fonts.caption_end, row(x - width - 2.0, x - 16.0), if over { p.text } else { p.soft }, a);
                x -= width + TIGHT + 4.0;
            }
        }
        if let Some(weather) = s.weather {
            let temp = weather.split(' ').next().unwrap_or(weather);
            let city = weather.rsplit(" · ").next().filter(|c| *c != weather);
            let short = city.map(|c| format!("{temp} · {c}"));
            let options = [Some(weather.to_string()), short, Some(temp.to_string())];
            if let Some((text, width)) = options
                .into_iter()
                .flatten()
                .map(|t| {
                    let w = self.text_width(&t, &self.fonts.caption_end);
                    (t, w)
                })
                .find(|(_, w)| fits(x, *w + 6.0))
            {
                let over = item(x, width, Page::Controls);
                self.text(&text, &self.fonts.caption_end, row(x - width - 2.0, x), if over { p.text } else { p.muted }, a);
            }
        }
    }

    /// Small battery ring: green while charging, red when nearly empty.
    fn battery_ring(&self, percent: u32, plugged: bool, cx: f32, cy: f32, a: f32) {
        let color = if plugged {
            GREEN
        } else if percent <= 20 {
            RED
        } else {
            self.pal.soft
        };
        self.ring(cx, cy, Ring::BATTERY, percent as f32 / 100.0, color, a);
    }

    /// Redraw only the music wave at the right end of the tab row, on a settled panel `w` wide. Returns
    /// the changed area (pixels) for [`Renderer::present`].
    pub fn draw_header_wave(&self, w: f32, seconds: f64, color: Option<u32>) -> Result<RECT> {
        let (cx, cy) = (CANVAS.0 / 2.0 + w / 2.0 - MARGIN - 8.0, tokens::TABS_Y);
        self.patch(cx, cy, 10.0, 8.0, || self.wave(cx, cy, seconds, accent(color, self.pal.text), 1.0))
    }

    /// Apple Music-style lyrics: one large, bright current line with quiet context above and
    /// below. The album tint sits behind the focus line and the existing eased scroll moves the
    /// next line into place without changing the layout width.
    fn lyrics_column(&self, lyrics: &LyricsView, cover_accent: Option<u32>, l: f32, r: f32, a: f32) {
        let p = &self.pal;
        // One rounded card on the page's 16 DIP padding (top, right, bottom), its content
        // `LYRIC_PAD` inside it on all four sides. Drawing the card
        // once keeps its corners round by construction.
        let card = rect(l - LYRIC_PAD, LYRIC_TOP, r, LYRIC_BOTTOM);
        let glow = accent(cover_accent, p.accent);
        self.rounded(card, RADIUS_LYRIC, self.tint(tokens::FILL_GHOST), a * 0.9);
        self.outline(card, RADIUS_LYRIC, 1.0, false, self.tint(tokens::HAIRLINE), a * 0.8);
        // A soft glow from the cover colour, inside the card's own shape.
        self.rounded(rect(card.left + 1.0, card.top + 1.0, card.right - 1.0, card.bottom - 1.0), RADIUS_LYRIC - 1.0, with_alpha(glow, 0.05), a);
        let inner = rect(card.left + LYRIC_PAD, card.top + LYRIC_PAD, card.right - LYRIC_PAD, card.bottom - LYRIC_PAD);

        if let Some(status) = lyrics.status {
            self.empty_state(inner, Some(ICON_MUSIC), status, "", a);
            return;
        }
        let eased = 1.0 - (1.0 - lyrics.scroll.clamp(0.0, 1.0)).powi(3);
        // Lines that scroll past the padding fade out there instead of reaching the card's edge.
        self.push_clip(rect(card.left, card.top + 2.0, card.right, card.bottom - 2.0));
        for (i, line) in lyrics.lines.iter().enumerate() {
            // Position relative to the sung line (0) and the next one (1); while scrolling,
            // everything is still a bit lower. The previous line fades out as it leaves.
            let pos = i as f32 - 2.0 + (1.0 - eased);
            let visible = ((0.85 - (pos - 0.5).abs()) / 0.35).clamp(0.0, 1.0);
            if line.is_empty() || visible < 0.01 {
                continue;
            }
            let focus = (1.0 - pos.abs()).clamp(0.0, 1.0);
            let y = LYRIC_CENTER + pos * LYRIC_GAP;
            let line_alpha = a * visible * (0.34 + 0.66 * focus);
            // The current line may wrap to two lines; the context lines are trimmed to one.
            let (font, half) = if focus > 0.5 {
                (&self.fonts.lyric_active, 22.0)
            } else {
                (&self.fonts.lyric_context, 10.0)
            };
            self.text(line, font, rect(inner.left, y - half, inner.right, y + half), blend(p.soft, p.text, focus), line_alpha);
        }
        self.pop_clip();
    }

    /// Welcome on Crest's first run: how to use it in four lines.
    fn welcome_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        self.text("Hi! This is Crest", &self.fonts.title, rect(l, HEADER_Y - 10.0, r - 60.0, HEADER_Y + 10.0), p.text, a);
        self.eyes(r - 18.0, HEADER_Y + 2.0, EyeSize::HEADER, s.eyes, a);
        let tips = [
            "Hover the pill with the cursor to open it",
            "The tabs on top or the mouse wheel switch pages",
            "Settings: the gear at the end of the tabs, or the tray icon",
            "Lyrics, weather, and messages stay off until you turn them on",
        ];
        for (i, tip) in tips.iter().enumerate() {
            let y = 48.0 + i as f32 * 19.0;
            self.circle(l + 4.0, y, 2.5, p.accent, a);
            self.text(tip, &self.fonts.caption, rect(l + 14.0, y - 9.0, r, y + 9.0), p.soft, a);
        }
        let button = rect(r - 110.0, ROW_Y - CONTROL_H / 2.0, r, ROW_Y + CONTROL_H / 2.0);
        let over = s.hovered == Some(Hit::WelcomeDone);
        self.chip(button, "Got it", Style::Primary, over, a);
        self.hit(button, Hit::WelcomeDone);
    }

    /// Tab row: one icon per visible page, then the settings gear after a thin divider.
    ///
    /// Layout is fixed for a given set of pages, so the bar never changes width or shifts while
    /// the user moves between pages: the reserved width for a page's name is the widest label
    /// the row could show, not the label of whichever page happens to be current. The name is
    /// drawn only for the current page (and the one it is coming from), inside that reserve.
    /// Tab row layout: the pages on it, the icon slot, whether the current page shows its name,
    /// and the width charged for that name. Shared by the bar and by Home's status, which may
    /// only use what the bar leaves free.
    fn tab_layout(&self, s: &Scene, l: f32, r: f32) -> (Vec<Page>, f32, bool, f32) {
        let pages: Vec<Page> = s.pages.visible().chain([Page::Settings]).collect();
        // Keep every page on the row. The caption on the right and the gutter always win; the
        // icon slots tighten, then the name reserve drops, and only then do labels disappear.
        // A fixed reserve, not the caption of the current page: the caption changes with the
        // page ("Drink at 19:37" on Water, "Nothing unread" on Mail), and sizing the row from
        // it made the whole tab bar shift as the user moved between pages.
        let available = (r - l - caption_reserve(r - l) - GUTTER).max(0.0);
        let slot = navigation_slot(available, pages.len());
        // The reserve is the widest label in the row and does not depend on the current page,
        // so the bar measures the same on every page.
        let reserve = pages.iter().map(|&page| self.text_width(label(page), &self.fonts.caption) + TAB_LABEL_PAD).fold(0.0, f32::max);
        // A name is shown only when the row plus one label genuinely fits beside the caption.
        let labels = slot * pages.len() as f32 + TIGHT + reserve <= available;
        (pages, slot, labels, reserve)
    }

    /// Right edge of the tab bar's capsule, for content that shares the row.
    fn tabs_end(&self, s: &Scene, l: f32, r: f32) -> f32 {
        let (pages, slot, labels, reserve) = self.tab_layout(s, l, r);
        l - 4.0 + slot * pages.len() as f32 + if labels { reserve } else { 0.0 } + TIGHT
    }

    fn navigation(&self, s: &Scene, x0: f32, x1: f32, a: f32, dy: f32) {
        const HALF: f32 = tokens::TAB_H / 2.0;
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let (pages, slot, labels, _) = self.tab_layout(s, l, r);
        let eased = s.page_from.map_or(1.0, |(_, t, _)| 1.0 - (1.0 - t).powi(3));
        let current = s.pages.tab(s.page);
        let previous = s.page_from.map(|(from, ..)| s.pages.tab(from));
        let weight = |page: Page| match previous {
            // Switching between Focus and Timer keeps the shared tab lit.
            Some(from) if from == page && page == current => 1.0,
            _ if page == current => eased,
            Some(from) if from == page => 1.0 - eased,
            _ => 0.0,
        };
        // One tab carries the name at a time: the reserve is the widest label in the row, and it
        // is charged once. That keeps the bar within a normal 440 DIP panel and still measures
        // the same on every page, which is what makes it feel stable while switching.
        // The named tab is exactly as wide as its own name, and grows or shrinks with the page
        // slide, so the name glides from one tab to the next instead of jumping. (The budget
        // in `tab_layout` charges the widest name, so the row always fits.)
        let name = |page: Page| if page == current { label(s.page) } else { label(page) };
        let widths: Vec<f32> = pages
            .iter()
            .map(|&page| {
                let k = weight(page);
                if labels && k > 0.0 { slot + (self.text_width(name(page), &self.fonts.caption) + TAB_LABEL_PAD) * k } else { slot }
            })
            .collect();
        let total = widths.iter().sum::<f32>() + TIGHT;
        let y = TABS_Y + dy;
        let left = l - 4.0;
        self.rounded(rect(left, y - HALF - 2.0, left + total, y + HALF + 2.0), HALF + 2.0, self.tint(tokens::FILL_SUBTLE), a);
        let mut x = left;
        for (&page, &width) in pages.iter().zip(&widths) {
            if page == Page::Settings {
                let mid = x + TIGHT / 2.0;
                self.separator(mid, y - 6.0, y + 6.0, a);
                x += TIGHT;
            }
            let k = weight(page);
            let over = s.hovered == Some(Hit::PageDot(page));
            let key = rect(x + 2.0, y - HALF, x + width - 2.0, y + HALF);
            let color = self.tab_key(key, k, over, a);
            // The icon stays on the same pixel whether or not the name is showing, so the row
            // does not jump as a label appears.
            let cx = x + slot / 2.0;
            match page_icon(page) {
                Some(icon) => self.glyph(icon, &self.fonts.icon_13, cx, y, color, a),
                None => self.drop_icon(cx, y, 13.0, color, a),
            }
            if labels && k > 0.01 {
                // Clipped to its key while the key is still growing, so it never spills over.
                self.push_clip(key);
                let text_w = self.text_width(name(page), &self.fonts.caption) + 2.0;
                self.text(name(page), &self.fonts.caption, rect(x + slot - 2.0, y - 8.0, x + slot + text_w, y + 8.0), p.text, a * k);
                self.pop_clip();
            }
            self.hit(rect(x, TABS_Y - HALF - 2.0, x + width, TABS_Y + HALF + 2.0), Hit::PageDot(page));
            x += width;
        }
    }

    /// Page caption on the right of the tab row; it slides along with its page.
    fn header(&self, s: &Scene, header: Header, x1: f32, a: f32) {
        let Header { right, right_hit } = header;
        let p = &self.pal;
        let r = x1 - MARGIN;
        let color = match right_hit {
            Some(hit) if s.hovered == Some(hit) => p.text,
            Some(_) => p.accent,
            None => p.muted,
        };
        // The caption never takes more than its share of the panel, so it cannot crowd the tabs.
        // The panel's inner width is the scene width minus both insets.
        let width = self.text_width(right, &self.fonts.caption_end).min(caption_reserve(s.width - 2.0 * MARGIN));
        self.text(right, &self.fonts.caption_end, rect(r - width - 2.0, TOP_Y - 8.0, r, TOP_Y + 8.0), color, a);
        if let Some(hit) = right_hit {
            self.hit(rect(r - width - 6.0, TOP_Y - 11.0, r + 4.0, TOP_Y + 11.0), hit);
        }
    }

    /// Clock on Home, between `l` and `r`.
    fn clock_page(&self, s: &Scene, l: f32, r: f32, a: f32) {
        let p = &self.pal;
        // Raised slightly so the glance chips below have room.
        self.text(s.greeting, &self.fonts.greeting, rect(l, 42.0, r - 90.0, 58.0), p.muted, a);
        self.text(s.time, &self.fonts.big_time, rect(l, 54.0, r - 90.0, 104.0), p.text, a);
        self.text(s.date, &self.fonts.date, rect(l, 102.0, r - 90.0, 118.0), p.soft, a);
        self.eyes(r - 40.0, HERO_Y, EyeSize::CLOCK, s.eyes, a);
        if !s.locked {
            self.glance(s, l, r, a);
        }
    }

    /// At-a-glance chips under the clock: what is left today on other pages (tasks, water,
    /// agents that need you). Each chip opens its page; chips that do not fit are left out.
    fn glance(&self, s: &Scene, l: f32, r: f32, a: f32) {
        let p = &self.pal;
        let mut chips: Vec<(Option<char>, String, D2D1_COLOR_F, Page)> = Vec::new();
        if s.pages.shown(Page::Prayer)
            && let Some((name, time, _)) = &s.prayer.next
        {
            chips.push((Some(ICON_PRAYER), format!("{name} {time}"), p.accent, Page::Prayer));
        }
        let remaining = s.tasks.remaining();
        if s.pages.shown(Page::Tasks) && remaining > 0 {
            let text = if remaining == 1 { "1 task left".to_string() } else { format!("{remaining} tasks left") };
            chips.push((Some(ICON_TASKS), text, p.accent, Page::Tasks));
        }
        if s.pages.shown(Page::Water) {
            chips.push((None, format!("{} / {} L", liters(s.water.ml), liters(s.water.goal)), BLUE, Page::Water));
        }
        if s.pages.shown(Page::Agents) {
            let count = |state: AgentState| s.agents.iter().filter(|row| row.state == state).count();
            match (count(AgentState::Waiting), count(AgentState::Running)) {
                (0, 0) => {}
                (0, working) => chips.push((Some(ICON_TERMINAL), format!("{working} working"), GREEN, Page::Agents)),
                (waiting, _) => chips.push((Some(ICON_TERMINAL), format!("{waiting} need you"), ORANGE, Page::Agents)),
            }
        }
        let mut x = l;
        for (icon, text, color, page) in chips {
            let width = self.text_width(&text, &self.fonts.caption) + 34.0;
            if x + width > r {
                break;
            }
            let area = rect(x, ROW_Y - CONTROL_H / 2.0, x + width, ROW_Y + CONTROL_H / 2.0);
            let hit = Hit::PageDot(page);
            let over = s.hovered == Some(hit);
            self.card(area, CONTROL_H / 2.0, over, a);
            match icon {
                Some(icon) => self.glyph(icon, &self.fonts.icon_11, x + 14.0, ROW_Y, color, a),
                None => self.drop_icon(x + 14.0, ROW_Y, 11.0, color, a),
            }
            let label = rect(x + 25.0, ROW_Y - 8.0, area.right - 8.0, ROW_Y + 8.0);
            self.text(&text, &self.fonts.caption, label, if over { p.text } else { p.soft }, a);
            self.hit(area, hit);
            x += width + TIGHT;
        }
    }

    /// Calendar column: month, five sliding days, the selected day's agenda, and the add box.
    fn calendar(&self, s: &Scene, cal: &CalendarView, l: f32, r: f32, a: f32) {
        let p = &self.pal;
        self.text(&cal.month, &self.fonts.title, rect(l, 44.0, l + 46.0, 64.0), p.text, a);
        self.text(&cal.year, &self.fonts.caption, rect(l, 64.0, l + 46.0, 78.0), p.muted, a);
        let strip = l + 46.0;
        let col = (r - strip) / cal.days.len().max(1) as f32;
        self.hit(rect(strip, 40.0, r, 90.0), Hit::CalendarStrip);
        for (i, day) in cal.days.iter().enumerate() {
            let cx = strip + col * (i as f32 + 0.5);
            let hit = Hit::CalendarDay(day.offset);
            let name = if day.today { p.accent } else { p.muted };
            self.text(day.name, &self.fonts.caption_center, rect(cx - col / 2.0, 42.0, cx + col / 2.0, 56.0), name, a);
            let (fill, color) = if day.today {
                (Some(p.accent), WHITE)
            } else if day.selected {
                (Some(self.tint(tokens::FILL_STRONG)), p.text)
            } else if s.hovered == Some(hit) {
                (Some(self.tint(tokens::FILL_SUBTLE)), p.text)
            } else {
                (None, p.soft)
            };
            if let Some(fill) = fill {
                self.circle(cx, 69.0, 12.0, fill, a);
            }
            if day.today && day.selected {
                self.circle_outline(cx, 69.0, 14.5, 1.2, with_alpha(p.accent, 0.5), a);
            }
            self.text(&day.number.to_string(), &self.fonts.label_center, rect(cx - 14.0, 59.0, cx + 14.0, 79.0), color, a);
            if day.has_events {
                self.circle(cx, 86.0, 2.0, if day.today { p.accent } else { p.muted }, a);
            }
            self.hit(rect(cx - col / 2.0, 40.0, cx + col / 2.0, 90.0), hit);
        }

        // Agenda of the selected day, then events from subscribed feeds.
        self.hit(rect(l - 4.0, 91.0, r + 4.0, 122.0), Hit::AgendaList);
        if cal.events.is_empty() {
            self.text("No events", &self.fonts.caption, rect(l, 98.0, r, 114.0), p.muted, a);
        }
        for (row, item) in cal.events.iter().skip(cal.scroll).take(crate::tools::agenda::VISIBLE).enumerate() {
            let y = 98.0 + row as f32 * AGENDA_ROW_H;
            // Feed events are read-only: no circle and no ×, so they cannot be ticked off here.
            let typed = item.id;
            let hovered = typed.is_some_and(|id| {
                matches!(s.hovered, Some(Hit::AgendaRow(h) | Hit::AgendaToggle(h) | Hit::AgendaRemove(h)) if h == id)
            });
            if let Some(id) = typed {
                // The whole row triggers the hover; the circle and × are recorded after it so they win.
                self.hit(rect(l - 4.0, y - 8.0, r + 4.0, y + 8.0), Hit::AgendaRow(id));
            }
            let bx = l + 6.0;
            if let Some(id) = typed {
                if item.done {
                    self.circle(bx, y, 6.0, p.accent, a);
                    self.glyph(ICON_CHECK, &self.fonts.icon_9, bx, y, WHITE, a);
                } else {
                    self.circle_outline(bx, y, 5.5, 1.4, if hovered { p.text } else { p.accent }, a);
                }
                self.hit(rect(bx - 9.0, y - 8.0, bx + 9.0, y + 8.0), Hit::AgendaToggle(id));
            } else {
                // A feed event gets a dot in its feed's color instead of a checkbox.
                self.circle(bx, y, 2.5, if item.now { GREEN } else { p.muted }, a);
            }
            // Typed rows show their time on the right; a feed row shows where it came from,
            // "09.00 · Work" when timed and just "Work" for an all-day event.
            let label = match (&item.source, item.time.is_empty()) {
                (Some(source), true) => source.clone(),
                (Some(source), false) => format!("{} · {source}", item.time),
                (None, _) => item.time.clone(),
            };
            // The time column keeps its width; a feed name that needs more takes it from the title.
            let column = match item.source {
                Some(_) if !hovered => self.text_width(&label, &self.fonts.caption_end).max(44.0),
                _ => 44.0,
            };
            let area = rect(l + 18.0, y - 8.0, r - column, y + 8.0);
            if item.done {
                self.text_struck(&item.text, &self.fonts.caption, area, p.muted, a);
            } else if item.now {
                // Running now: the title in the accent color.
                self.text(&item.text, &self.fonts.caption, area, GREEN, a);
            } else {
                self.text(&item.text, &self.fonts.caption, area, p.text, a);
            }
            if hovered {
                if let Some(id) = typed {
                    let over = s.hovered == Some(Hit::AgendaRemove(id));
                    self.glyph(ICON_CLOSE, &self.fonts.icon_9, r - 8.0, y, if over { RED } else { p.muted }, a);
                    self.hit(rect(r - 20.0, y - 8.0, r + 4.0, y + 8.0), Hit::AgendaRemove(id));
                }
            } else if !label.is_empty() {
                self.text(&label, &self.fonts.caption_end, rect(r - column, y - 8.0, r, y + 8.0), p.muted, a);
            }
        }

        // Add-event box; the number of events that do not fit is written at its right end.
        let input = rect(l, ROW_Y - CONTROL_H / 2.0, r, ROW_Y + CONTROL_H / 2.0);
        let over = s.hovered == Some(Hit::AgendaInput);
        match s.agenda_draft {
            Some(draft) => self.typing_box(input, draft, "09.00 Meeting, Enter", a),
            None => {
                self.text_field(input, "", "+  Add event", over, a);
                let inner = rect(input.left + 10.0, input.top, input.right - 10.0, input.bottom);
                let hidden = cal.events.len().saturating_sub(cal.scroll + crate::tools::agenda::VISIBLE);
                if hidden > 0 {
                    self.text(&format!("{hidden} more"), &self.fonts.caption_end, inner, p.muted, a);
                }
            }
        }
        self.hit(input, Hit::AgendaInput);
    }

    /// Volume, brightness, microphone, quiet mode, and weather city, laid out like a control
    /// centre: full-width capsule sliders on top (brightness only on displays that support it),
    /// and a row of equal toggle tiles at the bottom that each show their state.
    fn controls_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let c = s.controls;
        let right = c.quiet_minutes.map(|m| format!("Quiet · {m} min")).unwrap_or_default();
        self.header(s, Header { right: &right, right_hit: None }, x1, a);

        // Sliders and tiles are one block centred in the deck, so the gaps stay the same whether
        // or not the display has a brightness control (pinning the tiles to the bottom left a
        // hole where the second slider would be).
        let sliders = if c.brightness.is_some() { 2.0 } else { 1.0 };
        let block = sliders * SLIDER_H + (sliders - 1.0) * SLIDER_GAP + TILES_GAP + CONTROL_TILE_H;
        let start = (tokens::DECK_TOP + tokens::DECK_BOTTOM - block) / 2.0;
        let first = start + SLIDER_H / 2.0;
        let (top, bottom) = (start + block - CONTROL_TILE_H, start + block);
        let volume = c.volume;
        let second = c.brightness.map(|_| first + SLIDER_H + SLIDER_GAP);
        {
            let y = first;
            let (icon, value, label, muted) = match volume {
                Some((level, true)) => (ICON_MUTE, Some(level), "Muted".to_string(), true),
                Some((level, false)) => (volume_icon(level), Some(level), format!("{}", (level * 100.0).round()), false),
                None => (ICON_VOLUME[2], None, "No output".to_string(), false),
            };
            self.capsule_slider(s, y, l, r, (icon, Some(Hit::VolumeMute)), Hit::VolumeSlider, value, &label, muted, a);
        }
        if let (Some(y), Some(b)) = (second, c.brightness) {
            self.capsule_slider(s, y, l, r, (ICON_BRIGHTNESS, None), Hit::BrightnessSlider, Some(b as f32 / 100.0), &b.to_string(), false, a);
        }

        if let Some(draft) = s.city_draft {
            let cy = (top + bottom) / 2.0;
            let area = rect(l, cy - CONTROL_H / 2.0, r, cy + CONTROL_H / 2.0);
            self.typing_box(area, draft, "Type a city name, Enter to save", a);
            self.hit(area, Hit::CityInput);
            return;
        }
        // Tiles: icon disc (filled while the toggle is on), name, and state below it.
        let mic = match c.mic {
            Some(true) => Some((ICON_MICROPHONE, "Microphone", "Muted".to_string(), Some(ORANGE), Some(Hit::MicToggle))),
            Some(false) => Some((ICON_MICROPHONE, "Microphone", "On".to_string(), None, Some(Hit::MicToggle))),
            None => Some((ICON_MICROPHONE, "Microphone", "Not found".to_string(), None, None)),
        };
        let quiet = Some(match c.quiet_minutes {
            Some(m) => (ICON_QUIET, "Quiet", format!("{m} min left"), Some(p.accent), Some(Hit::QuietToggle)),
            None => (ICON_QUIET, "Quiet", "1 hour".to_string(), None, Some(Hit::QuietToggle)),
        });
        let city = c.city.as_ref().map(|text| {
            let (name, detail) = match text.split_once(" · ") {
                Some((name, temp)) => (name.trim_start_matches("City: ").to_string(), temp.to_string()),
                None => (text.trim_start_matches("City: ").to_string(), "Weather".to_string()),
            };
            (ICON_LOCATION, name, detail)
        });
        let mut tiles: Vec<Tile> = [mic, quiet]
            .into_iter()
            .flatten()
            .map(|(icon, name, state, on, hit)| (icon, name.to_string(), state, on, hit))
            .collect();
        if let Some((icon, name, detail)) = city {
            tiles.push((icon, name, detail, None, Some(Hit::CityInput)));
        }
        let count = tiles.len().max(1) as f32;
        let width = (r - l - TIGHT * (count - 1.0)) / count;
        for (i, (icon, name, state, on, hit)) in tiles.into_iter().enumerate() {
            let x = l + i as f32 * (width + TIGHT);
            let area = rect(x, top, x + width, bottom);
            let over = hit.is_some() && s.hovered == hit;
            self.card(area, tokens::RADIUS_ROW, over, a);
            let (cx, cy) = (x + 20.0, (top + bottom) / 2.0);
            match on {
                Some(fill) => {
                    self.circle(cx, cy, 12.0, fill, a);
                    let ink = if fill == p.accent { p.on_accent } else { ON_STATUS };
                    self.glyph(icon, &self.fonts.icon_13, cx, cy, ink, a);
                }
                None => {
                    self.circle(cx, cy, 12.0, self.tint(tokens::FILL_SUBTLE), a);
                    self.glyph(icon, &self.fonts.icon_13, cx, cy, if hit.is_some() { p.text } else { p.dim }, a);
                }
            }
            let text_l = x + 38.0;
            self.text(&name, &self.fonts.caption, rect(text_l, cy - 15.0, area.right - 8.0, cy), if hit.is_some() { p.text } else { p.muted }, a);
            let tone = on.unwrap_or(if over { p.soft } else { p.muted });
            self.text(&state, &self.fonts.caption, rect(text_l, cy, area.right - 8.0, cy + 15.0), tone, a);
            if let Some(hit) = hit {
                self.hit(area, hit);
            }
        }
    }

    /// Control-centre slider: one capsule, filled from the left up to the value, the icon
    /// inside its left end (a mute button when `icon.1` is set), and the value at its right end.
    /// Brightens on hover; the click area is the whole capsule, so click position = value.
    #[allow(clippy::too_many_arguments)]
    fn capsule_slider(&self, s: &Scene, y: f32, l: f32, r: f32, icon: (char, Option<Hit>), hit: Hit, value: Option<f32>, label: &str, muted: bool, a: f32) {
        let p = &self.pal;
        let half = SLIDER_H / 2.0;
        let track = rect(l, y - half, r, y + half);
        let over = s.hovered == Some(hit);
        self.rounded(track, half, self.tint(if over { tokens::FILL_SOFT } else { tokens::FILL_SUBTLE }), a);
        let fill = if muted { p.muted } else if over { blend(p.accent, WHITE, 0.12) } else { p.accent };
        let reach = value.map_or(0.0, |v| l + SLIDER_H + (r - l - SLIDER_H) * v.clamp(0.0, 1.0));
        if value.is_some() {
            self.rounded(rect(l, track.top, reach, track.bottom), half, fill, a);
        }
        let (glyph, icon_hit) = icon;
        let over_icon = icon_hit.is_some() && s.hovered == icon_hit;
        let cx = l + half;
        if over_icon {
            self.circle(cx, y, half - 3.0, with_alpha(WHITE, 0.18), a);
        }
        let on_fill = if muted { ON_STATUS } else { p.on_accent };
        self.glyph(glyph, &self.fonts.icon_13, cx, y, if value.is_some() { on_fill } else { p.muted }, a);
        // The value sits on the track; when the fill reaches it, it takes the fill's ink.
        let label_area = rect(r - 56.0, y - 8.0, r - 12.0, y + 8.0);
        let color = if value.is_none() { p.muted } else if reach >= label_area.right { on_fill } else { p.soft };
        self.text(label, &self.fonts.caption_end, label_area, color, a);
        if value.is_some() {
            self.hit(track, hit);
        }
        if let Some(icon_hit) = icon_hit {
            self.hit(rect(l, track.top, l + SLIDER_H, track.bottom), icon_hit);
        }
    }

    /// Quick timer: time-left ring, duration choices, pause, and cancel.
    fn timer_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let t = s.timer;
        let total = if t.active { format!("{} min timer", t.total_minutes) } else { String::new() };
        let merged = self.timer_modes(s, x1, a);
        if !merged {
            self.header(s, Header { right: &total, right_hit: None }, x1, a);
        }

        let (cx, cy) = (l + 34.0, HERO_Y);
        if t.active {
            if s.hovered == Some(Hit::TimerToggle) {
                self.icon_button(cx, cy, 24.0, true, a);
            }
            self.ring(cx, cy, Ring::LARGE, t.progress, ORANGE, a);
            self.glyph(if t.running { ICON_PAUSE } else { ICON_PLAY }, &self.fonts.icon_20, cx, cy, p.text, a);
            self.hit(rect(cx - 34.0, cy - 34.0, cx + 34.0, cy + 34.0), Hit::TimerToggle);
            self.text(&t.clock, &self.fonts.big_num, rect(l + 82.0, HERO_Y - 28.0, r - 44.0, HERO_Y + 12.0), p.text, a);
            let state = if t.running { "Running" } else { "Paused" };
            let state = if merged { format!("{total} · {}", state.to_lowercase()) } else { state.to_string() };
            self.text(&state, &self.fonts.caption, rect(l + 84.0, HERO_Y + 10.0, r - 44.0, HERO_Y + 26.0), p.muted, a);
            let (bx, by) = (r - 14.0, HERO_Y);
            let over = s.hovered == Some(Hit::TimerCancel);
            self.icon_button(bx, by, 15.0, over, a);
            self.glyph(ICON_CLOSE, &self.fonts.icon_13, bx, by, if over { p.text } else { p.soft }, a);
            self.hit(rect(bx - 16.0, by - 16.0, bx + 16.0, by + 16.0), Hit::TimerCancel);
        } else {
            self.ring(cx, cy, Ring::LARGE, 0.0, ORANGE, a);
            self.glyph(ICON_TIMER, &self.fonts.icon_20, cx, cy, p.soft, a);
            self.text("Pick a duration", &self.fonts.title, rect(l + 82.0, HERO_Y - 18.0, r, HERO_Y + 2.0), p.text, a);
            let hint = "You get a notice and an alarm when time is up";
            self.text(hint, &self.fonts.caption, rect(l + 82.0, HERO_Y + 4.0, r, HERO_Y + 20.0), p.muted, a);
        }

        let mut x = l;
        let presets = crate::tools::timer::PRESETS.map(|m| (format!("{m} min"), Hit::TimerPreset(m)));
        let add = t.active.then(|| ("+1 min".to_string(), Hit::TimerAdd));
        for (label, hit) in presets.into_iter().chain(add) {
            let area = rect(x, ROW_Y - CONTROL_H / 2.0, x + 60.0, ROW_Y + CONTROL_H / 2.0);
            let over = s.hovered == Some(hit);
            self.chip(area, &label, Style::Secondary, over, a);
            self.hit(area, hit);
            x += 60.0 + TIGHT;
        }
    }

    /// A text field at rest (rust-ui's `Input`): `--input` fill and a `--border` hairline. The
    /// focused field uses [`Renderer::typing_box`] instead.
    fn text_field(&self, area: D2D_RECT_F, text: &str, placeholder: &str, over: bool, a: f32) {
        let p = &self.pal;
        self.rounded(area, tokens::RADIUS_ROW, p.placeholder, a);
        // The border brightens on hover, as the component library's input does.
        let line = if over { tokens::FILL_STRONG } else { tokens::HAIRLINE };
        self.outline(area, tokens::RADIUS_ROW, 1.0, false, self.tint(line), a);
        let inner = rect(area.left + 12.0, area.top, area.right - 12.0, area.bottom);
        let (label, color) = if text.is_empty() { (placeholder, p.muted) } else { (text, p.text) };
        self.push_clip(inner);
        self.text(label, &self.fonts.input, inner, color, a);
        self.pop_clip();
    }

    /// Focused text box: a `--ring` outline, with the text shifted so the typed end stays visible.
    fn typing_box(&self, area: D2D_RECT_F, draft: &str, placeholder: &str, a: f32) {
        let p = &self.pal;
        self.rounded(area, tokens::RADIUS_ROW, p.placeholder, a);
        self.outline(area, tokens::RADIUS_ROW, 1.0, false, p.accent, a);
        let inner = rect(area.left + 12.0, area.top, area.right - 12.0, area.bottom);
        let width = self.text_width(draft, &self.fonts.input);
        let start = if width > inner.right - inner.left { inner.right - width } else { inner.left };
        let cy = (area.top + area.bottom) / 2.0;
        self.push_clip(inner);
        self.text(draft, &self.fonts.input, rect(start, inner.top, start + width + 4.0, inner.bottom), p.text, a);
        let caret = start + width + 1.0;
        self.rounded(rect(caret, cy - 7.0, caret + 1.5, cy + 7.0), 0.75, p.text, a);
        if draft.is_empty() {
            self.text(placeholder, &self.fonts.input, rect(caret + 6.0, inner.top, inner.right, inner.bottom), p.muted, a);
        }
        self.pop_clip();
    }

    /// Music player on Home, between `l` and `r`: cover, title, and artist on top; progress
    /// bar in the middle; lyrics and playback buttons at the bottom.
    fn media_page(&self, s: &Scene, m: &MediaView, l: f32, r: f32, a: f32) {
        let p = &self.pal;
        // Without an artist (e.g. a video), the title is centered on the cover.
        self.art(l, ART_TOP + 8.0, ART_SIZE, 11.0, a);
        let text_l = l + ART_SIZE + 12.0;
        let (title_y, artist_y) = if m.artist.is_empty() { (ART_TOP + 8.0 + ART_SIZE / 2.0, 0.0) } else { (ART_TOP + 19.0, ART_TOP + 38.0) };
        self.text(&m.title, &self.fonts.title, rect(text_l, title_y - 10.0, r, title_y + 10.0), p.text, a);
        if !m.artist.is_empty() {
            self.text(&m.artist, &self.fonts.body, rect(text_l, artist_y - 9.0, r, artist_y + 9.0), p.muted, a);
        }

        // Elapsed time, bar, and time left. The bar thickens on hover if it can be dragged.
        if let Some(progress) = m.progress {
            let y = PROGRESS_Y;
            self.text(&m.elapsed, &self.fonts.caption, rect(l, y - 8.0, l + 40.0, y + 8.0), p.soft, a);
            self.text(&m.remaining, &self.fonts.caption_end, rect(r - 44.0, y - 8.0, r, y + 8.0), p.soft, a);
            let (start, end) = (l + 42.0, r - 48.0);
            let active = m.can_seek && (m.seeking || s.hovered == Some(Hit::MediaSeek));
            self.bar_sized(start, end, y, if active { 7.0 } else { 4.0 }, progress, p.text, a);
            if m.can_seek {
                // The click area is exactly as wide as the bar, so click position = song position.
                self.hit(rect(start, y - 9.0, end, y + 9.0), Hit::MediaSeek);
            }
        }

        // Lyrics button on the left (under the elapsed time), playback buttons in the middle.
        let y = CONTROLS_Y;
        if let Some(on) = s.lyrics_button {
            let (bx, hit) = (l + 14.0, Hit::LyricsToggle);
            let over = s.hovered == Some(hit);
            if on || over {
                self.circle(bx, y, 14.0, self.tint(if on { tokens::FILL_STRONG } else { tokens::FILL_SOFT }), a);
            }
            let icon = if on { ICON_LYRICS_ON } else { ICON_LYRICS };
            self.glyph(icon, &self.fonts.icon_13, bx, y + 0.5, if on || over { p.text } else { p.muted }, a);
            self.hit(rect(bx - 14.0, y - 14.0, bx + 14.0, y + 14.0), hit);
        }
        let cx = (l + r) / 2.0;
        for (button, bx) in [(Button::Previous, cx - 48.0), (Button::PlayPause, cx), (Button::Next, cx + 48.0)] {
            let enabled = match button {
                Button::Previous => m.can_previous,
                Button::Next => m.can_next,
                Button::PlayPause => true,
            };
            let hit = Hit::Media(button);
            if enabled && s.hovered == Some(hit) {
                self.icon_button(bx, y, 16.0, true, a);
            }
            let (icon, font) = match button {
                Button::Previous => (ICON_PREVIOUS, &self.fonts.icon_16),
                Button::Next => (ICON_NEXT, &self.fonts.icon_16),
                Button::PlayPause => (if m.playing { ICON_PAUSE } else { ICON_PLAY }, &self.fonts.icon_20),
            };
            self.glyph(icon, font, bx, y, if enabled { p.text } else { p.dim }, a);
            if enabled {
                self.hit(rect(bx - 16.0, y - 16.0, bx + 16.0, y + 16.0), hit);
            }
        }
    }

    fn shelf_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let shelf = s.shelf;
        let count = shelf.items.len();
        let (right, right_hit) = if count > 0 { ("Clear all", Some(Hit::ShelfClear)) } else { ("", None) };
        self.header(s, Header { right, right_hit }, x1, a);

        let strip = rect(l, SHELF_TOP, r, SHELF_TOP + TILE_H);
        self.hit(strip, Hit::ShelfStrip);
        let fx = s.shelf_fx;
        if count == 0 && !shelf.drop_hover {
            self.outline(strip, tokens::RADIUS_CARD, 1.2, true, p.dim, a);
            self.glyph(ICON_DOWNLOAD, &self.fonts.icon_20, (l + r) / 2.0, 66.0, p.muted, a);
            let text = "Drag files onto the panel to keep them here for now";
            self.text(text, &self.fonts.caption_center, rect(l, SHELF_TOP + 48.0, r, SHELF_TOP + 64.0), p.muted, a);
            self.drop_glow(strip, fx, a);
            return;
        }

        for (slot, item) in shelf.items.iter().skip(shelf.scroll).take(SHELF_VISIBLE).enumerate() {
            let tx = l + slot as f32 * TILE_PITCH;
            let tile = rect(tx, SHELF_TOP, tx + TILE_PITCH - 8.0, SHELF_TOP + TILE_H);
            let hovered = matches!(s.hovered, Some(Hit::ShelfItem(id) | Hit::ShelfRemove(id)) if id == item.id);
            // New tiles grow in, removed tiles shrink away, the hovered tile grows 5%.
            let (grow, fade) = fx.tiles.get(&item.id).copied().unwrap_or((1.0, 1.0));
            let ta = a * fade;
            self.scaled(tx + 34.0, SHELF_TOP + 40.0, grow * if hovered { 1.05 } else { 1.0 }, || {
                if hovered {
                    self.card(tile, tokens::RADIUS_ROW, false, ta);
                }
                self.thumb(item.id, item.is_dir, tx + 34.0, SHELF_TOP + 6.0, 48.0, ta);
                self.text(&item.name, &self.fonts.caption_center, rect(tx + 3.0, SHELF_TOP + 58.0, tx + 65.0, SHELF_TOP + 74.0), p.soft, ta);
            });
            self.hit(tile, Hit::ShelfItem(item.id));
            if hovered {
                let (bx, by) = (tx + 60.0, SHELF_TOP + 7.0);
                let over = s.hovered == Some(Hit::ShelfRemove(item.id));
                self.circle(bx, by, 8.5, if over { RED } else { p.badge }, a);
                self.glyph(ICON_CLOSE, &self.fonts.icon_9, bx, by, if over { WHITE } else { p.text }, a);
                self.hit(rect(bx - 9.5, by - 9.5, bx + 9.5, by + 9.5), Hit::ShelfRemove(item.id));
            }
        }
        if shelf.drop_hover {
            self.drop_zone(s, strip, count > 0, a);
        } else {
            let hint = if count > SHELF_VISIBLE {
                let last = (shelf.scroll + SHELF_VISIBLE).min(count);
                format!("{}–{last} of {count} · scroll for more · double-click to open", shelf.scroll + 1)
            } else {
                "Double-click to open · right-click to show in folder · drag out".to_string()
            };
            self.text(&hint, &self.fonts.caption_center, rect(l, ROW_Y - 8.0, r, ROW_Y + 8.0), p.muted, a);
        }
        self.drop_glow(strip, fx, a);
    }

    /// Drop area while a file is dragged onto the panel: a marching dashed line, lit in the
    /// accent color when the file is right over it, with an icon bobbing to invite the drop.
    fn drop_zone(&self, s: &Scene, area: D2D_RECT_F, adding: bool, a: f32) {
        let p = &self.pal;
        let fx = s.shelf_fx;
        if adding {
            // Existing tiles are dimmed so the prompt stays readable.
            self.rounded(area, tokens::RADIUS_CARD, with_alpha(p.body, 0.85), a);
        }
        // Two zones: keep on the shelf (left) or share (right).
        let shelf = rect(area.left, area.top, area.right - 120.0, area.bottom);
        let share = rect(area.right - 112.0, area.top, area.right, area.bottom);
        let text = if adding { "Drop to add" } else { "Drop to keep on the shelf" };
        self.drop_target(s, shelf, fx.targeted, ICON_DOWNLOAD, text, a);
        self.drop_target(s, share, fx.share_targeted, ICON_SHARE, "Share", a);
        self.hit(share, Hit::DropShare);
    }

    /// One drop zone: a marching dashed line that lights up, with its icon bobbing, when the
    /// file is right over it.
    fn drop_target(&self, s: &Scene, area: D2D_RECT_F, targeted: bool, icon: char, text: &str, a: f32) {
        let p = &self.pal;
        let (fill, line) = if targeted { (0.22, 1.0) } else { (0.08, 0.55) };
        self.rounded(area, tokens::RADIUS_CARD, with_alpha(p.accent, fill), a);
        self.marching(area, tokens::RADIUS_CARD, 2.0, s.shelf_fx.dash, with_alpha(p.accent, line), a);
        let bob = if targeted { (s.seconds * 7.0).sin() as f32 * 2.5 } else { 0.0 };
        let color = if targeted { p.text } else { p.soft };
        self.glyph(icon, &self.fonts.icon_20, (area.left + area.right) / 2.0, 66.0 + bob, color, a);
        self.text(text, &self.fonts.caption_center, rect(area.left + 4.0, SHELF_TOP + 48.0, area.right - 4.0, SHELF_TOP + 64.0), color, a);
    }

    /// Short glow after a file is dropped: an accent ring that widens while fading.
    fn drop_glow(&self, area: D2D_RECT_F, fx: &ShelfFx, a: f32) {
        let Some(t) = fx.glow else { return };
        let (fade, grow) = (1.0 - t, 3.0 * t);
        let ring = rect(area.left - grow, area.top - grow, area.right + grow, area.bottom + grow);
        self.rounded(area, tokens::RADIUS_CARD, with_alpha(self.pal.accent, 0.16 * fade), a);
        self.outline(ring, tokens::RADIUS_CARD + grow, 2.0, false, with_alpha(self.pal.accent, fade), a);
    }

    fn focus_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let f = s.focus;
        let today = if f.sessions > 0 { format!("{} today", f.sessions) } else { String::new() };
        // With the shared tab the caption slot holds the Focus | Timer switch, and the session
        // count moves into the status line.
        let merged = self.timer_modes(s, x1, a);
        if !merged {
            self.header(s, Header { right: &today, right_hit: None }, x1, a);
        }

        // The progress ring doubles as the start/pause button.
        let (cx, cy) = (l + 34.0, HERO_Y);
        if s.hovered == Some(Hit::FocusToggle) {
            self.icon_button(cx, cy, 24.0, true, a);
        }
        self.ring(cx, cy, Ring::LARGE, if f.active { f.progress } else { 0.0 }, phase_color(f.phase), a);
        self.glyph(if f.running { ICON_PAUSE } else { ICON_PLAY }, &self.fonts.icon_20, cx, cy, p.text, a);
        self.hit(rect(cx - 34.0, cy - 34.0, cx + 34.0, cy + 34.0), Hit::FocusToggle);

        self.text(&f.clock, &self.fonts.big_num, rect(l + 82.0, HERO_Y - 28.0, r - 44.0, HERO_Y + 12.0), p.text, a);
        let phase = match f.phase {
            Phase::Focus => "Focus time",
            Phase::Break => "Break",
        };
        let state = if f.running {
            "running"
        } else if f.active {
            "paused"
        } else {
            "ready to start"
        };
        let status = if merged && !today.is_empty() { format!("{phase} · {state} · {today}") } else { format!("{phase} · {state}") };
        self.text(&status, &self.fonts.caption, rect(l + 84.0, HERO_Y + 10.0, r - 44.0, HERO_Y + 26.0), p.muted, a);

        if f.active {
            let (bx, by) = (r - 14.0, HERO_Y);
            let over = s.hovered == Some(Hit::FocusReset);
            self.icon_button(bx, by, 15.0, over, a);
            self.glyph(ICON_REFRESH, &self.fonts.icon_13, bx, by, if over { p.text } else { p.soft }, a);
            self.hit(rect(bx - 16.0, by - 16.0, bx + 16.0, by + 16.0), Hit::FocusReset);
        }

        let phases = [(Phase::Focus, "Focus 25 min"), (Phase::Break, "Break 5 min")]
            .map(|(phase, label)| (label, Hit::FocusPhase(phase), f.phase == phase));
        self.segmented(s, l, ROW_Y, &phases, a);
    }

    /// Segmented control starting at `left`, centered on `cy`: one segment per choice, the
    /// current one filled with the accent. Segments fit their text, at least `CONTROL_H` wide.
    /// Returns the right edge.
    fn segmented(&self, s: &Scene, left: f32, cy: f32, items: &[(&str, Hit, bool)], a: f32) -> f32 {
        let p = &self.pal;
        let widths = self.segment_widths(items);
        let right = left + widths.iter().sum::<f32>() + 4.0;
        let half = CONTROL_H / 2.0;
        self.rounded(rect(left, cy - half, right, cy + half), half, self.tint(tokens::FILL_SUBTLE), a);
        let mut x = left + 2.0;
        for ((label, hit, current), width) in items.iter().zip(widths) {
            let inner = rect(x, cy - half + 2.0, x + width, cy + half - 2.0);
            if *current {
                self.rounded(inner, half - 2.0, p.accent, a);
            } else if s.hovered == Some(*hit) {
                self.rounded(inner, half - 2.0, self.tint(tokens::FILL_SOFT), a * self.hover.get());
            }
            let color = if *current { p.on_accent } else { p.soft };
            self.text(label, &self.fonts.label_center, inner, color, a);
            self.hit(inner, *hit);
            x += width;
        }
        right
    }

    /// The buttons under an empty state, centred on the bottom row as one group, so an empty
    /// page reads as one centred block (icon, title, hint, actions).
    fn empty_actions(&self, s: &Scene, l: f32, r: f32, actions: &[(&str, Hit, Style)], a: f32) {
        let widths: Vec<f32> = actions.iter().map(|(label, ..)| self.text_width(label, &self.fonts.label_center) + 32.0).collect();
        let total = widths.iter().sum::<f32>() + TIGHT * (actions.len().saturating_sub(1)) as f32;
        let mut x = (l + r - total) / 2.0;
        for ((label, hit, style), width) in actions.iter().zip(widths) {
            let area = rect(x, ROW_Y - CONTROL_H / 2.0, x + width, ROW_Y + CONTROL_H / 2.0);
            self.chip(area, label, *style, s.hovered == Some(*hit), a);
            self.hit(area, *hit);
            x += width + TIGHT;
        }
    }

    /// One key of the tab row: the soft accent plate under the current choice (`k` = how
    /// current, for the slide between pages), a soft plate on hover. Returns the colour for the
    /// icon or name on it. The tab bar and the Focus | Timer switch both use it, so a choice in
    /// the top row always looks the same.
    fn tab_key(&self, key: D2D_RECT_F, k: f32, over: bool, a: f32) -> D2D1_COLOR_F {
        let p = &self.pal;
        let radius = (key.bottom - key.top) / 2.0;
        if k > 0.0 {
            self.rounded(key, radius, with_alpha(p.accent, 0.2 * k), a);
        } else if over {
            self.rounded(key, radius, self.tint(tokens::FILL_SOFT), a * self.hover.get());
        }
        blend(if over { p.text } else { p.muted }, p.accent, k)
    }

    /// Focus | Timer switch in the caption slot of the shared timer tab. Drawn only while the
    /// two share a tab; returns whether it was drawn.
    fn timer_modes(&self, s: &Scene, x1: f32, a: f32) -> bool {
        if s.pages.tab(Page::Timer) != Page::Focus {
            return false;
        }
        let r = x1 - MARGIN;
        let half = tokens::TAB_H / 2.0;
        // Same capsule, key height, and 2 DIP inset as the tab bar beside it.
        let items = [(Page::Focus, "Focus"), (Page::Timer, "Timer")];
        let widths: Vec<f32> = items.iter().map(|(_, l)| self.text_width(l, &self.fonts.caption_center) + 16.0).collect();
        let mut x = r - widths.iter().sum::<f32>() - 4.0;
        self.rounded(rect(x, TOP_Y - half - 2.0, r, TOP_Y + half + 2.0), half + 2.0, self.tint(tokens::FILL_SUBTLE), a);
        x += 2.0;
        for ((page, name), width) in items.into_iter().zip(widths) {
            let key = rect(x, TOP_Y - half, x + width, TOP_Y + half);
            let hit = Hit::TimerMode(page);
            let k = if s.page == page { 1.0 } else { 0.0 };
            let color = self.tab_key(key, k, s.hovered == Some(hit), a);
            // A name on the current key reads in text colour, like the tab bar's page name.
            self.text(name, &self.fonts.caption_center, key, if k > 0.0 { self.pal.text } else { color }, a);
            self.hit(key, hit);
            x += width;
        }
        true
    }

    /// Segment widths of a segmented control; the whole control is their sum plus 4.
    fn segment_widths(&self, items: &[(&str, Hit, bool)]) -> Vec<f32> {
        items.iter().map(|(label, ..)| (self.text_width(label, &self.fonts.label_center) + 20.0).max(CONTROL_H + 6.0)).collect()
    }

    fn water_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let w = s.water;
        let right = if w.ml >= w.goal {
            "Today's goal reached".to_string()
        } else {
            w.next.as_ref().map(|t| format!("Drink at {t}")).unwrap_or_default()
        };
        self.header(s, Header { right: &right, right_hit: None }, x1, a);

        self.text(&format!("{} L", liters(w.ml)), &self.fonts.big_num, rect(l, 35.0, l + 140.0, 73.0), p.text, a);
        let sub = format!("of {} L · {} glasses", liters(w.goal), w.ml / GLASS_ML);
        self.text(&sub, &self.fonts.caption, rect(l + 2.0, 73.0, l + 146.0, 89.0), p.muted, a);

        // Chart of the last seven days; today rightmost and colored.
        let base = 88.0;
        let goal = w.goal.max(1) as f32;
        for (i, (ml, initial)) in w.week.iter().enumerate() {
            let x = l + 150.0 + i as f32 * 16.0;
            let today = i + 1 == w.week.len();
            let height = 4.0 + 36.0 * (*ml as f32 / goal).min(1.0);
            self.rounded(rect(x, base - height, x + 8.0, base), 3.0, if today { BLUE } else { self.tint(0.28) }, a);
            self.text(&initial.to_string(), &self.fonts.tiny, rect(x - 4.0, 93.0, x + 12.0, 105.0), if today { p.soft } else { p.muted }, a);
        }

        let done = (w.ml as f32 / goal).min(1.0);
        self.ring(r - 30.0, HERO_Y, Ring::WATER, done, BLUE, a);
        self.text(&format!("{}%", (done * 100.0).round()), &self.fonts.label_center, rect(r - 56.0, HERO_Y - 10.0, r - 4.0, HERO_Y + 10.0), p.text, a);

        let drink = rect(l, ROW_Y - CONTROL_H / 2.0, l + 136.0, ROW_Y + CONTROL_H / 2.0);
        let over = s.hovered == Some(Hit::WaterDrink);
        self.chip(drink, "+ Drink 250 ml", Style::OnBlue, over, a);
        self.hit(drink, Hit::WaterDrink);
        if w.ml > 0 {
            let undo = rect(l + 144.0, ROW_Y - CONTROL_H / 2.0, l + 206.0, ROW_Y + CONTROL_H / 2.0);
            let over = s.hovered == Some(Hit::WaterUndo);
            self.chip(undo, "Undo", Style::Ghost, over, a);
            self.hit(undo, Hit::WaterUndo);
        }
    }

    /// Prayer times: the next azan as the page's hero, the day's six times as tiles, and when
    /// to be reminded. Without a city the page asks for one (the same city as the weather).
    fn prayer_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let v = s.prayer;
        // The caption names the city; clicking it types a new one.
        let caption = v.status.as_deref().or(v.city.as_deref()).unwrap_or("");
        let caption_hit = (v.city.is_some() && s.city_draft.is_none()).then_some(Hit::CityInput);
        self.header(s, Header { right: caption, right_hit: caption_hit }, x1, a);
        let row = rect(l, ROW_Y - CONTROL_H / 2.0, r, ROW_Y + CONTROL_H / 2.0);

        if v.city.is_none() {
            let hint = v.status.as_deref().unwrap_or("Worked out on this PC from your city");
            let area = rect(l, EMPTY_AREA.0, r, EMPTY_AREA.1);
            self.empty_state(area, Some(ICON_PRAYER), "Prayer times for your city", hint, a);
        }
        if let Some(draft) = s.city_draft {
            self.typing_box(row, draft, "Type a city name, Enter to save", a);
            self.hit(row, Hit::CityInput);
            return;
        }
        if v.city.is_none() {
            self.empty_actions(s, l, r, &[("Set city", Hit::CityInput, Style::Primary)], a);
            return;
        }

        // Hero: the coming prayer and how long until it.
        if let Some((name, time, left)) = &v.next {
            self.text(name, &self.fonts.title, rect(l, 38.0, r, 58.0), p.text, a);
            self.text(&format!("{time} · {left}"), &self.fonts.caption, rect(l, 58.0, r, 72.0), p.muted, a);
        }

        // The day as six even tiles; the next one carries the accent, passed ones step back.
        let (top, bottom) = (80.0, 116.0);
        let count = v.times.len().max(1) as f32;
        let width = (r - l - PRAYER_GAP * (count - 1.0)) / count;
        for (i, t) in v.times.iter().enumerate() {
            let x = l + i as f32 * (width + PRAYER_GAP);
            let area = rect(x, top, x + width, bottom);
            if t.next {
                self.rounded(area, tokens::RADIUS_ROW, with_alpha(p.accent, tokens::FILL_STRONG), a);
                self.outline(area, tokens::RADIUS_ROW, 1.0, false, with_alpha(p.accent, 0.5), a);
            } else {
                self.rounded(area, tokens::RADIUS_ROW, self.tint(tokens::FILL_GHOST), a);
            }
            let (name, time) = match (t.next, t.passed) {
                (true, _) => (p.accent, p.text),
                (false, true) => (p.dim, p.muted),
                (false, false) => (p.muted, p.soft),
            };
            self.text(t.name, &self.fonts.caption_center, rect(x, top + 4.0, x + width, top + 18.0), name, a);
            self.text(&t.time, &self.fonts.label_center, rect(x, top + 17.0, x + width, bottom - 3.0), time, a);
        }

        // Reminder before each azan, and the calculation method (click to step through).
        self.text("Remind", &self.fonts.caption, rect(l, ROW_Y - 8.0, l + 50.0, ROW_Y + 8.0), p.muted, a);
        let labels = prayer::LEADS.map(|lead| if lead == 0 { "Azan".to_string() } else { format!("{lead}m") });
        let leads: Vec<(&str, Hit, bool)> = prayer::LEADS
            .iter()
            .zip(&labels)
            .map(|(lead, label)| (label.as_str(), Hit::Command(Command::PrayerLead(*lead)), v.lead == *lead))
            .collect();
        self.segmented(s, l + 50.0, ROW_Y, &leads, a);
        let method = v.method.label();
        let width = self.text_width(method, &self.fonts.label_center) + 28.0;
        let chip = rect(r - width, row.top, r, row.bottom);
        let hit = Hit::Command(Command::PrayerMethod(v.method.next()));
        self.chip(chip, method, Style::Secondary, s.hovered == Some(hit), a);
        self.hit(chip, hit);
    }

    /// Two-line list cell, as in an iOS list: `lead` DIP on the left for a dot or avatar the
    /// caller draws, the title with trailing text on the first line, the subtitle below. Inert
    /// at rest, a soft fill on hover. Returns the row area and the two line centres.
    fn list_cell(&self, top: f32, l: f32, r: f32, cell: Cell, hovered: bool, a: f32) -> (D2D_RECT_F, f32, f32) {
        let area = rect(l - 6.0, top + 1.0, r + 6.0, top + LIST_ROW_H - 1.0);
        self.list_row(area, tokens::RADIUS_ROW, hovered, a);
        let (y1, y2) = (top + 13.0, top + 28.0);
        let trailing_w = if cell.trailing.is_empty() { 0.0 } else { self.text_width(cell.trailing, &self.fonts.caption_end).max(24.0) };
        if trailing_w > 0.0 {
            self.text(cell.trailing, &self.fonts.caption_end, rect(r - trailing_w - 2.0, y1 - 8.0, r, y1 + 8.0), cell.trailing_color, a);
        }
        let text_l = l + cell.lead;
        let title_r = if trailing_w > 0.0 { r - trailing_w - 10.0 } else { r };
        self.text(cell.title, &self.fonts.label, rect(text_l, y1 - 9.0, title_r, y1 + 9.0), cell.title_color, a);
        if !cell.subtitle.is_empty() {
            self.text(cell.subtitle, &self.fonts.caption, rect(text_l, y2 - 8.0, r - cell.subtitle_inset, y2 + 8.0), cell.subtitle_color, a);
        }
        (area, y1, y2)
    }

    fn tasks_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let tasks = s.tasks;
        let remaining = tasks.remaining();
        let right = if tasks.items.is_empty() {
            String::new()
        } else if remaining == 0 {
            "All done".to_string()
        } else {
            format!("{remaining} left")
        };
        self.header(s, Header { right: &right, right_hit: None }, x1, a);

        self.hit(rect(l - 6.0, tokens::DECK_TOP, r + 6.0, 124.0), Hit::TaskList);
        let order = tasks.order();
        if order.is_empty() {
            self.empty_state(rect(l, EMPTY_AREA.0, r, EMPTY_AREA.1), Some(ICON_TASKS), "No tasks yet", "Add one below", a);
        }
        for (row, &index) in order.iter().skip(tasks.scroll).take(TASKS_VISIBLE).enumerate() {
            let task = &tasks.items[index];
            let y = TASKS_TOP + row as f32 * ROW_H;
            let hovered = matches!(s.hovered, Some(Hit::TaskToggle(i) | Hit::TaskRemove(i)) if i == index);
            self.list_row(rect(l - 6.0, y - 11.0, r + 6.0, y + 11.0), tokens::RADIUS_ROW, hovered, a);
            let cx = l + 9.0;
            if task.done {
                self.circle(cx, y, 8.0, p.accent, a);
                self.glyph(ICON_CHECK, &self.fonts.icon_9, cx, y, WHITE, a);
            } else {
                self.circle_outline(cx, y, 7.0, 1.5, if hovered { p.text } else { p.muted }, a);
            }
            let area = rect(l + 26.0, y - 10.0, r - 26.0, y + 10.0);
            if task.done {
                self.text_struck(&task.text, &self.fonts.body, area, p.muted, a);
            } else {
                self.text(&task.text, &self.fonts.body, area, p.text, a);
            }
            self.hit(rect(l - 6.0, y - 11.0, r - 20.0, y + 11.0), Hit::TaskToggle(index));
            if hovered {
                let over = s.hovered == Some(Hit::TaskRemove(index));
                self.glyph(ICON_CLOSE, &self.fonts.icon_9, r - 8.0, y, if over { RED } else { p.muted }, a);
                self.hit(rect(r - 20.0, y - 11.0, r + 6.0, y + 11.0), Hit::TaskRemove(index));
            }
        }
        // Markers for more tasks above/below; scrolled with the mouse wheel.
        if tasks.scroll > 0 {
            self.glyph(ICON_CHEVRON_UP, &self.fonts.icon_9, (l + r) / 2.0, TASKS_TOP - 17.0, p.muted, a);
        }
        if order.len() > tasks.scroll + TASKS_VISIBLE {
            self.glyph(ICON_CHEVRON_DOWN, &self.fonts.icon_9, (l + r) / 2.0, 121.0, p.muted, a);
        }

        // Add-task box, and the clear-finished button.
        let has_done = tasks.has_done();
        let input = rect(l, ROW_Y - CONTROL_H / 2.0, if has_done { r - 112.0 } else { r }, ROW_Y + CONTROL_H / 2.0);
        let over = s.hovered == Some(Hit::TaskInput);
        match &tasks.draft {
            Some(draft) => self.typing_box(input, draft, "Type a task, Enter to save", a),
            None => self.text_field(input, "", "+  Add task", over, a),
        }
        self.hit(input, Hit::TaskInput);
        if has_done {
            let clear = rect(r - 104.0, ROW_Y - CONTROL_H / 2.0, r, ROW_Y + CONTROL_H / 2.0);
            let over = s.hovered == Some(Hit::TasksClearDone);
            self.chip(clear, "Clear done", Style::Ghost, over, a);
            self.hit(clear, Hit::TasksClearDone);
        }
    }

    /// Coding agent sessions, kept quiet: two roomy rows, the rest summed up as "+N more".
    /// Per row a state dot, the title, and a short status; a dim second line names the agent
    /// (in its tint) and project. The waiting reason only shows for a
    /// session that needs you. Rows scroll with the mouse wheel; clicking one switches to the
    /// agent's terminal or editor.
    fn agents_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let rows = s.agents;
        if rows.is_empty() {
            self.header(s, Header { right: "Refresh", right_hit: Some(Hit::AgentsRefresh) }, x1, a);
            self.empty_state(
                rect(l, EMPTY_AREA.0, r, EMPTY_AREA.1),
                Some(ICON_TERMINAL),
                "No agents at work",
                "Claude Code, Codex, Cursor, Copilot CLI, and omp appear here live",
                a,
            );
            self.empty_actions(s, l, r, &[("Install hooks", Hit::AgentHooks, Style::Primary)], a);
            return;
        }
        let waiting = rows.iter().filter(|row| row.state == AgentState::Waiting).count();
        self.header(s, Header { right: "", right_hit: None }, x1, a);
        if waiting > 0 {
            // A badge, not another line of text: the count that matters reads at a glance.
            self.badge(r - 18.0, TOP_Y, &waiting.to_string(), Some(ORANGE), a);
        }

        self.hit(rect(l - 6.0, AGENT_TOP, r + 6.0, AGENT_TOP + AGENT_ROW_H * VISIBLE_AGENTS as f32), Hit::AgentList);
        for (index, session) in rows.iter().enumerate().skip(s.agent_scroll).take(VISIBLE_AGENTS) {
            let top = AGENT_TOP + (index - s.agent_scroll) as f32 * AGENT_ROW_H;
            let (y1, y2) = (top + 13.0, top + 28.0);
            let row = rect(l - 6.0, top + 1.0, r + 6.0, top + AGENT_ROW_H - 1.0);
            self.list_row(row, tokens::RADIUS_ROW, s.hovered == Some(Hit::AgentRow(index)), a);
            self.hit(row, Hit::AgentRow(index));

            let color = agent_color(session.state, p.accent, p.dim);
            if session.state == AgentState::Running {
                self.circle_outline(l + 4.0, y1, 3.2, 1.6, color, a);
            } else {
                self.circle(l + 4.0, y1, 3.5, color, a);
            }
            let status = agent_status(session);
            let status_w = self.text_width(&status, &self.fonts.caption_end);
            let status_color = if session.state == AgentState::Waiting { ORANGE } else { p.muted };
            self.text(&status, &self.fonts.caption_end, rect(r - status_w - 2.0, y1 - 8.0, r, y1 + 8.0), status_color, a);
            self.text(&session.title, &self.fonts.label, rect(l + 14.0, y1 - 10.0, r - status_w - 12.0, y1 + 10.0), p.text, a);

            // Agent name in its tint, then project (and the waiting reason) in muted text.
            let kind = session.kind.label();
            let kind_w = self.text_width(kind, &self.fonts.caption);
            self.text(kind, &self.fonts.caption, rect(l + 14.0, y2 - 8.0, l + 16.0 + kind_w, y2 + 8.0), rgb(session.kind.tint(), 1.0), a);
            let mut parts = vec![session.project.as_str()];
            if session.state == AgentState::Waiting {
                parts.push(session.detail.as_str());
            }
            let rest: String = parts.into_iter().filter(|part| !part.is_empty()).map(|part| format!(" · {part}")).collect();
            let folder_right = if !session.folder.is_empty() { r - 24.0 } else { r };
            self.text(&rest, &self.fonts.caption, rect(l + 14.0 + kind_w, y2 - 8.0, folder_right, y2 + 8.0), p.muted, a);
            if !session.folder.is_empty() && s.hovered == Some(Hit::AgentRow(index)) {
                self.glyph(ICON_FOLDER, &self.fonts.icon_11, r - 9.0, y2, p.accent, a);
                self.hit(rect(r - 21.0, y2 - 10.0, r + 3.0, y2 + 10.0), Hit::AgentFolder(index));
            }
        }
        let hidden = rows.len().saturating_sub(s.agent_scroll + VISIBLE_AGENTS);
        if hidden > 0 {
            self.text(&format!("+{hidden} more"), &self.fonts.caption, rect(l, ROW_Y - 8.0, r - 110.0, ROW_Y + 8.0), p.muted, a);
        }
        // Once every hook is in place the link turns into a quiet "Hooks on" marker, so the
        // row stops asking for something already done.
        let over = s.hovered == Some(Hit::AgentHooks);
        let installed = s.agent_hooks.all();
        let label = if installed { "Hooks on" } else { "Install hooks" };
        let color = if installed && !over { p.muted } else if over { p.text } else { p.accent };
        let width = self.text_width(label, &self.fonts.caption_end) + 16.0;
        self.text(label, &self.fonts.caption_end, rect(r - width, ROW_Y - 8.0, r, ROW_Y + 8.0), color, a);
        self.hit(rect(r - width - 4.0, ROW_Y - 11.0, r + 4.0, ROW_Y + 11.0), Hit::AgentHooks);
    }

    /// Unread mail from the connected Google account. Without an account the page holds the
    /// one-time setup: paste the client id and secret, then connect.
    fn mail_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let m = s.mail;
        if m.setup {
            self.header(s, Header { right: "", right_hit: None }, x1, a);
            let title = if m.id.is_empty() { "Connect Google" } else { "Finish connecting" };
            // The action sits on the title row, the way a sheet carries Done in its bar, so the
            // boxes below keep the whole width and everything stays inside the deck.
            let label = if m.connecting { "Cancel" } else { "Connect…" };
            let width = self.text_width(label, &self.fonts.label_center) + 32.0;
            let button = rect(r - width, MAIL_SETUP_TITLE - CONTROL_H / 2.0, r, MAIL_SETUP_TITLE + CONTROL_H / 2.0);
            self.text(title, &self.fonts.title, rect(l, MAIL_SETUP_TITLE - 10.0, button.left - TIGHT, MAIL_SETUP_TITLE + 10.0), p.text, a);
            let over = s.hovered == Some(Hit::GoogleConnect);
            // Cancel, not a dead "Waiting…": Google refuses before redirecting often enough that
            // waiting out the timeout would feel like a freeze.
            self.chip(button, label, if m.connecting { Style::Secondary } else { Style::Primary }, over, a);
            self.hit(button, Hit::GoogleConnect);
            // One line under the title: the last failure when there is one, so a refused sign-in
            // is explained where the next attempt starts; otherwise what to paste.
            let hint = rect(l, MAIL_SETUP_HINT - 10.0, r, MAIL_SETUP_HINT + 10.0);
            match &m.status {
                Some(status) => self.alert(hint, status, ORANGE, a),
                None => self.text(
                    "Desktop app client from Google Cloud Console · read-only",
                    &self.fonts.caption,
                    hint,
                    p.muted,
                    a,
                ),
            }
            let id = rect(l, MAIL_SETUP_ID, r, MAIL_SETUP_ID + CONTROL_H);
            let id_hit = Hit::GoogleClient(0);
            if m.editing == Some(0) {
                self.typing_box(id, &m.id, "Client id", a);
            } else {
                self.text_field(id, &m.id, "Client id (…apps.googleusercontent.com)", s.hovered == Some(id_hit), a);
                self.hit(id, id_hit);
            }
            let secret = rect(l, MAIL_SETUP_SECRET, r, MAIL_SETUP_SECRET + CONTROL_H);
            let secret_hit = Hit::GoogleClient(1);
            if m.editing == Some(1) {
                self.typing_box(secret, &m.secret, "Client secret", a);
            } else {
                self.text_field(secret, &m.secret, "Client secret", s.hovered == Some(secret_hit), a);
                self.hit(secret, secret_hit);
            }
            return;
        }
        let count = m.rows.len();
        let right = match (&m.status, count) {
            (Some(status), _) => status.clone(),
            (None, 0) => "Nothing unread".to_string(),
            (None, 1) => "1 unread".to_string(),
            (None, n) => format!("{n} unread"),
        };
        self.header(s, Header { right: &right, right_hit: Some(Hit::GoogleOpen) }, x1, a);
        if m.rows.is_empty() {
            let (title, hint) = match &m.status {
                Some(status) => (status.clone(), "Press the caption to open Gmail."),
                None => ("You're all caught up".to_string(), m.account.as_deref().unwrap_or("Gmail")),
            };
            self.empty_state(rect(l, EMPTY_AREA.0, r, EMPTY_AREA.1), Some(ICON_MAIL), &title, hint, a);
            // Reached only with an account connected (otherwise the setup form is drawn above):
            // nothing unread, so the page offers Gmail itself and a way to sign out.
            let actions = [("Open Gmail", Hit::GoogleOpen, Style::Primary), ("Disconnect", Hit::GoogleDisconnect, Style::Secondary)];
            self.empty_actions(s, l, r, &actions, a);
            return;
        }
        for (i, row) in m.rows.iter().take(MAIL_ROWS).enumerate() {
            let top = LIST_TOP + i as f32 * LIST_ROW_H;
            let hit = Hit::MailRow(i);
            let over = s.hovered == Some(hit);
            let cell = Cell {
                lead: 34.0,
                title: &row.from,
                title_color: p.text,
                subtitle: &row.subject,
                subtitle_color: p.muted,
                subtitle_inset: 18.0,
                trailing: &row.age,
                trailing_color: p.dim,
            };
            let (area, _, y2) = self.list_cell(top, l, r, cell, over, a);
            self.hit(area, hit);
            // Avatar: the sender's initials on a quiet plate, so rows line up without a logo
            // per sender (mail carries none that Crest can read).
            let (cx, cy) = (l + 12.0, top + LIST_ROW_H / 2.0);
            self.circle(cx, cy, 12.0, self.tint(tokens::FILL_STRONG), a);
            self.text(row.initials.as_str(), &self.fonts.tiny, rect(cx - 12.0, cy - 8.0, cx + 12.0, cy + 8.0), p.soft, a);
            if over {
                self.glyph(ICON_MAIL_OPEN, &self.fonts.icon_11, r - 6.0, y2, p.accent, a);
            }
        }
        let over = s.hovered == Some(Hit::GoogleOpen);
        let link = rect(l, ROW_Y - CONTROL_H / 2.0, l + self.text_width("Open Gmail", &self.fonts.caption) + 16.0, ROW_Y + CONTROL_H / 2.0);
        self.chip(link, "Open Gmail", Style::Link, over, a);
        self.hit(rect(l - 4.0, ROW_Y - 11.0, link.right + 4.0, ROW_Y + 11.0), Hit::GoogleOpen);
        let hidden = count.saturating_sub(MAIL_ROWS);
        if hidden > 0 {
            self.text(&format!("+{hidden} more"), &self.fonts.caption_end, rect(r - 120.0, ROW_Y - 8.0, r, ROW_Y + 8.0), p.muted, a);
        }
    }

    /// Rows printed by local plugin commands. Clicking a row with a URL opens it.
    fn plugins_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let list = s.plugins;
        if list.empty {
            self.header(s, Header { right: "", right_hit: None }, x1, a);
            self.empty_state(
                rect(l, EMPTY_AREA.0, r, EMPTY_AREA.1),
                Some(ICON_PLUGIN),
                "No plugins yet",
                "Add a .plugin file to the folder; its rows show up here",
                a,
            );
            self.empty_actions(s, l, r, &[("Open folder", Hit::PluginsFolder, Style::Primary)], a);
            return;
        }
        let right = format!("{} plugins", list.count);
        self.header(s, Header { right: &right, right_hit: Some(Hit::PluginsReload) }, x1, a);
        self.hit(rect(l - 6.0, LIST_TOP, r + 6.0, LIST_TOP + LIST_ROW_H * PLUGIN_ROWS as f32), Hit::PluginList);
        let total = list.rows.len() + list.problems.len();
        if total == 0 {
            self.empty_state(rect(l, EMPTY_AREA.0, r, EMPTY_AREA.1), Some(ICON_PLUGIN), "Running…", "", a);
            return;
        }
        for (i, row) in list.rows.iter().chain(&list.problems).take(PLUGIN_ROWS).enumerate() {
            let top = LIST_TOP + i as f32 * LIST_ROW_H;
            let clickable = !row.url.is_empty();
            let hit = Hit::PluginRow(i);
            let failed = i >= list.rows.len();
            let line = if row.detail.is_empty() && clickable { "opens in the browser" } else { row.detail.as_str() };
            let cell = Cell {
                lead: 16.0,
                title: &row.title,
                title_color: if failed { ORANGE } else { p.text },
                subtitle: line,
                subtitle_color: p.muted,
                subtitle_inset: 0.0,
                trailing: &row.plugin,
                trailing_color: p.muted,
            };
            let (area, y1, _) = self.list_cell(top, l, r, cell, clickable && s.hovered == Some(hit), a);
            if clickable {
                self.hit(area, hit);
            }
            self.circle(l + 4.0, y1, 3.5, if failed { RED } else { GREEN }, a);
        }
        let hidden = total.saturating_sub(PLUGIN_ROWS);
        if hidden > 0 {
            self.text(&format!("+{hidden} more"), &self.fonts.caption_end, rect(r - 120.0, ROW_Y - 8.0, r, ROW_Y + 8.0), p.muted, a);
        }
        let over = s.hovered == Some(Hit::PluginsFolder);
        let link = rect(l, ROW_Y - CONTROL_H / 2.0, l + self.text_width("Open folder", &self.fonts.caption) + 16.0, ROW_Y + CONTROL_H / 2.0);
        self.chip(link, "Open folder", Style::Link, over, a);
        self.hit(rect(l - 4.0, ROW_Y - 11.0, link.right + 4.0, ROW_Y + 11.0), Hit::PluginsFolder);
    }

    /// Settings: theme swatches and size on top, then the most used switches. Each control sends
    /// the same command as its tray menu entry; rarer options stay in the tray menu.
    fn settings_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let st = &s.settings;
        self.header(s, Header { right: "", right_hit: None }, x1, a);

        // Theme: each body color with its accent inside; the current one has an accent ring.
        let y = SETTINGS_ROWS[0];
        self.text("Theme", &self.fonts.caption, rect(l, y - 8.0, l + 56.0, y + 8.0), p.muted, a);
        for (i, theme) in Theme::ALL.into_iter().enumerate() {
            let cx = l + 66.0 + i as f32 * SWATCH_PITCH;
            let hit = Hit::Command(Command::Theme(theme));
            let look = palette(theme);
            if st.theme == theme {
                self.circle_outline(cx, y, 13.0, 1.6, p.accent, a);
            } else if s.hovered == Some(hit) {
                self.circle_outline(cx, y, 13.0, 1.2, self.tint(tokens::TRACK), a);
            }
            self.circle(cx, y, 10.0, D2D1_COLOR_F { a: 1.0, ..look.body }, a);
            self.circle_outline(cx, y, 10.0, 1.0, self.tint(tokens::DIVIDER), a);
            self.circle(cx, y, 4.0, look.accent, a);
            self.hit(rect(cx - 14.0, y - 14.0, cx + 14.0, y + 14.0), hit);
        }

        // Size: a small segmented control at the right end of the theme row.
        let sizes = [(Size::Small, "S"), (Size::Medium, "M"), (Size::Large, "L")]
            .map(|(size, short)| (short, Hit::Command(Command::Size(size)), st.size == size));
        let track_w = self.segment_widths(&sizes).iter().sum::<f32>() + 4.0;
        self.segmented(s, r - track_w, y, &sizes, a);
        self.text("Size", &self.fonts.caption_end, rect(r - track_w - 60.0, y - 8.0, r - track_w - TIGHT, y + 8.0), p.muted, a);

        // Feature switches, three per row. The globe marks the ones that use the internet.
        let switches = [
            ("Lyrics", st.lyrics, true, Command::ToggleLyrics),
            ("Weather", st.weather, true, Command::ToggleWeather),
            ("Mail", st.mail, true, Command::ToggleMail),
            ("Calendar", st.calendar, false, Command::ToggleCalendar),
            ("Google Cal", st.google_calendar, true, Command::ToggleGoogleCalendar),
            ("Plugins", st.plugins, false, Command::TogglePlugins),
        ];
        let width = (r - l - 2.0 * TIGHT) / 3.0;
        for (i, (label, on, online, command)) in switches.into_iter().enumerate() {
            let x = l + (i % 3) as f32 * (width + TIGHT);
            let cy = if i < 3 { SETTINGS_ROWS[1] } else { SETTINGS_ROWS[2] };
            let area = rect(x, cy - CONTROL_H / 2.0, x + width, cy + CONTROL_H / 2.0);
            let hit = Hit::Command(command);
            let over = s.hovered == Some(hit);
            self.card(area, tokens::RADIUS_ROW, over, a);
            let knob = rect(area.right - 30.0, cy - 6.0, area.right - 10.0, cy + 6.0);
            let mut text_right = knob.left - 6.0;
            if online {
                self.glyph(ICON_GLOBE, &self.fonts.icon_9, knob.left - 10.0, cy, p.muted, a);
                text_right -= 12.0;
            }
            self.text(label, &self.fonts.caption, rect(x + 10.0, cy - 8.0, text_right, cy + 8.0), if on { p.text } else { p.soft }, a);
            self.switch(knob, on, a);
            self.hit(area, hit);
        }

        // Prayer times: the calculation method steps through on click, the Asr school beside it.
        let cy = SETTINGS_ROWS[3];
        self.text("Prayer", &self.fonts.caption, rect(l, cy - 8.0, l + 76.0, cy + 8.0), p.muted, a);
        let method = st.prayer_method.label();
        let width = self.text_width(method, &self.fonts.label_center) + 28.0;
        let chip = rect(l + 82.0, cy - CONTROL_H / 2.0, l + 82.0 + width, cy + CONTROL_H / 2.0);
        let hit = Hit::Command(Command::PrayerMethod(st.prayer_method.next()));
        self.chip(chip, method, Style::Secondary, s.hovered == Some(hit), a);
        self.hit(chip, hit);
        let schools = prayer::Madhab::ALL.map(|m| (m.label(), Hit::Command(Command::PrayerMadhab(m)), st.prayer_madhab == m));
        let track_w = self.segment_widths(&schools).iter().sum::<f32>() + 4.0;
        self.segmented(s, r - track_w, cy, &schools, a);
        self.text("Asr", &self.fonts.caption_end, rect(chip.right + TIGHT, cy - 8.0, r - track_w - TIGHT, cy + 8.0), p.muted, a);

        // Hook installation is provider-specific and explicit. Codex and omp already expose
        // bounded local session records, so they need no configuration here.
        let cy = SETTINGS_ROWS[4];
        self.text("Agent hooks", &self.fonts.caption, rect(l, cy - 8.0, l + 76.0, cy + 8.0), p.muted, a);
        use crate::feeds::agents::Provider;
        let providers = [Provider::Claude, Provider::Cursor, Provider::Copilot];
        let gap = TIGHT;
        let start = l + 82.0;
        let width = (r - start - gap * 2.0) / 3.0;
        for (i, provider) in providers.into_iter().enumerate() {
            let x = start + i as f32 * (width + gap);
            let area = rect(x, cy - CONTROL_H / 2.0, x + width, cy + CONTROL_H / 2.0);
            let hit = Hit::AgentHook(provider);
            let over = s.hovered == Some(hit);
            // An installed hook is marked with a check, so it is clear which tools report here.
            let on = s.agent_hooks.get(&provider.kind());
            self.card(area, tokens::RADIUS_ROW, over, a);
            let color = if over { p.text } else if on { p.accent } else { p.soft };
            self.text(provider.label(), &self.fonts.caption_center, area, color, a);
            if on {
                self.glyph(ICON_CHECK, &self.fonts.icon_9, area.right - 9.0, cy, p.accent, a);
            }
            self.hit(area, hit);
        }
    }

    /// Small on/off switch: accent track with the knob on the right when on.
    fn switch(&self, track: D2D_RECT_F, on: bool, a: f32) {
        let p = &self.pal;
        let radius = (track.bottom - track.top) / 2.0;
        self.rounded(track, radius, if on { p.accent } else { self.tint(tokens::TRACK) }, a);
        let cx = if on { track.right - radius } else { track.left + radius };
        let knob = if on { p.on_accent } else { p.soft };
        self.circle(cx, (track.top + track.bottom) / 2.0, radius - 2.0, knob, a);
    }

    fn system_page(&self, s: &Scene, x0: f32, x1: f32, a: f32) {
        let p = &self.pal;
        let (l, r) = (x0 + MARGIN, x1 - MARGIN);
        let uptime = format!("Up {}", s.uptime);
        self.header(s, Header { right: &uptime, right_hit: None }, x1, a);

        let percent = |used: u64, total: u64| if total > 0 { used as f32 / total as f32 * 100.0 } else { 0.0 };
        // GPU temperature under its name; orange from 78 °C, red from 87 °C.
        let gpu_detail = |st: &Stats| match (st.gpu, st.gpu_temp) {
            (None, _) => ("unavailable".to_string(), p.muted),
            (Some(_), Some(t)) => (format!("{t:.0}°C"), if t >= 87.0 { RED } else if t >= 78.0 { ORANGE } else { p.muted }),
            (Some(_), None) => (String::new(), p.muted),
        };
        let meters: [(&str, Option<f32>, (String, D2D1_COLOR_F)); 4] = match s.system {
            Some(st) => [
                ("CPU", Some(st.cpu), (String::new(), p.muted)),
                ("GPU", st.gpu, gpu_detail(st)),
                ("RAM", Some(percent(st.ram_used, st.ram_total)), (format!("{} / {}", gigabytes(st.ram_used), gigabytes(st.ram_total)), p.muted)),
                ("Disk C:", Some(percent(st.disk_total.saturating_sub(st.disk_free), st.disk_total)), (format!("{} free", gigabytes(st.disk_free)), p.muted)),
            ],
            None => ["CPU", "GPU", "RAM", "Disk C:"].map(|name| (name, None, ("measuring…".to_string(), p.muted))),
        };
        let spacing = (r - l) / 4.0;
        for (i, (name, value, (detail, detail_color))) in meters.iter().enumerate() {
            let cx = l + spacing * (i as f32 + 0.5);
            let color = value.map_or(p.dim, |v| load_color(v, p.accent));
            self.ring(cx, 66.0, Ring::STAT, value.unwrap_or(0.0) / 100.0, color, a);
            let label = value.map_or("–".to_string(), |v| format!("{}%", v.round()));
            self.text(&label, &self.fonts.label_center, rect(cx - 20.0, 56.0, cx + 20.0, 76.0), p.text, a);
            self.text(name, &self.fonts.caption_center, rect(cx - spacing / 2.0, 92.0, cx + spacing / 2.0, 106.0), p.soft, a);
            self.text(detail, &self.fonts.tiny, rect(cx - spacing / 2.0, 106.0, cx + spacing / 2.0, 118.0), *detail_color, a);
        }

        // Bottom row: network on the left, privacy on the right.
        let network = s.system.map_or(String::new(), |st| format!("↓ {}   ↑ {}", rate(st.down), rate(st.up)));
        self.text(&network, &self.fonts.caption, rect(l, ROW_Y - 8.0, l + 170.0, ROW_Y + 8.0), p.muted, a);
        let privacy = s.privacy;
        let (icon, color, text) = match (privacy.camera.first(), privacy.microphone.first()) {
            (Some(app), _) => (ICON_CAMERA, GREEN, format!("Camera: {app}")),
            (None, Some(app)) => (ICON_MICROPHONE, ORANGE, format!("Microphone: {app}")),
            (None, None) => (ICON_MICROPHONE, p.muted, "Camera & microphone not in use".to_string()),
        };
        self.text(&text, &self.fonts.caption_end, rect(l + 176.0, ROW_Y - 8.0, r, ROW_Y + 8.0), color, a);
        let width = self.text_width(&text, &self.fonts.caption_end).min(r - l - 190.0);
        self.glyph(icon, &self.fonts.icon_11, r - width - 10.0, ROW_Y, color, a);
    }
}

/// Every list page (Agents, Mail, Plugins) has the same shape: two roomy two-line rows that
/// start a little inside the deck, then a footer row at `ROW_Y` for the page's action and the
/// "+N more" count. One row height and one top, so the pages read as one family.
const LIST_ROWS: usize = 2;
const LIST_ROW_H: f32 = 40.0;
const LIST_TOP: f32 = tokens::DECK_TOP + 8.0;
const MAIL_ROWS: usize = LIST_ROWS;
const PLUGIN_ROWS: usize = LIST_ROWS;
/// One toggle tile on the Controls page: icon, name, state line, the fill while on, its click.
type Tile = (char, String, String, Option<D2D1_COLOR_F>, Option<Hit>);

/// Content of one [`Renderer::list_cell`].
struct Cell<'a> {
    lead: f32,
    title: &'a str,
    title_color: D2D1_COLOR_F,
    subtitle: &'a str,
    subtitle_color: D2D1_COLOR_F,
    /// Room kept free at the right of the subtitle, e.g. for a hover action.
    subtitle_inset: f32,
    trailing: &'a str,
    trailing_color: D2D1_COLOR_F,
}

/// Where an empty page centres its icon, title, and hint: between the deck's top and the footer.
const EMPTY_AREA: (f32, f32) = (tokens::DECK_TOP + 12.0, ROW_Y - CONTROL_H / 2.0 - 8.0);
/// The Google setup boxes and the Connect button under them. These are page coordinates, like
/// `ROW_Y`; the renderer adds `CONTENT_SHIFT`, so the button's bottom lands at 170 in the panel.
const MAIL_SETUP_TITLE: f32 = tokens::DECK_TOP + PAD + CONTROL_H / 2.0;
const MAIL_SETUP_HINT: f32 = MAIL_SETUP_TITLE + 24.0;
const MAIL_SETUP_ID: f32 = MAIL_SETUP_HINT + 18.0;
const MAIL_SETUP_SECRET: f32 = MAIL_SETUP_ID + CONTROL_H + 6.0;

/// Bottom of the last row of each list, measured from the page's top: three rows, then the
/// link at `ROW_Y`. The panels are 176 DIP tall and content starts `CONTENT_SHIFT` lower.
const _: () = assert!(MAIL_SETUP_SECRET + CONTROL_H <= tokens::DECK_BOTTOM - 8.0);
const _: () = assert!(LIST_TOP + LIST_ROWS as f32 * LIST_ROW_H <= tokens::ROW_Y - CONTROL_H / 2.0 - 4.0);

/// The lyrics column's card, inside the page deck. It starts below the deck's top edge (the deck
/// is drawn from `DECK_TOP`) so the deck's own square top cannot cut across the card's rounded
/// corners, which is what left a straight line along the card's top.
pub(super) const LYRIC_TOP: f32 = tokens::DECK_TOP + PAD;
pub(super) const LYRIC_BOTTOM: f32 = tokens::DECK_BOTTOM - PAD;
/// Every page keeps this much room between the deck's edge and its content, on all four
/// sides; the lyrics card is that content block for its column.
const PAD: f32 = 16.0;
/// Corner of every card and tile on a page (lyrics card, prayer and control tiles, rows).
const RADIUS_LYRIC: f32 = tokens::RADIUS_ROW;
/// Padding inside the lyrics card, the same on all four sides; the sung line and the next one
/// stay inside it (checked below).
const LYRIC_PAD: f32 = 12.0;
const _: () = assert!(tokens::LYRIC_CENTER - 22.0 >= LYRIC_TOP + LYRIC_PAD - 2.0);
const _: () = assert!(tokens::LYRIC_CENTER + tokens::LYRIC_GAP + 10.0 <= LYRIC_BOTTOM - LYRIC_PAD);
/// The card must sit inside the deck (a deck line across its rounded corners is what looked
/// ragged) and be tall enough for both corners.
const _: () = assert!(LYRIC_TOP > tokens::DECK_TOP);
const _: () = assert!(LYRIC_BOTTOM < tokens::DECK_BOTTOM);
const _: () = assert!(LYRIC_BOTTOM - LYRIC_TOP > 2.0 * RADIUS_LYRIC);

/// Distance between theme swatches on the settings page.
const SWATCH_PITCH: f32 = 28.0;

/// Agent rows that fit in the panel; they use the shared list rows.
const VISIBLE_AGENTS: usize = crate::feeds::agents::VISIBLE;
const AGENT_ROW_H: f32 = LIST_ROW_H;
const AGENT_TOP: f32 = LIST_TOP;

/// Dot color per agent state.
fn agent_color(state: AgentState, accent: D2D1_COLOR_F, idle: D2D1_COLOR_F) -> D2D1_COLOR_F {
    match state {
        AgentState::Waiting => ORANGE,
        AgentState::Running => GREEN,
        AgentState::Done => accent,
        AgentState::Idle => idle,
    }
}

/// Status at the right of an agent row's top line: what state it is in, and since when.
fn agent_status(session: &Session) -> String {
    match session.state {
        AgentState::Waiting => "Needs you".to_string(),
        AgentState::Running => "Working".to_string(),
        AgentState::Done => format!("Done · {}", age(session.age_secs)),
        AgentState::Idle => age(session.age_secs),
    }
}

/// Age of the last activity in short form: "12s", "5m", "3h", "2d".
fn age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Tab bar glyph per page; `None` draws the water drop shape.
fn page_icon(page: Page) -> Option<char> {
    match page {
        Page::Home => Some(ICON_HOME),
        Page::Controls => Some(ICON_VOLUME[3]),
        Page::Shelf => Some(ICON_FOLDER),
        Page::Focus => Some(ICON_STOPWATCH),
        Page::Timer => Some(ICON_TIMER),
        Page::Water => None,
        Page::Prayer => Some(ICON_PRAYER),
        Page::Tasks => Some(ICON_TASKS),
        Page::System => Some(ICON_SYSTEM),
        Page::Agents => Some(ICON_TERMINAL),
        Page::Mail => Some(ICON_MAIL),
        Page::Plugins => Some(ICON_PLUGIN),
        Page::Settings => Some(ICON_SETTINGS),
    }
}

/// Speaker icon for the volume level.
fn volume_icon(level: f32) -> char {
    if level < 0.005 { ICON_VOLUME[0] } else { ICON_VOLUME[((level * 3.0).ceil() as usize).clamp(1, 3)] }
}

/// Width of a navigation icon slot after reserving the page caption. Icons are 13 DIP, so the
/// 14 DIP floor remains legible in the worst case: every page plus a 190 DIP caption.
/// Name shown on a tab. Kept short: the row has twelve slots beside a caption, so a tab's name
/// has to earn its width. Menus keep using `Page::label` for the full name.
fn label(page: Page) -> &'static str {
    match page {
        Page::Home => "Home",
        Page::Controls => "Audio",
        Page::Shelf => "Shelf",
        Page::Focus => "Focus",
        Page::Timer => "Timer",
        Page::Water => "Water",
        Page::Prayer => "Prayer",
        Page::Tasks => "Tasks",
        Page::System => "System",
        Page::Agents => "Agents",
        Page::Mail => "Mail",
        Page::Plugins => "Add-ons",
        Page::Settings => "Settings",
    }
}

fn navigation_slot(available: f32, count: usize) -> f32 {
    if count == 0 {
        return tokens::TAB_SLOT;
    }
    ((available - TIGHT).max(0.0) / count as f32).clamp(14.0, tokens::TAB_SLOT)
}

/// Battery icon, its color, and the label for the power notice.
fn power_look(plugged: bool, percent: Option<u32>, low: bool, normal: D2D1_COLOR_F) -> (char, D2D1_COLOR_F, &'static str) {
    let level = percent.map_or(10, |p| ((p + 5) / 10).min(10));
    let base = if plugged { ICON_BATTERY_CHARGING } else { ICON_BATTERY };
    let icon = char::from_u32(base + level).unwrap_or(ICON_MUTE);
    let color = if low {
        RED
    } else if plugged {
        GREEN
    } else {
        normal
    };
    let label = match (plugged, low, percent) {
        (_, true, _) => "Battery low",
        (true, _, Some(100)) => "Battery full",
        (true, _, _) => "Charging",
        (false, _, _) => "On battery",
    };
    (icon, color, label)
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn navigation_reserves_a_long_caption_without_hiding_pages() {
        let available = 186.0;
        let pages = 12;
        let slot = navigation_slot(available, pages);
        assert!(slot >= 14.0);
        assert!(slot * pages as f32 + TIGHT <= available);
    }
}
