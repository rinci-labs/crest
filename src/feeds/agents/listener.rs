//! Background thread that rescans agents when their folders change.
//!
//! Folder change notifications wake it right away; a slow fallback tick catches what files
//! cannot show (a process that exited, a "Done" that turns "Idle"). Bursts of writes, such as a
//! transcript streaming, are coalesced so a busy agent costs at most a few scans per second.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FindCloseChangeNotification,
    FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
use windows::core::HSTRING;

use super::{Scanner, watched};
use crate::system::services::{Event, Notifier};

/// Fallback rescan when no folder changed.
const TICK: Duration = Duration::from_secs(5);
/// Shortest gap between two scans while files keep changing.
const MIN_GAP: Duration = Duration::from_millis(400);

pub struct Listener {
    /// Wakes the thread: to rescan now, or to stop when `stop` is set.
    wake: HANDLE,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Listener {
    /// Start watching. The first scan is sent right away as [`Event::Agents`].
    pub fn start(notify: Notifier) -> Option<Self> {
        let wake = unsafe { CreateEventW(None, false, false, None) }.ok()?;
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_wake, thread_stop) = (wake.0 as usize, stop.clone());
        let thread = thread::Builder::new()
            .name("agents".into())
            .spawn(move || run(HANDLE(thread_wake as *mut _), &thread_stop, &notify))
            .map_err(|e| error_log!("failed to spawn agents thread: {e}"))
            .ok()?;
        Some(Self { wake, stop, thread: Some(thread) })
    }

    /// Rescan now instead of waiting for a change or the next tick.
    pub fn refresh(&self) {
        let _ = unsafe { SetEvent(self.wake) };
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.refresh();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = unsafe { CloseHandle(self.wake) };
    }
}

/// Change notification handles for the folders that exist right now.
struct Watches {
    folders: Vec<PathBuf>,
    handles: Vec<HANDLE>,
}

impl Watches {
    fn open() -> Self {
        let mut watches = Watches { folders: Vec::new(), handles: Vec::new() };
        for (folder, subtree) in watched() {
            let filter = FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
            if let Ok(handle) = unsafe { FindFirstChangeNotificationW(&HSTRING::from(folder.as_os_str()), subtree, filter) } {
                watches.folders.push(folder);
                watches.handles.push(handle);
            }
        }
        watches
    }

    /// An agent installed or first run since the handles were opened.
    fn stale(&self) -> bool {
        watched().len() != self.folders.len()
    }
}

impl Drop for Watches {
    fn drop(&mut self) {
        for handle in &self.handles {
            let _ = unsafe { FindCloseChangeNotification(*handle) };
        }
    }
}

fn run(wake: HANDLE, stop: &AtomicBool, notify: &Notifier) {
    // Session records are parsed with WinRT JSON.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut scanner = Scanner::default();
    let mut watches = Watches::open();
    let mut last_scan = Instant::now() - MIN_GAP;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let gap = last_scan.elapsed();
        if gap < MIN_GAP {
            thread::sleep(MIN_GAP - gap);
        }
        if let Some(rows) = scanner.scan() {
            notify.send(Event::Agents(rows));
        }
        last_scan = Instant::now();
        if watches.stale() {
            watches = Watches::open();
        }

        let mut handles = watches.handles.clone();
        handles.push(wake);
        let result = unsafe { WaitForMultipleObjects(&handles, false, TICK.as_millis() as u32) };
        if result == WAIT_TIMEOUT {
            continue;
        }
        let index = result.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if let Some(handle) = watches.handles.get(index)
            && unsafe { FindNextChangeNotification(*handle) }.is_err()
        {
            // The folder went away; reopen everything on the next round.
            watches = Watches::open();
        }
    }
}
