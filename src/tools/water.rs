//! Daily water log. Stored by date in `water.txt`, one line per day.

use crate::shell::store;

pub const GOAL_ML: u32 = 2_000;
pub const GLASS_ML: u32 = 250;
const FILE: &str = "water.txt";
/// Days of history kept; enough for the weekly chart with some margin.
const KEEP_DAYS: usize = 30;

#[derive(Default)]
pub struct Water {
    /// (date "YYYY-MM-DD", ml), oldest first.
    history: Vec<(String, u32)>,
}

impl Water {
    pub fn load() -> Self {
        Self::parse(store::load(FILE))
    }

    pub fn save(&self) {
        store::save(FILE, self.lines());
    }

    fn parse(lines: Vec<String>) -> Self {
        let mut history: Vec<(String, u32)> = lines
            .iter()
            .filter_map(|line| {
                let (day, ml) = line.split_once(' ')?;
                Some((day.to_string(), ml.trim().parse().ok()?))
            })
            .collect();
        history.sort_by(|a, b| a.0.cmp(&b.0));
        Self { history }
    }

    fn lines(&self) -> Vec<String> {
        self.history.iter().map(|(day, ml)| format!("{day} {ml}")).collect()
    }

    pub fn on(&self, day: &str) -> u32 {
        self.history.iter().find(|(d, _)| d == day).map_or(0, |(_, ml)| *ml)
    }

    pub fn drink(&mut self, day: &str) {
        let ml = self.on(day) + GLASS_ML;
        self.set(day, ml);
    }

    /// Undo today's last glass.
    pub fn undo(&mut self, day: &str) {
        let ml = self.on(day).saturating_sub(GLASS_ML);
        self.set(day, ml);
    }

    fn set(&mut self, day: &str, ml: u32) {
        match self.history.iter_mut().find(|(d, _)| d == day) {
            Some(entry) => entry.1 = ml,
            None => {
                self.history.push((day.to_string(), ml));
                self.history.sort_by(|a, b| a.0.cmp(&b.0));
            }
        }
        let excess = self.history.len().saturating_sub(KEEP_DAYS);
        self.history.drain(..excess);
    }
}

/// Liters with a decimal point: 1250 → "1.25", 1000 → "1", 1500 → "1.5".
pub fn liters(ml: u32) -> String {
    let text = format!("{:.2}", ml as f64 / 1000.0);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drink_undo_and_round_trip() {
        let mut water = Water::default();
        water.drink("2026-10-19");
        water.drink("2026-10-19");
        water.undo("2026-10-19");
        water.drink("2026-10-18");
        assert_eq!(water.on("2026-10-19"), GLASS_ML);
        assert_eq!(water.on("2026-10-17"), 0);

        let again = Water::parse(water.lines());
        assert_eq!(again.on("2026-10-18"), GLASS_ML);
        assert_eq!(again.history.first().map(|(d, _)| d.as_str()), Some("2026-10-18"));
    }

    #[test]
    fn undo_never_goes_negative() {
        let mut water = Water::default();
        water.undo("2026-10-19");
        assert_eq!(water.on("2026-10-19"), 0);
    }

    #[test]
    fn keeps_only_recent_days() {
        let mut water = Water::default();
        for day in 1..=40 {
            water.drink(&format!("2026-09-{day:02}"));
        }
        assert_eq!(water.history.len(), KEEP_DAYS);
        assert_eq!(water.on("2026-09-40"), GLASS_ML);
        assert_eq!(water.on("2026-09-01"), 0);
    }

    #[test]
    fn liters_format() {
        assert_eq!(liters(1250), "1.25");
        assert_eq!(liters(1500), "1.5");
        assert_eq!(liters(2000), "2");
        assert_eq!(liters(0), "0");
    }
}
