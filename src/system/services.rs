//! Background thread for WinRT/COM-based Windows services: media, volume, Bluetooth,
//! privacy, messages, network, and system stats.
//!
//! Callbacks from Windows arrive on arbitrary threads. They are all forwarded as plain data to
//! this one thread so they are handled in order, and the results go to the Crest window over
//! a channel plus a `PostMessage` to wake it.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::Result;

use crate::{system::audio, system::bluetooth, system::brightness, system::watch, feeds::lyrics, feeds::media, feeds::messages, system::network, system::privacy, system::stats, system::thumbnail, feeds::weather};

/// Sent to the Crest window when a new event is in the channel.
pub const WM_SERVICE: u32 = WM_APP + 3;

/// Apps often send several media changes in a row; batch them briefly first.
const MEDIA_SETTLE: Duration = Duration::from_millis(60);
/// System sampling interval while the System page is open.
const SYSTEM_EVERY: Duration = Duration::from_secs(1);
/// One app starting to record usually triggers several session events in a row.
const PRIVACY_SETTLE: Duration = Duration::from_millis(150);
/// While the brightness slider is dragged, brightness changes at most once per this interval.
const BRIGHTNESS_EVERY: Duration = Duration::from_millis(60);
/// A Wi-Fi connection triggers several events in a row over a few seconds.
const NETWORK_SETTLE: Duration = Duration::from_secs(2);

/// Events from the services for the Crest window.
pub enum Event {
    Media(Option<media::Snapshot>),
    /// `initial` = the device's starting state (for the slider), not a change worth announcing.
    Volume { level: f32, muted: bool, initial: bool },
    /// Default microphone muted (`Some(true)`), or `None` when there is no microphone.
    Mic(Option<bool>),
    Bluetooth(bluetooth::Notice),
    BluetoothBattery { id: String, battery: u8 },
    Thumbnail(thumbnail::Thumbnail),
    Privacy(privacy::Usage),
    Lyrics(lyrics::Lyrics),
    System(stats::Stats),
    /// Result of asking to read notifications (false also means not installed as a package).
    MessagesAccess(bool),
    Message(messages::Message),
    Network(network::State),
    /// Downloads in progress: file count and total bytes; `None` = none left.
    Downloading(Option<(usize, u64)>),
    /// New files finished writing in the Screenshots or Downloads folder.
    Files { kind: watch::Kind, paths: Vec<std::path::PathBuf> },
    /// Weather city search result; `None` if not found or failed.
    City(Option<weather::City>),
    Weather(Option<weather::Weather>),
    /// Coding agent rows changed (see [`crate::feeds::agents::Listener`]).
    Agents(Vec<crate::feeds::agents::Session>),
    /// Google sign-in finished, or fresh mail and calendar data arrived.
    Google(std::result::Result<crate::feeds::google::Outcome, String>),
    /// Plugin output, by plugin name.
    Plugin(String, crate::feeds::plugins::Output),
    /// One calendar feed was fetched (or failed).
    Calendar(crate::tools::feeds::Fetched),
}

/// Work for the service thread.
pub enum Task {
    /// Combination of the [`media::PROPS`], [`media::PLAYBACK`], [`media::TIMELINE`] bits.
    MediaChanged(u8),
    MediaSessionChanged,
    MediaCommand(media::Command),
    AudioDeviceChanged,
    Bluetooth(bluetooth::Change),
    /// Start/stop sampling the system (while the System page is open).
    WatchSystem(bool),
    /// A recording session or camera usage changed.
    PrivacyChanged,
    /// Start/stop listening for message notifications.
    WatchMessages(bool),
    /// Start/stop watching Wi-Fi; the initial state is sent right away.
    WatchNetwork(bool),
    NetworkChanged,
    SetVolume(f32),
    ToggleMute,
    ToggleMic,
    SetBrightness(u8),
    Quit,
}

/// Sends events to the Crest window from any thread.
#[derive(Clone)]
pub struct Notifier {
    hwnd: usize,
    tx: Sender<Event>,
}

impl Notifier {
    pub fn send(&self, event: Event) {
        if self.tx.send(event).is_ok() {
            let hwnd = HWND(self.hwnd as *mut _);
            let _ = unsafe { PostMessageW(Some(hwnd), WM_SERVICE, WPARAM(0), LPARAM(0)) };
        }
    }
}

pub struct Services {
    tasks: Sender<Task>,
    events: Receiver<Event>,
    notify: Notifier,
}

impl Services {
    pub fn start(hwnd: HWND) -> Self {
        let (tasks, task_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let notify = Notifier { hwnd: hwnd.0 as usize, tx: event_tx };
        let (worker_tasks, worker_notify) = (tasks.clone(), notify.clone());
        let spawned = thread::Builder::new()
            .name("services".into())
            .spawn(move || run(worker_tasks, task_rx, worker_notify));
        if let Err(e) = spawned {
            error_log!("failed to spawn services thread: {e}");
        }
        Self { tasks, events, notify }
    }

    pub fn send(&self, task: Task) {
        let _ = self.tasks.send(task);
    }

    /// Build shelf file thumbnails on a separate thread; results arrive as [`Event::Thumbnail`].
    pub fn thumbnails(&self, items: Vec<(u64, std::path::PathBuf)>) {
        thumbnail::spawn(items, self.notify.clone());
    }

    /// Fetch lyrics on a separate thread; the result arrives as [`Event::Lyrics`].
    pub fn lyrics(&self, request: lyrics::Request) {
        lyrics::spawn(request, self.notify.clone());
    }

    /// For work that must start on the window thread but replies over the channel.
    pub fn notifier(&self) -> Notifier {
        self.notify.clone()
    }

    pub fn next_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

impl Drop for Services {
    fn drop(&mut self) {
        self.send(Task::Quit);
    }
}

fn run(tasks: Sender<Task>, rx: Receiver<Task>, notify: Notifier) {
    // All COM/WinRT objects on this thread live in the multithreaded apartment, so callbacks
    // from Windows may arrive on any thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut media = started("media", media::Monitor::new(tasks.clone(), notify.clone()));
    let mut audio = started("volume", audio::Monitor::new(tasks.clone(), notify.clone()));
    let mut bluetooth = started("bluetooth", bluetooth::Monitor::new(tasks.clone(), notify.clone()));
    let mut privacy = started("privacy",privacy::Monitor::new(tasks.clone(), notify.clone()));
    let mut system: Option<stats::Monitor> = None;
    let mut chat: Option<messages::Monitor> = None;
    let mut net: Option<network::Monitor> = None;
    // Created on first use; `Err` if the display does not support it.
    let mut screen: Option<Result<brightness::Brightness>> = None;

    let mut media_due = Some((Instant::now(), media::ALL));
    let mut system_due: Option<Instant> = None;
    let mut privacy_due: Option<Instant> = None;
    let mut network_due: Option<Instant> = None;
    let mut brightness_due: Option<(Instant, u8)> = None;
    // Task counts per kind (media, volume, bluetooth) for a debug log every minute.
    let (mut counts, mut since) = ([0u32; 3], Instant::now());
    loop {
        if since.elapsed() >= Duration::from_secs(60) {
            debug_log!("services/min: media {}, audio devices {}, bluetooth {}", counts[0], counts[1], counts[2]);
            (counts, since) = ([0; 3], Instant::now());
        }
        let due = [media_due.map(|(due, _)| due), system_due, privacy_due, network_due, brightness_due.map(|(due, _)| due)]
            .into_iter()
            .flatten()
            .min();
        let task = match due {
            Some(due) => match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                Ok(task) => Some(task),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(task) => Some(task),
                Err(_) => break,
            },
        };
        match task {
            None => {
                let now = Instant::now();
                if media_due.is_some_and(|(due, _)| due <= now)
                    && let (Some(monitor), Some((_, flags))) = (&mut media, media_due.take())
                {
                    monitor.refresh(flags);
                }
                if system_due.is_some_and(|due| due <= now) {
                    system_due = system.as_mut().map(|monitor| {
                        notify.send(Event::System(monitor.sample()));
                        now + SYSTEM_EVERY
                    });
                }
                if let Some((due, percent)) = brightness_due
                    && due <= now
                {
                    brightness_due = None;
                    let screen = screen.get_or_insert_with(brightness::Brightness::new);
                    if let Err(e) = screen.as_ref().map(|s| s.set(percent)) {
                        error_log!("cannot change brightness: {e}");
                    }
                }
                if network_due.is_some_and(|due| due <= now) {
                    network_due = None;
                    if net.is_some() {
                        notify.send(Event::Network(network::state()));
                    }
                }
                if privacy_due.is_some_and(|due| due <= now) {
                    privacy_due = None;
                    if let Some(monitor) = &mut privacy {
                        monitor.rescan();
                    }
                }
            }
            Some(Task::MediaChanged(flags)) => {
                counts[0] += 1;
                let (due, pending) = media_due.unwrap_or((Instant::now() + MEDIA_SETTLE, 0));
                media_due = Some((due, pending | flags));
            }
            Some(Task::MediaSessionChanged) => {
                if let Some(monitor) = &mut media {
                    monitor.follow_session();
                }
                media_due = Some((Instant::now() + MEDIA_SETTLE, media::ALL));
            }
            Some(Task::MediaCommand(command)) => {
                if let Some(monitor) = &media {
                    monitor.command(command);
                }
            }
            Some(Task::AudioDeviceChanged) => {
                counts[1] += 1;
                if let Some(monitor) = &mut audio {
                    monitor.attach_default();
                }
                // A microphone was plugged in/out: re-watch all input devices.
                privacy_due.get_or_insert(Instant::now() + PRIVACY_SETTLE);
            }
            Some(Task::PrivacyChanged) => {
                privacy_due.get_or_insert(Instant::now() + PRIVACY_SETTLE);
            }
            Some(Task::WatchMessages(on)) => {
                if on && chat.is_none() {
                    chat = started("messages",messages::Monitor::start(notify.clone()));
                } else if !on {
                    chat = None;
                }
            }
            Some(Task::WatchNetwork(on)) => {
                if on && net.is_none() {
                    net = started("network",network::Monitor::new(tasks.clone()));
                    network_due = Some(Instant::now());
                } else if !on {
                    net = None;
                    network_due = None;
                }
            }
            Some(Task::SetVolume(level)) => {
                if let Some(monitor) = &audio {
                    monitor.set_volume(level);
                }
            }
            Some(Task::ToggleMute) => {
                if let Some(monitor) = &audio {
                    monitor.toggle_mute();
                }
            }
            Some(Task::ToggleMic) => {
                if let Some(monitor) = &audio {
                    monitor.toggle_mic();
                }
            }
            Some(Task::SetBrightness(percent)) => {
                // Use the latest value; the deadline is not pushed back so the slider stays responsive.
                let due = brightness_due.map_or(Instant::now() + BRIGHTNESS_EVERY, |(due, _)| due);
                brightness_due = Some((due, percent));
            }
            Some(Task::NetworkChanged) => {
                // Wait for the burst to settle (each new event pushes the deadline back).
                network_due = Some(Instant::now() + NETWORK_SETTLE);
            }
            Some(Task::Bluetooth(change)) => {
                counts[2] += 1;
                if let Some(monitor) = &mut bluetooth {
                    monitor.apply(change);
                }
            }
            Some(Task::WatchSystem(on)) => {
                if on && system.is_none() {
                    system = stats::Monitor::new();
                    // Percentages only mean something after an interval since the first sample.
                    system_due = system.as_ref().map(|_| Instant::now() + SYSTEM_EVERY);
                } else if !on {
                    system = None;
                    system_due = None;
                }
            }
            Some(Task::Quit) => break,
        }
    }
}

/// A service that fails to start (e.g. no audio device) is simply skipped.
fn started<T>(name: &str, result: Result<T>) -> Option<T> {
    match result {
        Ok(monitor) => Some(monitor),
        Err(e) => {
            error_log!("{name} service inactive: {e}");
            None
        }
    }
}
