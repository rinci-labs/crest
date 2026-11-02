//! The few parts of WebView2 the player uses, bound by hand: the `windows` crate has no WebView2
//! bindings, and no third-party crate is used.
//!
//! Interface ids and method order are copied from Microsoft's `WebView2.h` (SDK 1.0.4314.35).
//! A COM vtable is positional, so every interface lists its methods in the header's order up to
//! the last one Crest calls; the ones after it are left out.
//!
//! WebView2's loader (`WebView2Loader.dll`) is not part of Windows. Crest does what the loader
//! does: it finds the Evergreen runtime through EdgeUpdate's registry entry and calls the
//! runtime's own `CreateWebViewEnvironmentWithOptionsInternal`. That export is not documented,
//! so a runtime without it simply leaves the player unavailable (the link then opens in the
//! browser).

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{E_FAIL, E_NOINTERFACE, HWND, RECT};
use windows::Win32::System::Com::CoTaskMemAlloc;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY, RegGetValueW};
use windows::core::{BOOL, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, Interface, PCWSTR, PWSTR, Result, implement, interface, s, w};

/// EdgeUpdate's entry for the stable WebView2 runtime; its `EBWebView` value is the runtime's
/// version folder.
const CLIENT_STATE: PCWSTR = w!("SOFTWARE\\Microsoft\\EdgeUpdate\\ClientState\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}");
/// The runtime's client library inside that folder.
#[cfg(target_arch = "aarch64")]
const CLIENT_DLL: &str = "EBWebView\\arm64\\EmbeddedBrowserWebView.dll";
#[cfg(not(target_arch = "aarch64"))]
const CLIENT_DLL: &str = "EBWebView\\x64\\EmbeddedBrowserWebView.dll";
/// The oldest runtime with every method used here (the first stable WebView2 release).
const TARGET_VERSION: &str = "86.0.616.0";
/// Chromium switches for the player's browser: start videos with sound without a click, as
/// OBS's browser source does.
const BROWSER_ARGUMENTS: &str = "--autoplay-policy=no-user-gesture-required";

/// `EventRegistrationToken`.
pub type Token = i64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Color {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[interface("b96d755e-0319-4e92-a296-23436f46a1fc")]
pub unsafe trait ICoreWebView2Environment: IUnknown {
    pub fn CreateCoreWebView2Controller(&self, parent: HWND, handler: *mut c_void) -> HRESULT;
}

#[interface("4d00c0d1-9434-4eb6-8078-8697a560334f")]
pub unsafe trait ICoreWebView2Controller: IUnknown {
    pub fn get_IsVisible(&self, visible: *mut BOOL) -> HRESULT;
    pub fn put_IsVisible(&self, visible: BOOL) -> HRESULT;
    pub fn get_Bounds(&self, bounds: *mut RECT) -> HRESULT;
    pub fn put_Bounds(&self, bounds: RECT) -> HRESULT;
    pub fn get_ZoomFactor(&self, zoom: *mut f64) -> HRESULT;
    pub fn put_ZoomFactor(&self, zoom: f64) -> HRESULT;
    pub fn add_ZoomFactorChanged(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_ZoomFactorChanged(&self, token: Token) -> HRESULT;
    pub fn SetBoundsAndZoomFactor(&self, bounds: RECT, zoom: f64) -> HRESULT;
    pub fn MoveFocus(&self, reason: i32) -> HRESULT;
    pub fn add_MoveFocusRequested(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_MoveFocusRequested(&self, token: Token) -> HRESULT;
    pub fn add_GotFocus(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_GotFocus(&self, token: Token) -> HRESULT;
    pub fn add_LostFocus(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_LostFocus(&self, token: Token) -> HRESULT;
    pub fn add_AcceleratorKeyPressed(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_AcceleratorKeyPressed(&self, token: Token) -> HRESULT;
    pub fn get_ParentWindow(&self, parent: *mut HWND) -> HRESULT;
    pub fn put_ParentWindow(&self, parent: HWND) -> HRESULT;
    pub fn NotifyParentWindowPositionChanged(&self) -> HRESULT;
    pub fn Close(&self) -> HRESULT;
    pub fn get_CoreWebView2(&self, webview: *mut *mut c_void) -> HRESULT;
}

#[interface("c979903e-d4ca-4228-92eb-47ee3fa96eab")]
pub unsafe trait ICoreWebView2Controller2: ICoreWebView2Controller {
    pub fn get_DefaultBackgroundColor(&self, color: *mut Color) -> HRESULT;
    pub fn put_DefaultBackgroundColor(&self, color: Color) -> HRESULT;
}

#[interface("76eceacb-0462-4d94-ac83-423a6793775e")]
pub unsafe trait ICoreWebView2: IUnknown {
    pub fn get_Settings(&self, settings: *mut *mut c_void) -> HRESULT;
    pub fn get_Source(&self, uri: *mut PWSTR) -> HRESULT;
    pub fn Navigate(&self, uri: PCWSTR) -> HRESULT;
    pub fn NavigateToString(&self, html: PCWSTR) -> HRESULT;
    pub fn add_NavigationStarting(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_NavigationStarting(&self, token: Token) -> HRESULT;
    pub fn add_ContentLoading(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_ContentLoading(&self, token: Token) -> HRESULT;
    pub fn add_SourceChanged(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_SourceChanged(&self, token: Token) -> HRESULT;
    pub fn add_HistoryChanged(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_HistoryChanged(&self, token: Token) -> HRESULT;
    pub fn add_NavigationCompleted(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_NavigationCompleted(&self, token: Token) -> HRESULT;
    pub fn add_FrameNavigationStarting(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_FrameNavigationStarting(&self, token: Token) -> HRESULT;
    pub fn add_FrameNavigationCompleted(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_FrameNavigationCompleted(&self, token: Token) -> HRESULT;
    pub fn add_ScriptDialogOpening(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_ScriptDialogOpening(&self, token: Token) -> HRESULT;
    pub fn add_PermissionRequested(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_PermissionRequested(&self, token: Token) -> HRESULT;
    pub fn add_ProcessFailed(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_ProcessFailed(&self, token: Token) -> HRESULT;
    pub fn AddScriptToExecuteOnDocumentCreated(&self, script: PCWSTR, handler: *mut c_void) -> HRESULT;
    pub fn RemoveScriptToExecuteOnDocumentCreated(&self, id: PCWSTR) -> HRESULT;
    pub fn ExecuteScript(&self, script: PCWSTR, handler: *mut c_void) -> HRESULT;
    pub fn CapturePreview(&self, format: i32, stream: *mut c_void, handler: *mut c_void) -> HRESULT;
    pub fn Reload(&self) -> HRESULT;
    pub fn PostWebMessageAsJson(&self, json: PCWSTR) -> HRESULT;
    pub fn PostWebMessageAsString(&self, text: PCWSTR) -> HRESULT;
    pub fn add_WebMessageReceived(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_WebMessageReceived(&self, token: Token) -> HRESULT;
    pub fn CallDevToolsProtocolMethod(&self, method: PCWSTR, json: PCWSTR, handler: *mut c_void) -> HRESULT;
    pub fn get_BrowserProcessId(&self, pid: *mut u32) -> HRESULT;
    pub fn get_CanGoBack(&self, value: *mut BOOL) -> HRESULT;
    pub fn get_CanGoForward(&self, value: *mut BOOL) -> HRESULT;
    pub fn GoBack(&self) -> HRESULT;
    pub fn GoForward(&self) -> HRESULT;
    pub fn GetDevToolsProtocolEventReceiver(&self, event: PCWSTR, receiver: *mut *mut c_void) -> HRESULT;
    pub fn Stop(&self) -> HRESULT;
    pub fn add_NewWindowRequested(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_NewWindowRequested(&self, token: Token) -> HRESULT;
    pub fn add_DocumentTitleChanged(&self, handler: *mut c_void, token: *mut Token) -> HRESULT;
    pub fn remove_DocumentTitleChanged(&self, token: Token) -> HRESULT;
    pub fn get_DocumentTitle(&self, title: *mut PWSTR) -> HRESULT;
}

#[interface("e562e4f0-d7fa-43ac-8d71-c05150499f00")]
pub unsafe trait ICoreWebView2Settings: IUnknown {
    pub fn get_IsScriptEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_IsScriptEnabled(&self, value: BOOL) -> HRESULT;
    pub fn get_IsWebMessageEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_IsWebMessageEnabled(&self, value: BOOL) -> HRESULT;
    pub fn get_AreDefaultScriptDialogsEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_AreDefaultScriptDialogsEnabled(&self, value: BOOL) -> HRESULT;
    pub fn get_IsStatusBarEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_IsStatusBarEnabled(&self, value: BOOL) -> HRESULT;
    pub fn get_AreDevToolsEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_AreDevToolsEnabled(&self, value: BOOL) -> HRESULT;
    pub fn get_AreDefaultContextMenusEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_AreDefaultContextMenusEnabled(&self, value: BOOL) -> HRESULT;
    pub fn get_AreHostObjectsAllowed(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_AreHostObjectsAllowed(&self, value: BOOL) -> HRESULT;
    pub fn get_IsZoomControlEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_IsZoomControlEnabled(&self, value: BOOL) -> HRESULT;
    pub fn get_IsBuiltInErrorPageEnabled(&self, value: *mut BOOL) -> HRESULT;
    pub fn put_IsBuiltInErrorPageEnabled(&self, value: BOOL) -> HRESULT;
}

#[interface("5b495469-e119-438a-9b18-7604f25f2e49")]
pub unsafe trait ICoreWebView2NavigationStartingEventArgs: IUnknown {
    pub fn get_Uri(&self, uri: *mut PWSTR) -> HRESULT;
    pub fn get_IsUserInitiated(&self, value: *mut BOOL) -> HRESULT;
    pub fn get_IsRedirected(&self, value: *mut BOOL) -> HRESULT;
    pub fn get_RequestHeaders(&self, headers: *mut *mut c_void) -> HRESULT;
    pub fn get_Cancel(&self, cancel: *mut BOOL) -> HRESULT;
    pub fn put_Cancel(&self, cancel: BOOL) -> HRESULT;
}

#[interface("34acb11c-fc37-4418-9132-f9c21d1eafb9")]
pub unsafe trait ICoreWebView2NewWindowRequestedEventArgs: IUnknown {
    pub fn get_Uri(&self, uri: *mut PWSTR) -> HRESULT;
    pub fn put_NewWindow(&self, window: *mut c_void) -> HRESULT;
    pub fn get_NewWindow(&self, window: *mut *mut c_void) -> HRESULT;
    pub fn put_Handled(&self, handled: BOOL) -> HRESULT;
}

#[interface("4e8a3389-c9d8-4bd2-b6b5-124fee6cc14d")]
unsafe trait ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler: IUnknown {
    fn Invoke(&self, error: HRESULT, environment: *mut c_void) -> HRESULT;
}

#[interface("6c4819f3-c9b7-4260-8127-c9f5bde7f68c")]
unsafe trait ICoreWebView2CreateCoreWebView2ControllerCompletedHandler: IUnknown {
    fn Invoke(&self, error: HRESULT, controller: *mut c_void) -> HRESULT;
}

#[interface("9adbe429-f36d-432b-9ddc-f8881fbd76e3")]
unsafe trait ICoreWebView2NavigationStartingEventHandler: IUnknown {
    fn Invoke(&self, sender: *mut c_void, args: *mut c_void) -> HRESULT;
}

#[interface("d4c185fe-c81c-4989-97af-2d3fa7ab5651")]
unsafe trait ICoreWebView2NewWindowRequestedEventHandler: IUnknown {
    fn Invoke(&self, sender: *mut c_void, args: *mut c_void) -> HRESULT;
}

#[interface("2fde08a8-1e9a-4766-8c05-95a9ceb9d1c5")]
unsafe trait ICoreWebView2EnvironmentOptions: IUnknown {
    fn get_AdditionalBrowserArguments(&self, value: *mut PWSTR) -> HRESULT;
    fn put_AdditionalBrowserArguments(&self, value: PCWSTR) -> HRESULT;
    fn get_Language(&self, value: *mut PWSTR) -> HRESULT;
    fn put_Language(&self, value: PCWSTR) -> HRESULT;
    fn get_TargetCompatibleBrowserVersion(&self, value: *mut PWSTR) -> HRESULT;
    fn put_TargetCompatibleBrowserVersion(&self, value: PCWSTR) -> HRESULT;
    fn get_AllowSingleSignOnUsingOSPrimaryAccount(&self, allow: *mut BOOL) -> HRESULT;
    fn put_AllowSingleSignOnUsingOSPrimaryAccount(&self, allow: BOOL) -> HRESULT;
}

/// A one-shot callback, taken out on its first call.
type Once<T> = RefCell<Option<Box<dyn FnOnce(Result<T>)>>>;

#[implement(ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler)]
struct EnvironmentCreated(Once<ICoreWebView2Environment>);

impl ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler_Impl for EnvironmentCreated_Impl {
    unsafe fn Invoke(&self, error: HRESULT, environment: *mut c_void) -> HRESULT {
        // SAFETY: WebView2 passes a valid environment, or null on failure; it is borrowed for the call.
        let environment = unsafe { ICoreWebView2Environment::from_raw_borrowed(&environment) }.cloned();
        if let Some(done) = self.0.borrow_mut().take() {
            done(error.ok().and_then(|()| environment.ok_or_else(|| E_FAIL.into())));
        }
        HRESULT(0)
    }
}

#[implement(ICoreWebView2CreateCoreWebView2ControllerCompletedHandler)]
struct ControllerCreated(Once<ICoreWebView2Controller>);

impl ICoreWebView2CreateCoreWebView2ControllerCompletedHandler_Impl for ControllerCreated_Impl {
    unsafe fn Invoke(&self, error: HRESULT, controller: *mut c_void) -> HRESULT {
        // SAFETY: as above, a controller borrowed for the call, or null on failure.
        let controller = unsafe { ICoreWebView2Controller::from_raw_borrowed(&controller) }.cloned();
        if let Some(done) = self.0.borrow_mut().take() {
            done(error.ok().and_then(|()| controller.ok_or_else(|| E_FAIL.into())));
        }
        HRESULT(0)
    }
}

/// Decides each top-level navigation: `true` lets it go ahead.
#[implement(ICoreWebView2NavigationStartingEventHandler)]
struct NavigationStarting(Box<dyn Fn(&str) -> bool>);

impl ICoreWebView2NavigationStartingEventHandler_Impl for NavigationStarting_Impl {
    unsafe fn Invoke(&self, _sender: *mut c_void, args: *mut c_void) -> HRESULT {
        // SAFETY: the event arguments are valid for the call.
        let Some(args) = (unsafe { ICoreWebView2NavigationStartingEventArgs::from_raw_borrowed(&args) }) else { return E_FAIL };
        let mut uri = PWSTR::null();
        let allowed = unsafe { args.get_Uri(&mut uri) }.is_ok() && (self.0)(&take_string(uri));
        if !allowed {
            let _ = unsafe { args.put_Cancel(true.into()) };
        }
        HRESULT(0)
    }
}

/// Every request for a new window is handled here, so WebView2 never opens one of its own; the
/// callback gets the link that asked for it.
#[implement(ICoreWebView2NewWindowRequestedEventHandler)]
struct NewWindowRequested(Box<dyn Fn(&str)>);

impl ICoreWebView2NewWindowRequestedEventHandler_Impl for NewWindowRequested_Impl {
    unsafe fn Invoke(&self, _sender: *mut c_void, args: *mut c_void) -> HRESULT {
        // SAFETY: the event arguments are valid for the call.
        let Some(args) = (unsafe { ICoreWebView2NewWindowRequestedEventArgs::from_raw_borrowed(&args) }) else { return E_FAIL };
        let _ = unsafe { args.put_Handled(true.into()) };
        let mut uri = PWSTR::null();
        if unsafe { args.get_Uri(&mut uri) }.is_ok() {
            (self.0)(&take_string(uri));
        }
        HRESULT(0)
    }
}

#[implement(ICoreWebView2EnvironmentOptions)]
struct Options;

impl ICoreWebView2EnvironmentOptions_Impl for Options_Impl {
    unsafe fn get_AdditionalBrowserArguments(&self, value: *mut PWSTR) -> HRESULT {
        give_string(BROWSER_ARGUMENTS, value)
    }
    unsafe fn put_AdditionalBrowserArguments(&self, _value: PCWSTR) -> HRESULT {
        HRESULT(0)
    }
    unsafe fn get_Language(&self, value: *mut PWSTR) -> HRESULT {
        give_string("", value)
    }
    unsafe fn put_Language(&self, _value: PCWSTR) -> HRESULT {
        HRESULT(0)
    }
    unsafe fn get_TargetCompatibleBrowserVersion(&self, value: *mut PWSTR) -> HRESULT {
        give_string(TARGET_VERSION, value)
    }
    unsafe fn put_TargetCompatibleBrowserVersion(&self, _value: PCWSTR) -> HRESULT {
        HRESULT(0)
    }
    // Never signed in with the Windows account: the player gets no identity of the user's.
    unsafe fn get_AllowSingleSignOnUsingOSPrimaryAccount(&self, allow: *mut BOOL) -> HRESULT {
        if allow.is_null() {
            return E_FAIL;
        }
        unsafe { *allow = false.into() };
        HRESULT(0)
    }
    unsafe fn put_AllowSingleSignOnUsingOSPrimaryAccount(&self, _allow: BOOL) -> HRESULT {
        HRESULT(0)
    }
}

/// Hand a string to WebView2 in memory it frees itself (`CoTaskMemAlloc`).
fn give_string(text: &str, out: *mut PWSTR) -> HRESULT {
    if out.is_null() {
        return E_FAIL;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the buffer is sized for `wide` and filled before WebView2 sees it.
    unsafe {
        let memory = CoTaskMemAlloc(wide.len() * size_of::<u16>()) as *mut u16;
        if memory.is_null() {
            return E_FAIL;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), memory, wide.len());
        *out = PWSTR(memory);
    }
    HRESULT(0)
}

/// A string WebView2 handed out, as Rust text; its memory is freed.
pub fn take_string(text: PWSTR) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: WebView2 returns a NUL-terminated string allocated with `CoTaskMemAlloc`.
    unsafe {
        let value = text.to_string().unwrap_or_default();
        windows::Win32::System::Com::CoTaskMemFree(Some(text.0 as *const c_void));
        value
    }
}

/// The installed runtime's client library, from EdgeUpdate's entry (machine-wide first).
fn client_dll() -> Option<PathBuf> {
    [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER].into_iter().find_map(|root| {
        let mut buffer = [0u16; 520];
        let mut size = (buffer.len() * 2) as u32;
        // EdgeUpdate is a 32-bit program, so its entries sit in the 32-bit registry view.
        let flags = RRF_RT_REG_SZ | RRF_SUBKEY_WOW6432KEY;
        let read = unsafe { RegGetValueW(root, CLIENT_STATE, w!("EBWebView"), flags, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size)) };
        if read.is_err() {
            return None;
        }
        let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        let folder = String::from_utf16_lossy(&buffer[..len]);
        let dll = Path::new(folder.trim()).join(CLIENT_DLL);
        (!folder.trim().is_empty() && dll.is_file()).then_some(dll)
    })
}

/// Is a WebView2 runtime installed?
pub fn available() -> bool {
    client_dll().is_some()
}

/// Start a WebView2 environment with its data in `data_dir`; `done` runs on this thread once it
/// is ready or has failed. The calling thread must be an STA with a message loop.
pub fn create_environment(data_dir: &Path, done: impl FnOnce(Result<ICoreWebView2Environment>) + 'static) -> Result<()> {
    type Create = unsafe extern "system" fn(bool, i32, PCWSTR, *mut c_void, *mut c_void) -> HRESULT;
    let dll = client_dll().ok_or_else(|| windows::core::Error::from(E_NOINTERFACE))?;
    // SAFETY: the runtime's library stays loaded for the rest of the process, since the
    // environment and its objects live in it.
    let create: Create = unsafe {
        let module = LoadLibraryW(&HSTRING::from(dll.as_os_str()))?;
        let proc = GetProcAddress(module, s!("CreateWebViewEnvironmentWithOptionsInternal")).ok_or_else(|| windows::core::Error::from(E_NOINTERFACE))?;
        std::mem::transmute::<unsafe extern "system" fn() -> isize, Create>(proc)
    };
    let options: ICoreWebView2EnvironmentOptions = Options.into();
    let handler: ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler = EnvironmentCreated(RefCell::new(Some(Box::new(done)))).into();
    let data_dir = HSTRING::from(data_dir.as_os_str());
    // `true` and runtime type 0 (an installed runtime) are what Microsoft's loader passes.
    unsafe { create(true, 0, PCWSTR(data_dir.as_ptr()), options.as_raw(), handler.as_raw()) }.ok()
}

/// Create the browser inside `parent`; `done` runs on this thread.
pub fn create_controller(
    environment: &ICoreWebView2Environment,
    parent: HWND,
    done: impl FnOnce(Result<ICoreWebView2Controller>) + 'static,
) -> Result<()> {
    let handler: ICoreWebView2CreateCoreWebView2ControllerCompletedHandler = ControllerCreated(RefCell::new(Some(Box::new(done)))).into();
    unsafe { environment.CreateCoreWebView2Controller(parent, handler.as_raw()) }.ok()
}

/// Let only navigations `allow` approves happen in the top-level page.
pub fn on_navigation(webview: &ICoreWebView2, allow: impl Fn(&str) -> bool + 'static) -> Result<()> {
    let handler: ICoreWebView2NavigationStartingEventHandler = NavigationStarting(Box::new(allow)).into();
    let mut token = 0;
    unsafe { webview.add_NavigationStarting(handler.as_raw(), &mut token) }.ok()
}

/// Handle every new-window request with `open` instead of a WebView2 window.
pub fn on_new_window(webview: &ICoreWebView2, open: impl Fn(&str) + 'static) -> Result<()> {
    let handler: ICoreWebView2NewWindowRequestedEventHandler = NewWindowRequested(Box::new(open)).into();
    let mut token = 0;
    unsafe { webview.add_NewWindowRequested(handler.as_raw(), &mut token) }.ok()
}

/// The browser behind a controller.
pub fn webview(controller: &ICoreWebView2Controller) -> Result<ICoreWebView2> {
    let mut raw = std::ptr::null_mut();
    unsafe { controller.get_CoreWebView2(&mut raw) }.ok()?;
    // SAFETY: `get_CoreWebView2` hands over one reference, owned from here on.
    if raw.is_null() { Err(E_FAIL.into()) } else { Ok(unsafe { ICoreWebView2::from_raw(raw) }) }
}

/// The browser's settings object.
pub fn settings(webview: &ICoreWebView2) -> Result<ICoreWebView2Settings> {
    let mut raw = std::ptr::null_mut();
    unsafe { webview.get_Settings(&mut raw) }.ok()?;
    // SAFETY: as above, one owned reference.
    if raw.is_null() { Err(E_FAIL.into()) } else { Ok(unsafe { ICoreWebView2Settings::from_raw(raw) }) }
}

/// The page's current title.
pub fn title(webview: &ICoreWebView2) -> Option<String> {
    let mut raw = PWSTR::null();
    unsafe { webview.get_DocumentTitle(&mut raw) }.ok().ok()?;
    Some(take_string(raw))
}
