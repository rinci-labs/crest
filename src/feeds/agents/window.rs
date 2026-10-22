//! Brings a coding agent's window to the front when its row or notice is clicked.
//!
//! An agent runs inside a terminal or an editor, so its own process rarely owns a window. The
//! parent chain is walked a few steps until a process with a visible top-level window turns up,
//! such as Windows Terminal, VS Code, or Cursor. Only process ids and window handles are read.

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SW_RESTORE, SetForegroundWindow, ShowWindow,
};
use windows::core::BOOL;

/// Parent steps tried from the agent process up to the process that owns its window.
const MAX_DEPTH: usize = 6;

/// Bring forward the window hosting process `pid`. False when there is none (pid unknown or
/// gone, or a host without a window, such as a service).
pub fn focus(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let parents = parents();
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        if let Some(window) = window_of(current) {
            return bring_forward(window);
        }
        match parents.get(&current) {
            Some(&parent) if parent != 0 && parent != current => current = parent,
            _ => break,
        }
    }
    false
}

/// Parent of every running process, from one snapshot.
fn parents() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return map };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    map
}

/// First visible, titled, unowned top-level window of `pid`.
fn window_of(pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        found: Option<HWND>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut owner_pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner_pid)) };
        if owner_pid != search.pid {
            return true.into();
        }
        let unowned = unsafe { GetWindow(hwnd, GW_OWNER) }.is_err();
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        if unowned && visible && unsafe { GetWindowTextLengthW(hwnd) } > 0 {
            search.found = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let mut search = Search { pid, found: None };
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.found
}

/// Restore a minimized window and bring it to the front. Allowed because the click that asked
/// for it was the last input event, received by Crest.
fn bring_forward(window: HWND) -> bool {
    unsafe {
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        SetForegroundWindow(window).as_bool()
    }
}
