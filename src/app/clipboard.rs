//! Clipboard history: Windows tells the window about every copy, the text goes into
//! [`Clips`](crate::tools::clipboard::Clips), and a click on a row puts it back on the clipboard.

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::Sleep;
use windows::core::{Error, w};

use super::{App, unicode_text};
use crate::tools::Page;

/// Another app may still hold the clipboard just after it changed; tries before giving up.
const OPEN_TRIES: u32 = 5;
const OPEN_RETRY_MS: u32 = 10;

impl App {
    /// Start or stop hearing about copies (`WM_CLIPBOARDUPDATE`). Off while the page is hidden,
    /// so a hidden page records nothing.
    pub(super) fn watch_clipboard(&mut self) {
        let on = self.settings.pages.shown(Page::Clipboard);
        if on == self.clipboard_watched {
            return;
        }
        let done = unsafe {
            if on { AddClipboardFormatListener(self.hwnd) } else { RemoveClipboardFormatListener(self.hwnd) }
        };
        match done {
            Ok(()) => self.clipboard_watched = on,
            Err(e) => error_log!("clipboard listener: {e}"),
        }
        if !on {
            self.clips.clear();
        }
    }

    /// Something was copied: keep it, unless the app that copied it asked to be left out.
    pub(super) fn on_clipboard_update(&mut self) {
        if let Some(text) = copied_text(self.hwnd) {
            self.clips.push(text);
            if self.page == Page::Clipboard {
                self.refresh();
            }
        }
    }

    /// Put a kept text back on the clipboard. Windows reports the copy as usual, which moves
    /// the row to the top.
    pub(super) fn copy_clip(&mut self, id: u64) {
        let Some(text) = self.clips.get(id).map(|clip| clip.text.clone()) else { return };
        if let Err(e) = set_clipboard_text(self.hwnd, &text) {
            error_log!("clipboard copy: {e}");
        }
    }
}

fn open(hwnd: HWND) -> windows::core::Result<()> {
    let mut tries = 0;
    loop {
        match unsafe { OpenClipboard(Some(hwnd)) } {
            Err(_) if tries + 1 < OPEN_TRIES => {
                tries += 1;
                unsafe { Sleep(OPEN_RETRY_MS) };
            }
            done => return done,
        }
    }
}

/// The copied text, or `None` when there is none or the copying app (a password manager, for
/// one) marked it as not to be kept in clipboard history or seen by clipboard monitors.
fn copied_text(hwnd: HWND) -> Option<String> {
    open(hwnd).ok()?;
    let text = unsafe {
        let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        let excluded = IsClipboardFormatAvailable(exclude).is_ok() || flag(history) == Some(0);
        if excluded { None } else { unicode_text() }
    };
    let _ = unsafe { CloseClipboard() };
    text
}

/// The `DWORD` stored in a clipboard format, if present. Clipboard must be open.
unsafe fn flag(format: u32) -> Option<u32> {
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let memory = HGLOBAL(handle.0);
        let ptr = GlobalLock(memory) as *const u32;
        let value = (!ptr.is_null()).then(|| ptr.read_unaligned());
        let _ = GlobalUnlock(memory);
        value
    }
}

fn set_clipboard_text(hwnd: HWND, text: &str) -> windows::core::Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    open(hwnd)?;
    // SAFETY: the memory is filled while locked; once `SetClipboardData` succeeds the
    // clipboard owns it, so it is freed here only when that call fails.
    let result = unsafe {
        EmptyClipboard().and_then(|()| {
            let memory = GlobalAlloc(GMEM_MOVEABLE, wide.len() * size_of::<u16>())?;
            let ptr = GlobalLock(memory) as *mut u16;
            if ptr.is_null() {
                let error = Error::from_thread();
                let _ = GlobalFree(Some(memory));
                return Err(error);
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(memory);
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))).map(|_| ()).inspect_err(|_| {
                let _ = GlobalFree(Some(memory));
            })
        })
    };
    let _ = unsafe { CloseClipboard() };
    result
}
