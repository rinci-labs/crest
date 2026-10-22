//! Watches the foreground window so Crest knows when to step aside: hide while an app is
//! full-screen, and shrink to a thin line while a window is maximized.
//!
//! The event sources are chosen to fire rarely: foreground change, minimize, end of
//! move/resize, and full-screen notifications from Explorer (AppBar). Window-position hooks
//! are deliberately avoided because busy apps (browsers, Electron) can send hundreds of
//! events per second. Maximizing via the button has no event of its own, so the Crest window
//! does a cheap check once per second.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTONULL, MonitorFromWindow};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Shell::{
    ABM_NEW, ABM_REMOVE, APPBARDATA, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN, SHAppBarMessage, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
    EVENT_SYSTEM_MOVESIZEEND, GetClassNameW, GetForegroundWindow, GetWindowRect, IsIconic,
    IsWindowVisible, IsZoomed, PostMessageW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    WM_APP,
};

/// Sent to the Crest window when the foreground window changes, is minimized, or finishes moving.
pub const WM_DESKTOP_CHANGED: u32 = WM_APP + 2;
/// Sent by Explorer when an app enters/leaves full-screen.
pub const WM_APPBAR: u32 = WM_APP + 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    /// The foreground window is maximized on Crest's monitor: the shape shrinks so it does not cover tabs.
    Maximized,
    /// An app is full-screen (game, video, presentation): Crest is hidden.
    Fullscreen,
}

/// Window that receives notifications from the hook callback.
static TARGET: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// How many times the hook callback ran; to monitor how often the process is woken up.
static CALLBACKS: AtomicU32 = AtomicU32::new(0);

/// Number of callbacks since the previous call.
pub fn take_callback_count() -> u32 {
    CALLBACKS.swap(0, Ordering::Relaxed)
}

pub struct Watcher {
    hooks: Vec<HWINEVENTHOOK>,
    appbar: APPBARDATA,
}

impl Watcher {
    pub fn new(notify: HWND) -> Self {
        TARGET.store(notify.0, Ordering::Relaxed);
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let hooks = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZEEND),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        ]
        .into_iter()
        .map(|(min, max)| unsafe { SetWinEventHook(min, max, None, Some(on_event), 0, 0, flags) })
        .filter(|h| !h.is_invalid())
        .collect();

        // Register as an AppBar without reserving screen space, only to receive full-screen notifications.
        let mut appbar = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: notify,
            uCallbackMessage: WM_APPBAR,
            ..Default::default()
        };
        unsafe { SHAppBarMessage(ABM_NEW, &mut appbar) };
        Self { hooks, appbar }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        unsafe {
            for h in self.hooks.drain(..) {
                let _ = UnhookWinEvent(h);
            }
            SHAppBarMessage(ABM_REMOVE, &mut self.appbar);
        }
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    CALLBACKS.fetch_add(1, Ordering::Relaxed);
    let target = HWND(TARGET.load(Ordering::Relaxed));
    let _ = unsafe { PostMessageW(Some(target), WM_DESKTOP_CHANGED, WPARAM(0), LPARAM(0)) };
}

/// Pick Crest's mode from the foreground window on Crest's monitor.
/// Cheap checks come first because this is also called periodically.
pub fn current_mode(own: HWND, monitor: HMONITOR, area: &RECT) -> Mode {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid()
            || fg == own
            || !IsWindowVisible(fg).as_bool()
            || IsIconic(fg).as_bool()
            || MonitorFromWindow(fg, MONITOR_DEFAULTTONULL) != monitor
            || is_shell(fg)
        {
            return Mode::Normal;
        }
        let zoomed = IsZoomed(fg).as_bool();
        let mut r = RECT::default();
        let covers = GetWindowRect(fg, &mut r).is_ok()
            && r.left <= area.left
            && r.top <= area.top
            && r.right >= area.right
            && r.bottom >= area.bottom;
        if !covers {
            return if zoomed { Mode::Maximized } else { Mode::Normal };
        }
        if !zoomed {
            return Mode::Fullscreen;
        }
        // A maximized window with an auto-hide taskbar also covers the whole screen; the
        // difference is that Windows itself does not treat it as full-screen.
        match SHQueryUserNotificationState() {
            Ok(QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE) => Mode::Fullscreen,
            _ => Mode::Maximized,
        }
    }
}

/// The desktop and taskbar can also be the "foreground window", but they are not apps.
fn is_shell(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) }.clamp(0, 64) as usize;
    let class = String::from_utf16_lossy(&buf[..len]);
    matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}
