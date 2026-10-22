//! Media controls, seeking, and lyrics.

use super::*;

impl App {
    pub(super) fn media_command(&mut self, button: Button) {
        let Some(m) = &mut self.media else { return };
        let command = match button {
            Button::Previous if m.can_previous => media::Command::Previous,
            Button::Next if m.can_next => media::Command::Next,
            Button::PlayPause => {
                // Show the change immediately; the official update from Windows follows.
                m.position = m.position_now();
                m.at = Instant::now();
                m.playing = !m.playing;
                media::Command::TogglePlayPause
            }
            _ => return,
        };
        self.services.send(Task::MediaCommand(command));
    }

    /// Seek to where the track bar was released; the display moves immediately, the app follows.
    pub(super) fn finish_seek(&mut self) {
        let Some(fraction) = self.seek_preview.take() else { return };
        let Some(m) = self.media.as_mut().filter(|m| m.duration > 0.0) else { return };
        let seconds = fraction as f64 * m.duration;
        m.position = seconds;
        m.at = Instant::now();
        let ticks = ((m.start + seconds) * 1e7) as i64;
        self.services.send(Task::MediaCommand(media::Command::Seek(ticks)));
        // The lyric line jumps to the new position without scrolling.
        self.lyric_scroll = None;
        self.refresh();
    }

    pub(super) fn media_key(&self) -> Option<String> {
        self.media.as_ref().map(|m| lyrics::key(&m.title, &m.artist))
    }

    /// Request lyrics for the playing track, if lyrics are enabled and not fetched yet.
    pub(super) fn request_lyrics(&mut self) {
        if !self.settings.lyrics {
            return;
        }
        let Some(m) = &self.media else { return };
        let key = lyrics::key(&m.title, &m.artist);
        let known = self.lyrics.as_ref().is_some_and(|l| l.key == key) || self.lyric_pending.as_ref() == Some(&key);
        if m.title.is_empty() || known {
            return;
        }
        self.lyric_pending = Some(key.clone());
        self.services.lyrics(lyrics::Request {
            key,
            title: m.title.clone(),
            artist: m.artist.clone(),
            album: m.album.clone(),
            duration: m.duration,
        });
    }

    /// Current lyric line (index and text), if lyrics are enabled and match the track.
    /// Before the first line, the text is a music note.
    pub(super) fn lyric_line(&self) -> Option<(Option<usize>, String)> {
        if !self.settings.lyrics {
            return None;
        }
        let m = self.media.as_ref()?;
        let key = lyrics::key(&m.title, &m.artist);
        let lyrics = self.lyrics.as_ref().filter(|l| l.key == key && !l.lines.is_empty())?;
        let index = lyrics.current(m.position_now());
        Some((index, index.map_or("♪", |i| lyrics.text(i)).to_string()))
    }

    /// The lyrics column beside the player is requested (lyrics button on).
    pub(super) fn lyric_column(&self) -> bool {
        self.settings.lyrics && self.settings.lyric_panel && self.media.is_some()
    }

    pub(super) fn lyric_goal(&self) -> f32 {
        if self.lyric_column() { 1.0 } else { 0.0 }
    }

    /// Current lyric line for the lyrics column; -1 before the first line.
    pub(super) fn lyric_index(&self) -> Option<isize> {
        let m = self.media.as_ref()?;
        let key = lyrics::key(&m.title, &m.artist);
        let lyrics = self.lyrics.as_ref().filter(|l| l.key == key && !l.lines.is_empty())?;
        Some(lyrics.current(m.position_now()).map_or(-1, |i| i as isize))
    }

    /// Lyrics column content: five lines around the current one, or a status while lyrics
    /// are being searched or unavailable. Kept while the column slides out.
    pub(super) fn lyrics_view(&self) -> Option<LyricsView> {
        if !self.settings.lyrics || (!self.lyric_column() && self.lyric_mix <= 0.0) {
            return None;
        }
        let m = self.media.as_ref()?;
        let key = lyrics::key(&m.title, &m.artist);
        let status = |text| Some(LyricsView { lines: Default::default(), scroll: 1.0, status: Some(text) });
        let Some(lyrics) = self.lyrics.as_ref().filter(|l| l.key == key && !l.lines.is_empty()) else {
            return if self.lyric_pending.as_ref() == Some(&key) { status("Searching for lyrics…") } else { status("Lyrics unavailable") };
        };
        let current = lyrics.current(m.position_now()).map_or(-1, |i| i as isize);
        let lines = std::array::from_fn(|k| match current + k as isize - 2 {
            // Before the first line: a music note in place of the line being sung.
            -1 => "♪".to_string(),
            i if i >= 0 && (i as usize) < lyrics.lines.len() => lyrics.text(i as usize).to_string(),
            _ => String::new(),
        });
        let scroll = self.lyric_scroll.map_or(1.0, |start| ((self.now() - start) / LYRIC_SCROLL_SECS).min(1.0) as f32);
        Some(LyricsView { lines, scroll, status: None })
    }

    pub(super) fn playing(&self) -> bool {
        self.media.as_ref().is_some_and(|m| m.playing)
    }
}
