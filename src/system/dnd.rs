//! Drag files onto Crest (into the shelf), drag from the shelf to other apps, and open or
//! reveal files in Explorer.

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HWND, LPARAM, POINTL, S_OK, WPARAM};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IBindCtx, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE,
    IDropSource, IDropTarget, IDropTarget_Impl, OleInitialize, OleUninitialize, RegisterDragDrop,
    ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{
    BHID_DataObject, DragQueryFileW, HDROP, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop,
    ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, PostMessageW, SW_SHOWNORMAL, WM_APP};
use windows::core::{HSTRING, PCWSTR, Ref, Result, implement, w};

/// Sent to the Crest window; `wparam` is one of `DRAG_*`.
pub const WM_DRAG: u32 = WM_APP + 5;
pub const DRAG_LEAVE: usize = 0;
pub const DRAG_ENTER: usize = 1;
pub const DRAG_DROP: usize = 2;
/// Files are being dragged over Crest; `lparam` = cursor screen position (like mouse messages).
pub const DRAG_OVER: usize = 3;

const HDROP_FORMAT: FORMATETC = FORMATETC {
    cfFormat: CF_HDROP.0,
    ptd: std::ptr::null_mut(),
    dwAspect: DVASPECT_CONTENT.0,
    lindex: -1,
    tymed: TYMED_HGLOBAL.0 as u32,
};

thread_local! {
    /// Files from the last drop, taken by the Crest window when it receives `WM_DRAG`.
    static DROPPED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

pub fn take_dropped() -> Vec<PathBuf> {
    DROPPED.with(|dropped| std::mem::take(&mut *dropped.borrow_mut()))
}

/// While alive, the Crest window accepts files dragged onto it.
pub struct DropRegistration {
    hwnd: HWND,
}

impl DropRegistration {
    pub fn register(hwnd: HWND) -> Result<Self> {
        unsafe {
            OleInitialize(None)?;
            let target: IDropTarget = Target { hwnd, last: Cell::new((i32::MIN, i32::MIN)) }.into();
            if let Err(e) = RegisterDragDrop(hwnd, &target) {
                OleUninitialize();
                return Err(e);
            }
        }
        Ok(Self { hwnd })
    }
}

impl Drop for DropRegistration {
    fn drop(&mut self) {
        unsafe {
            let _ = RevokeDragDrop(self.hwnd);
            OleUninitialize();
        }
    }
}

#[implement(IDropTarget)]
struct Target {
    hwnd: HWND,
    /// Last reported position; Windows keeps calling `DragOver` even when the cursor is still.
    last: Cell<(i32, i32)>,
}

impl Target {
    fn post(&self, kind: usize) {
        let _ = unsafe { PostMessageW(Some(self.hwnd), WM_DRAG, WPARAM(kind), LPARAM(0)) };
    }

    fn post_position(&self, pt: &POINTL) {
        if self.last.replace((pt.x, pt.y)) == (pt.x, pt.y) {
            return;
        }
        let packed = (((pt.y as u32) & 0xFFFF) << 16) | ((pt.x as u32) & 0xFFFF);
        let _ = unsafe { PostMessageW(Some(self.hwnd), WM_DRAG, WPARAM(DRAG_OVER), LPARAM(packed as isize)) };
    }
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let has_files = data.as_ref().is_some_and(|d| unsafe { d.QueryGetData(&HDROP_FORMAT) } == S_OK);
        set_effect(effect, if has_files { DROPEFFECT_COPY } else { DROPEFFECT_NONE });
        if has_files {
            self.post(DRAG_ENTER);
            self.last.set((i32::MIN, i32::MIN));
            self.post_position(pt);
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        // The shelf only stores file locations, so to the source app this is a copy.
        set_effect(effect, DROPEFFECT_COPY);
        self.post_position(pt);
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.post(DRAG_LEAVE);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let paths = data.as_ref().map(files).unwrap_or_default();
        set_effect(effect, if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY });
        DROPPED.with(|dropped| dropped.borrow_mut().extend(paths));
        self.post(DRAG_DROP);
        Ok(())
    }
}

fn set_effect(effect: *mut DROPEFFECT, value: DROPEFFECT) {
    if let Some(effect) = unsafe { effect.as_mut() } {
        *effect = value;
    }
}

fn files(data: &IDataObject) -> Vec<PathBuf> {
    unsafe {
        let Ok(mut medium) = data.GetData(&HDROP_FORMAT) else { return Vec::new() };
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        let paths = (0..count)
            .map(|i| {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(hdrop, i, Some(&mut buf));
                PathBuf::from(OsString::from_wide(&buf[..len]))
            })
            .collect();
        ReleaseStgMedium(&mut medium);
        paths
    }
}

/// Drag one file from the shelf to another app. Runs its own message loop until the file is
/// dropped, so do not call it while holding app state.
pub fn drag_out(hwnd: HWND, path: &Path) -> Option<DROPEFFECT> {
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None::<&IBindCtx>).ok()?;
        let data: IDataObject = item.BindToHandler(None::<&IBindCtx>, &BHID_DataObject).ok()?;
        let allowed = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;
        SHDoDragDrop(Some(hwnd), &data, None::<&IDropSource>, allowed).ok()
    }
}

/// Open a file or folder with its default app.
pub fn open(hwnd: HWND, path: &Path) {
    unsafe {
        ShellExecuteW(Some(hwnd), w!("open"), &HSTRING::from(path.as_os_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Open an app by its AppUserModelID, e.g. WhatsApp when its message alert is clicked.
pub fn open_app(hwnd: HWND, aumid: &str) {
    if aumid.is_empty() {
        return;
    }
    let target = HSTRING::from(format!("shell:AppsFolder\\{aumid}"));
    unsafe {
        // Let the opened app come to the front, since Crest itself never takes activation.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        ShellExecuteW(Some(hwnd), w!("open"), w!("explorer.exe"), &target, PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Open a web link in the default browser.
pub fn open_url(url: &str) {
    // Only web links are opened, so a plugin row or a settings value cannot start a program.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Show the file in File Explorer with it selected.
pub fn reveal(hwnd: HWND, path: &Path) {
    let args = HSTRING::from(format!("/select,\"{}\"", path.display()));
    unsafe {
        ShellExecuteW(Some(hwnd), w!("open"), w!("explorer.exe"), &args, PCWSTR::null(), SW_SHOWNORMAL);
    }
}
