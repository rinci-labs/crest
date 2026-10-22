//! Music/video playing on the PC, via the built-in Windows media controls (SMTC).
//! Works for Spotify, YouTube in the browser, Media Player, and other apps that
//! show up in the Windows media controls. All local, no internet.

use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Instant;

use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat,
    BitmapTransform, ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::IRandomAccessStreamReference;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::core::{Result, RuntimeType};

use crate::system::services::{Event, Notifier, Task};

/// Title, artist, or cover art changed.
pub const PROPS: u8 = 1;
/// Play/pause state or available buttons changed.
pub const PLAYBACK: u8 = 2;
/// Position or duration changed.
pub const TIMELINE: u8 = 4;
pub const ALL: u8 = PROPS | PLAYBACK | TIMELINE;

/// Album art side in pixels; sharp enough for 64 DIP at 200% scale.
pub const ART_PX: u32 = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    TogglePlayPause,
    Next,
    Previous,
    /// Jump to a song position, in 100-nanosecond units on the session timeline.
    Seek(i64),
}

/// Album art: premultiplied BGRA pixels, `ART_PX` x `ART_PX`.
pub struct Art {
    pub id: u64,
    pub pixels: Vec<u8>,
    /// Dominant cover color (0xRRGGBB), used to tint the waveform and progress.
    pub accent: Option<u32>,
}

#[derive(Clone)]
pub struct Snapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Position (seconds) at `at`, counted from the timeline start (`start`).
    pub position: f64,
    /// Session timeline start (seconds); seek positions are counted from here.
    pub start: f64,
    pub duration: f64,
    pub rate: f64,
    pub at: Instant,
    pub can_previous: bool,
    pub can_next: bool,
    /// The app allows seeking (the progress bar can be dragged).
    pub can_seek: bool,
    pub art: Option<Arc<Art>>,
}

impl Snapshot {
    /// Current position, estimated from the last position the app reported.
    pub fn position_now(&self) -> f64 {
        let position = if self.playing {
            self.position + self.at.elapsed().as_secs_f64() * self.rate
        } else {
            self.position
        };
        if self.duration > 0.0 { position.clamp(0.0, self.duration) } else { position.max(0.0) }
    }
}

pub struct Monitor {
    manager: Manager,
    session: Option<(Session, [i64; 3])>,
    tasks: Sender<Task>,
    notify: Notifier,
    last: Option<Snapshot>,
    next_art_id: u64,
}

impl Monitor {
    pub fn new(tasks: Sender<Task>, notify: Notifier) -> Result<Self> {
        let manager = Manager::RequestAsync()?.join()?;
        let on_change = tasks.clone();
        manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
            let _ = on_change.send(Task::MediaSessionChanged);
            Ok(())
        }))?;
        let mut monitor = Self { manager, session: None, tasks, notify, last: None, next_art_id: 1 };
        monitor.follow_session();
        Ok(monitor)
    }

    /// Switch to listening to the session Windows now considers current.
    pub fn follow_session(&mut self) {
        self.detach();
        self.last = None;
        let Ok(session) = self.manager.GetCurrentSession() else { return };
        let tokens = [
            session.MediaPropertiesChanged(&changed(&self.tasks, PROPS)).unwrap_or_default(),
            session.PlaybackInfoChanged(&changed(&self.tasks, PLAYBACK)).unwrap_or_default(),
            session.TimelinePropertiesChanged(&changed(&self.tasks, TIMELINE)).unwrap_or_default(),
        ];
        self.session = Some((session, tokens));
    }

    /// Re-read the parts that changed and send them to the Crest window.
    pub fn refresh(&mut self, flags: u8) {
        let snapshot = match &self.session {
            Some((session, _)) => match read(session, flags, self.last.as_ref(), &mut self.next_art_id) {
                Ok(snapshot) => Some(snapshot),
                Err(e) => {
                    error_log!("failed to read media: {e}");
                    None
                }
            },
            None => None,
        };
        // A session with no title that is not playing is not worth showing.
        let snapshot = snapshot.filter(|s| s.playing || !s.title.is_empty());
        self.last = snapshot.clone();
        self.notify.send(Event::Media(snapshot));
    }

    pub fn command(&self, command: Command) {
        let Some((session, _)) = &self.session else { return };
        let operation = match command {
            Command::TogglePlayPause => session.TryTogglePlayPauseAsync(),
            Command::Next => session.TrySkipNextAsync(),
            Command::Previous => session.TrySkipPreviousAsync(),
            Command::Seek(ticks) => session.TryChangePlaybackPositionAsync(ticks),
        };
        if let Err(e) = operation.and_then(|op| op.join()) {
            error_log!("media command {command:?} failed: {e}");
        }
    }

    fn detach(&mut self) {
        if let Some((old, [props, playback, timeline])) = self.session.take() {
            let _ = old.RemoveMediaPropertiesChanged(props);
            let _ = old.RemovePlaybackInfoChanged(playback);
            let _ = old.RemoveTimelinePropertiesChanged(timeline);
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.detach();
    }
}

/// Session event handler that only forwards "something changed" to the service thread.
fn changed<A: RuntimeType + 'static>(tasks: &Sender<Task>, flags: u8) -> TypedEventHandler<Session, A> {
    let tasks = tasks.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = tasks.send(Task::MediaChanged(flags));
        Ok(())
    })
}

fn read(session: &Session, flags: u8, last: Option<&Snapshot>, next_art_id: &mut u64) -> Result<Snapshot> {
    // Title and cover art are only re-read when they actually changed.
    let (title, artist, album, art) = match last.filter(|_| flags & PROPS == 0) {
        Some(last) => (last.title.clone(), last.artist.clone(), last.album.clone(), last.art.clone()),
        None => {
            let props = session.TryGetMediaPropertiesAsync()?.join()?;
            let art = props.Thumbnail().ok().and_then(|thumbnail| {
                *next_art_id += 1;
                decode_art(&thumbnail, *next_art_id)
                    .inspect_err(|e| debug_log!("unreadable cover art: {e}"))
                    .ok()
            });
            let album = props.AlbumTitle().map(|a| a.to_string()).unwrap_or_default();
            (props.Title()?.to_string(), props.Artist()?.to_string(), album, art.map(Arc::new))
        }
    };

    // A different title than the one already on screen means a new track, even if the app has
    // not finished publishing its timeline yet.
    let changed_track = last.is_some_and(|last| last.title != title);

    let info = session.GetPlaybackInfo()?;
    let controls = info.Controls()?;
    let playing = info.PlaybackStatus()? == Status::Playing;
    let rate = info.PlaybackRate().and_then(|r| r.Value()).unwrap_or(1.0);

    let timeline = session.GetTimelineProperties()?;
    let start = timeline.StartTime()?.Duration as f64 / 1e7;
    let duration = (timeline.EndTime()?.Duration as f64 / 1e7 - start).max(0.0);
    let mut position = timeline.Position()?.Duration as f64 / 1e7 - start;
    // A title change arriving with the previous track's timeline: the app has updated its
    // metadata but not yet its position, so `position` still belongs to the old song. Without
    // this the bar and the elapsed time kept the old song's time for a moment.
    if stale_position(changed_track, position, duration) {
        position = 0.0;
    }
    if playing {
        // The position was reported at some past moment; advance it to now.
        let updated = timeline.LastUpdatedTime()?.UniversalTime;
        let now = filetime_now();
        if updated > 0 && now > updated {
            position += (now - updated) as f64 / 1e7 * rate;
        }
    }
    let position = if duration > 0.0 { position.clamp(0.0, duration) } else { 0.0 };

    Ok(Snapshot {
        title,
        artist,
        album,
        playing,
        position,
        start,
        duration,
        rate,
        at: Instant::now(),
        can_previous: controls.IsPreviousEnabled()?,
        can_next: controls.IsNextEnabled()?,
        can_seek: controls.IsPlaybackPositionEnabled().unwrap_or(false),
        art,
    })
}

fn decode_art(reference: &IRandomAccessStreamReference, id: u64) -> Result<Art> {
    let pixels = decode_square(reference, ART_PX)?;
    Ok(Art { id, accent: accent(&pixels), pixels })
}

/// Crop the center of the image to a square and scale it to `size` pixels. The result is
/// premultiplied BGRA, ready for Direct2D. Used for album art and app logos.
pub fn decode_square(reference: &IRandomAccessStreamReference, size: u32) -> Result<Vec<u8>> {
    let stream = reference.OpenReadAsync()?.join()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let (width, height) = (decoder.PixelWidth()?.max(1), decoder.PixelHeight()?.max(1));
    let short = width.min(height);
    let scaled = (
        (width as u64 * size as u64 / short as u64) as u32,
        (height as u64 * size as u64 / short as u64) as u32,
    );
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(scaled.0)?;
    transform.SetScaledHeight(scaled.1)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    transform.SetBounds(BitmapBounds {
        X: (scaled.0 - size) / 2,
        Y: (scaled.1 - size) / 2,
        Width: size,
        Height: size,
    })?;
    let data = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?;
    let pixels = data.DetachPixelData()?.to_vec();
    if pixels.len() != (size * size * 4) as usize {
        return Err(E_FAIL.into());
    }
    Ok(pixels)
}

/// Dominant color that is colorful enough; gray/black/white are ignored.
fn accent(pixels: &[u8]) -> Option<u32> {
    let (mut r, mut g, mut b, mut weight, mut samples) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0u32);
    let (pixels, _) = pixels.as_chunks::<4>();
    for px in pixels.iter().step_by(3) {
        samples += 1;
        let (pb, pg, pr) = (px[0] as f32, px[1] as f32, px[2] as f32);
        let max = pr.max(pg).max(pb);
        if max < 40.0 {
            continue;
        }
        let saturation = (max - pr.min(pg).min(pb)) / max;
        let w = saturation * saturation * (max / 255.0);
        r += pr * w;
        g += pg * w;
        b += pb * w;
        weight += w;
    }
    if samples == 0 || weight / (samples as f32) < 0.03 {
        return None;
    }
    let (r, g, b) = (r / weight, g / weight, b / weight);
    // Brighten so it stays visible on the black shape.
    let k = 235.0 / r.max(g).max(b).max(1.0);
    let channel = |c: f32| (c * k).min(255.0) as u32;
    Some((channel(r) << 16) | (channel(g) << 8) | channel(b))
}

fn filetime_now() -> i64 {
    let ft = unsafe { GetSystemTimeAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

/// Readable app name from an AppUserModelID or exe name, e.g. "Spotify.exe" → "Spotify".
pub fn app_name(aumid: &str) -> String {
    const KNOWN: [(&str, &str); 11] = [
        ("spotify", "Spotify"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("brave", "Brave"),
        ("zunemusic", "Media Player"),
        ("zunevideo", "Movies & TV"),
        ("applemusic", "Apple Music"),
        ("vlc", "VLC"),
        ("foobar", "foobar2000"),
    ];
    let lower = aumid.to_lowercase();
    if let Some((_, name)) = KNOWN.iter().find(|(key, _)| lower.contains(key)) {
        return name.to_string();
    }
    let base = aumid.rsplit(['\\', '/']).next().unwrap_or(aumid);
    let base = base.split('!').next().unwrap_or(base);
    let base = base.split('_').next().unwrap_or(base).trim_end_matches(".exe");
    base.rsplit('.').next().unwrap_or(base).to_string()
}

/// True when `position` still belongs to the previous track, so it must read as zero.
///
/// Windows can publish the new title a beat before the new timeline, and the position that is
/// still there belongs to the song that just ended. Kept as a pure rule so it can be tested
/// without an audio session.
fn stale_position(changed_track: bool, position: f64, duration: f64) -> bool {
    changed_track && duration > 0.0 && position > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new song starts at zero, even when the app reports the old timeline for a moment.
    #[test]
    fn a_new_track_starts_at_zero() {
        // Same track, mid-song: the position is real.
        assert!(!stale_position(false, 95.0, 200.0));
        // New track with the previous track's position still reported: it is stale.
        assert!(stale_position(true, 95.0, 200.0));
        // A new track that already reports zero is fine either way.
        assert!(!stale_position(true, 0.0, 200.0));
        // A live stream has no duration, so there is nothing to reset.
        assert!(!stale_position(true, 95.0, 0.0));
    }

    #[test]
    fn readable_app_names() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("chrome.exe"), "Chrome");
        assert_eq!(app_name("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "WindowsCamera");
        assert_eq!(app_name("Discord.exe"), "Discord");
        assert_eq!(app_name("C:\\Program Files\\Zoom\\bin\\Zoom.exe"), "Zoom");
    }
}