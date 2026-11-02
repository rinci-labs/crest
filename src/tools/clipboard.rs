//! Clipboard history: the last texts copied anywhere, newest first, to copy again with one
//! click. Kept in memory only and never written to disk, so nothing copied outlives Crest.

/// Texts kept; the oldest drop off.
const MAX_ITEMS: usize = 30;
/// Bytes; a bigger copy (a whole log or file) is not kept.
const MAX_BYTES: usize = 100_000;
/// Rows that fit in the panel at once; the rest scroll with the mouse wheel.
pub const VISIBLE: usize = 3;

pub struct Clip {
    pub id: u64,
    pub text: String,
}

impl Clip {
    /// The first line that has text, trimmed, for the row.
    pub fn preview(&self) -> &str {
        self.text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("")
    }

    /// More lines below the one in [`Clip::preview`].
    pub fn more_lines(&self) -> usize {
        self.text.lines().filter(|line| !line.trim().is_empty()).count().saturating_sub(1)
    }
}

#[derive(Default)]
pub struct Clips {
    pub items: Vec<Clip>,
    next_id: u64,
    pub scroll: usize,
}

impl Clips {
    /// Record a copied text at the top. Copying a text already in the list moves it up rather
    /// than adding it twice. Blank or oversized text is ignored.
    pub fn push(&mut self, text: String) {
        if text.trim().is_empty() || text.len() > MAX_BYTES {
            return;
        }
        if self.items.first().is_some_and(|clip| clip.text == text) {
            return;
        }
        self.items.retain(|clip| clip.text != text);
        self.next_id += 1;
        self.items.insert(0, Clip { id: self.next_id, text });
        self.items.truncate(MAX_ITEMS);
        self.scroll = 0;
    }

    pub fn get(&self, id: u64) -> Option<&Clip> {
        self.items.iter().find(|clip| clip.id == id)
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|clip| clip.id != id);
        self.clamp_scroll();
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.scroll = 0;
    }

    pub fn scroll_by(&mut self, step: i32) {
        self.scroll = (self.scroll as i32 + step).max(0) as usize;
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.items.len().saturating_sub(VISIBLE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates() {
        let mut clips = Clips::default();
        for text in ["one", "two", "  ", "three", "one"] {
            clips.push(text.to_string());
        }
        let texts: Vec<&str> = clips.items.iter().map(|clip| clip.text.as_str()).collect();
        assert_eq!(texts, ["one", "three", "two"], "a copy again moves up; blank text is skipped");

        for i in 0..40 {
            clips.push(format!("text {i}"));
        }
        assert_eq!(clips.items.len(), MAX_ITEMS);
        assert_eq!(clips.items[0].text, "text 39");
    }

    #[test]
    fn scroll_stays_in_range() {
        let mut clips = Clips::default();
        (0..5).for_each(|i| clips.push(i.to_string()));
        clips.scroll_by(10);
        assert_eq!(clips.scroll, 2);
        let id = clips.items[4].id;
        clips.remove(id);
        assert_eq!(clips.scroll, 1);
        clips.push("new".to_string());
        assert_eq!(clips.scroll, 0, "a new copy shows at the top");
    }

    #[test]
    fn preview_is_the_first_line_with_text() {
        let clip = Clip { id: 1, text: "\n  fn main() {\n    run();\n}\n".to_string() };
        assert_eq!(clip.preview(), "fn main() {");
        assert_eq!(clip.more_lines(), 2);
    }
}
