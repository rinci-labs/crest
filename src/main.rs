//! Crest: a top-of-screen panel for Windows.
//!
//! Layout: `app` owns the window and state, `ui` draws it, `tools` holds page logic, `feeds`
//! reads outside data (media, lyrics, weather, messages, coding sessions), `system` wraps
//! Windows services, and `shell` covers install, identity, tray, settings, and storage.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Diagnostic message, shown only in debug builds (which have a console window).
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            eprintln!($($arg)*);
        }
    };
}

/// A failure that is also recorded in release builds, to the local log (see `log`). Never include
/// message contents, song titles, or the user's file/folder names.
macro_rules! error_log {
    ($($arg:tt)*) => {{
        let message = format!($($arg)*);
        debug_log!("{message}");
        crate::shell::log::write(message);
    }};
}

mod app;
mod feeds;
mod shell;
mod system;
mod tools;
mod ui;

use feeds::messages;
use shell::{identity, install, log};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::core::{Result, w};

fn main() -> Result<()> {
    log::catch_panics();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // Lightweight lifecycle relay installed explicitly from the Agents page. Input is a
        // bounded JSON object on stdin; it only updates Crest's local agent-status record.
        Some("--agent-hook") => {
            // `crest.exe --agent-hook <tool> [event]`: the event may also come from the payload's
            // own `hook_event_name`, so a third-party tool needs no event in its command line.
            if let Some(kind) = args.get(2) {
                let event = args.get(3).map(String::as_str).unwrap_or_default();
                feeds::agents::record_hook(kind, event);
            }
            return Ok(());
        }
        // Message notification check: `crest.exe --check-notifications [file] [seconds]` (see README).
        Some("--check-notifications") => {
            let out = args.get(2).map_or_else(|| std::env::temp_dir().join("crest-notification-check.txt"), Into::into);
            let listen = std::time::Duration::from_secs(args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0));
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if let Err(e) = messages::diagnose(&out, listen) {
                debug_log!("could not write the check result: {e}");
            }
            return Ok(());
        }
        // Administrator step for incoming messages (see `identity`); takes no outside input.
        Some("--prepare-identity") => {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let result = identity::prepare();
            if let Err(e) = &result {
                error_log!("failed to prepare the identity package: {e}");
            }
            std::process::exit(i32::from(result.is_err()));
        }
        Some("--remove-certificates") => std::process::exit(i32::from(identity::remove_certificates().is_err())),
        Some("--register-identity") => {
            register_identity();
            return Ok(());
        }
        // Install without asking.
        Some("--install") => std::process::exit(i32::from(!install::install_quietly())),
        // From Settings > Apps.
        Some("--uninstall") => {
            install::uninstall(args.get(2).map(String::as_str) == Some("--quiet"));
            return Ok(());
        }
        _ => {}
    }
    // Offers to install when run from outside the install folder (e.g. Downloads).
    if install::offer() {
        return Ok(());
    }
    // Leftover temporary copy from the message registration step.
    let _ = std::fs::remove_file(std::env::temp_dir().join(identity::REGISTER_COPY));
    unsafe {
        // Must happen before any window is created, so the panel stays sharp at every scale.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Only one copy may run. The handle is intentionally never closed so the mutex
        // lives until the process exits.
        let _mutex = CreateMutexW(None, true, w!("Local\\Crest.SingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
    }
    app::run()
}

/// Runs from a temporary copy of Crest (without package identity): waits for the running copy
/// to exit, registers the identity package, then starts Crest again from the install folder.
fn register_identity() {
    let Some(dir) = install::dir() else { return };
    install::stop_running();
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if let Err(e) = identity::register(&dir) {
        error_log!("failed to register the identity package: {e}");
        let text = format!("Couldn't turn on incoming messages:\n{e}\n\nEverything else in Crest still works as usual.");
        install::ask(&text, windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR);
    }
    install::launch(&dir.join("crest.exe"), None);
}
