//! Short notices (HUD) shown in the pill.

use super::*;

impl App {
    /// A HUD with a sound (if notification sounds are on). The sound plays even while the
    /// panel is open, because the news matters.
    pub(super) fn alert(&mut self, hud: Hud, ms: u32, sound: Sound) {
        // In a meeting Crest stays silent (others could hear it), except for the notice that
        // the camera/mic was just turned on. In quiet mode only urgent HUDs make a sound.
        let meeting = self.in_meeting() && sound != Sound::Privacy;
        let quiet = self.quiet() && !urgent(&hud);
        if self.settings.sound && !meeting && !quiet {
            sound::play(sound);
        }
        self.show_hud(hud, ms);
    }

    /// Show a HUD. Ignored while the panel is open or Crest is hidden.
    pub(super) fn show_hud(&mut self, hud: Hud, ms: u32) {
        // In quiet mode only low battery and finished timers still appear.
        if !self.shown || self.expanded || (self.quiet() && !urgent(&hud)) {
            return;
        }
        // Volume/brightness changing again while its HUD is shown: the bar slides smoothly
        // and the number rolls, instead of jumping.
        if let Some(level) = hud_level(&hud) {
            let same = self.hud_visible && self.hud.as_ref().is_some_and(|old| std::mem::discriminant(old) == std::mem::discriminant(&hud));
            if same {
                self.start_motion();
                let before = self.hud.as_ref().and_then(hud_level).unwrap_or(level);
                if let (Some(old), Some(new)) = (self.hud.as_ref().and_then(hud_value), hud_value(&hud))
                    && old != new
                {
                    self.hud_roll = Some((old, self.now(), level >= before));
                }
                self.hud_fill_target = level;
            } else {
                (self.hud_fill, self.hud_fill_target, self.hud_roll) = (level, level, None);
            }
        }
        self.hud = Some(hud);
        self.hud_visible = true;
        self.set_timer(TIMER_HUD, ms);
    }

    /// The last HUD uses the compact shape (also while fading out).
    pub(super) fn compact_hud(&self) -> bool {
        self.settings.compact_hud
            && matches!(self.hud, Some(Hud::Volume { .. } | Hud::Brightness { .. } | Hud::LockKey { .. } | Hud::Power { .. }))
    }
}
