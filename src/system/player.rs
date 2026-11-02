//! The video player inside the notch, for Tako media shares and song requests.
//!
//! The video plays in WebView2 (the Edge engine built into Windows 10 and 11), hosted in a
//! small frameless window of Crest's own that is docked over the video slot the notch draws, so
//! it reads as part of the island. Crest's main window is a layered surface and cannot host the
//! browser itself. See `system::webview` for the bindings.
//!
//! YouTube refuses its embed (error 150, "This video is unavailable") on a page served from an
//! IP address, and with no site behind it at all (error 153). So Crest serves the one page that
//! holds the embed itself, at `http://localhost` on a random port and path, for a minute. The
//! page reports back through its title: `error` when the platform will not play the video, and
//! `ended` when it has finished; Crest reads the title as it keeps the window docked.
//!
//! Safety rules:
//! - Only `https` links to the video platforms Tako supports are accepted ([`allowed`]); anything
//!   else is refused, so a donation can never point the player at an arbitrary page.
//! - The embed is rebuilt from the video id alone (letters, digits, `-`, `_`), never from the
//!   donor's text; the page's CSP allows only those platforms' frames and YouTube's player script.
//! - The local page answers only its own random path, only on the loopback address, for one
//!   minute, and nothing but that page.
//! - The browser cannot leave that page: any other top-level navigation is cancelled, and a
//!   request for a new window opens nothing, except a video link [`allowed`] accepts, which goes
//!   to the default browser. Dev tools, context menus, script dialogs, host objects, and web
//!   messages are off.
//! - Each video gets a new, empty profile of its own (`%LOCALAPPDATA%\Crest\Player`), never
//!   signed in with the Windows account, so it holds no cookies or history of the user's.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Once;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, CreateRoundRectRgn, GetStockObject, HBRUSH, SetWindowRgn};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics, HWND_TOPMOST, IsWindow,
    RegisterClassExW, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SetWindowPos, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{HSTRING, Interface, PCWSTR, w};

use crate::system::dnd;
use crate::system::webview::{self, Color, ICoreWebView2, ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2Environment};

/// Hosts a player link may point at: YouTube, TikTok, and Instagram Reels.
const HOSTS: [&str; 9] = [
    "www.youtube.com",
    "youtube.com",
    "m.youtube.com",
    "youtu.be",
    "www.tiktok.com",
    "tiktok.com",
    "vm.tiktok.com",
    "www.instagram.com",
    "instagram.com",
];
/// A link longer than this is not a video link.
const MAX_URL: usize = 512;
/// How long the local page is served: the browser asks for it within a second or two.
const SERVE_FOR: Duration = Duration::from_secs(60);
/// A request line and headers fit in this; more is not a browser asking for the page.
const MAX_REQUEST: usize = 8 * 1024;
/// Room between a stowed window and the leftmost screen, in pixels.
const STOW_GAP: i32 = 64;
/// The page's title while it plays.
const TITLE: &str = "Crest player";
/// Title the page sets when the platform refuses to play the video.
const TITLE_ERROR: &str = "error";
/// Title the page sets when the video has played to its end.
const TITLE_ENDED: &str = "ended";
/// Title the page sets while the video plays, followed by its position and length in seconds
/// and whether it is muted: `playing 12.5 161.0 0`.
const TITLE_PLAYING: &str = "playing";
/// The same while it is paused.
const TITLE_PAUSED: &str = "paused";

/// Is `url` a video link the player may open?
pub fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    url.len() <= MAX_URL
        && HOSTS.contains(&host)
        && url.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`' | '^' | '{' | '}' | '|'))
}

/// A video Crest knows how to embed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Video {
    /// Video id and start second.
    YouTube(String, u64),
    /// Numeric video id.
    TikTok(String),
    /// Reel shortcode.
    Reel(String),
}

impl Video {
    /// The platform's own embed address, built from the id alone.
    fn embed(&self) -> String {
        match self {
            Video::YouTube(id, 0) => format!("https://www.youtube.com/embed/{id}?autoplay=1&playsinline=1"),
            Video::YouTube(id, start) => format!("https://www.youtube.com/embed/{id}?autoplay=1&playsinline=1&start={start}"),
            // Only the video: no related videos, description, or music card over it.
            Video::TikTok(id) => format!("https://www.tiktok.com/player/v1/{id}?autoplay=1&rel=0&description=0&music_info=0"),
            Video::Reel(id) => format!("https://www.instagram.com/reel/{id}/embed"),
        }
    }
}

/// Characters a video id may have on every platform.
fn id_like(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The video behind an allowed link, if it is one Crest can embed.
fn video(url: &str) -> Option<Video> {
    let rest = url.strip_prefix("https://")?;
    let (host, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let query = query.split('#').next().unwrap_or("");
    let param = |name: &str| query.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='));
    let start = param("t").or_else(|| param("start")).and_then(|t| t.trim_end_matches('s').parse().ok()).unwrap_or(0);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let found = match (host, segments.as_slice()) {
        ("www.youtube.com" | "youtube.com" | "m.youtube.com", ["watch"]) => Video::YouTube(param("v")?.to_string(), start),
        ("www.youtube.com" | "youtube.com" | "m.youtube.com", ["shorts" | "embed" | "live", id]) => Video::YouTube(id.to_string(), start),
        ("youtu.be", [id]) => Video::YouTube(id.to_string(), start),
        ("www.tiktok.com" | "tiktok.com", [_, "video", id]) if id.bytes().all(|b| b.is_ascii_digit()) => Video::TikTok(id.to_string()),
        ("www.instagram.com" | "instagram.com", ["reel" | "reels" | "p", id]) => Video::Reel(id.to_string()),
        _ => return None,
    };
    let id = match &found {
        Video::YouTube(id, _) | Video::TikTok(id) | Video::Reel(id) => id,
    };
    id_like(id).then_some(found)
}

/// The page that holds the video: black, edge to edge. YouTube goes through its player script
/// and TikTok's player reports through `postMessage`, so on both a start with sound that the
/// browser blocks falls back to a muted start, and the page tells Crest through its title that
/// the video will not play or has ended. Instagram is a plain frame with no such reports.
/// `nonce` is the only inline script the CSP lets run.
fn page(video: &Video, nonce: &str) -> String {
    let head = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{TITLE}</title>\
         <style>html,body{{margin:0;height:100%;background:#000;overflow:hidden}}\
         #v,iframe{{border:0;width:100%;height:100%;display:block}}</style></head><body>"
    );
    let body = match video {
        // Sound first: a start the browser blocks (still unstarted or cued after a moment, not
        // merely buffering) is retried muted. Twice a second the title reports the position.
        Video::YouTube(id, start) => format!(
            "<div id=\"v\"></div>\
             <script nonce=\"{nonce}\" src=\"https://www.youtube.com/iframe_api\"></script>\
             <script nonce=\"{nonce}\">\
             var p;\
             function report(){{if(!p||!p.getPlayerState)return;var s=p.getPlayerState();\
             if(s===1||s===2)document.title=(s===1?'{TITLE_PLAYING}':'{TITLE_PAUSED}')+' '+p.getCurrentTime().toFixed(1)+' '+\
             p.getDuration().toFixed(1)+' '+(p.isMuted()?1:0);}}\
             function onYouTubeIframeAPIReady(){{p=new YT.Player('v',{{videoId:'{id}',\
             playerVars:{{autoplay:1,playsinline:1,rel:0,start:{start}}},\
             events:{{onReady:function(e){{e.target.unMute();e.target.playVideo();setInterval(report,500);\
             setTimeout(function(){{var s=e.target.getPlayerState();\
             if(s===-1||s===5){{e.target.mute();e.target.playVideo();}}}},2500);}},\
             onStateChange:function(e){{if(e.data===0)document.title='{TITLE_ENDED}';else report();}},\
             onError:function(){{document.title='{TITLE_ERROR}';}}}}}});}}\
             </script>"
        ),
        // TikTok's player talks through `postMessage` (its embed player documentation); only
        // messages from TikTok itself are read. It starts muted, so it is unmuted once, when it
        // is ready or first reports being muted; a mute after that is the viewer's own.
        Video::TikTok(_) => format!(
            "{}<script nonce=\"{nonce}\">\
             var f=document.getElementById('v'),m=0,u=0;\
             function send(t){{f.contentWindow.postMessage({{'x-tiktok-player':true,type:t}},'https://www.tiktok.com');}}\
             addEventListener('message',function(e){{if(e.origin!=='https://www.tiktok.com')return;\
             var d=e.data;if(typeof d==='string'){{try{{d=JSON.parse(d);}}catch(x){{return;}}}}\
             if(!d||typeof d!=='object')return;var v=d.value;\
             if(d.type==='onStateChange'&&v===0)document.title='{TITLE_ENDED}';\
             else if(d.type==='onStateChange'&&v===1&&document.title.indexOf('{TITLE_PLAYING}')!==0)\
             document.title='{TITLE_PLAYING} 0 0 '+m;\
             else if(d.type==='onPlayerReady'){{u=1;send('unMute');send('play');}}             else if(d.type==='onMute'){{m=v?1:0;if(v&&u<2){{u=2;send('unMute');}}}}\
             else if(d.type==='onCurrentTime'&&v&&typeof v==='object')\
             document.title='{TITLE_PLAYING} '+(+v.currentTime||0).toFixed(1)+' '+(+v.duration||0).toFixed(1)+' '+m;\
             else if(d.type==='onPlayerError'){{var c=v&&typeof v==='object'?v.errorCode:v;\
             if(c===3002){{send('mute');send('play');}}else document.title='{TITLE_ERROR}';}}\
             else if(d.type==='onError')document.title='{TITLE_ERROR}';}});\
             </script>",
            frame(video)
        ),
        Video::Reel(_) => frame(video),
    };
    format!("{head}{body}</body></html>")
}

/// The platform's embed in a full-size frame, `id="v"`.
fn frame(video: &Video) -> String {
    // Built from a checked id, but escaped anyway so the attribute stays well-formed.
    let src = video.embed().replace('&', "&amp;").replace('"', "&quot;");
    format!(
        "<iframe id=\"v\" src=\"{src}\" allow=\"autoplay; encrypted-media; picture-in-picture; fullscreen\" \
         allowfullscreen referrerpolicy=\"strict-origin-when-cross-origin\"></iframe>"
    )
}

/// The CSP of the local page: nothing but the platforms' frames, YouTube's player script, and
/// the page's own script by its nonce.
fn policy(nonce: &str) -> String {
    format!(
        "default-src 'none'; style-src 'unsafe-inline'; \
         script-src 'nonce-{nonce}' https://www.youtube.com https://s.ytimg.com; \
         frame-src https://www.youtube.com https://www.tiktok.com https://www.instagram.com"
    )
}

/// Serve `page` at `/token` on the loopback address for [`SERVE_FOR`]; returns its address. The
/// address names `localhost`, not the IP: YouTube will not play its embed on a page from an IP.
fn serve(page: String, policy: String, token: &str) -> Option<String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    listener.set_nonblocking(true).ok()?;
    let path = format!("/{token}");
    let spawned = std::thread::Builder::new().name("player-page".into()).spawn(move || {
        let until = Instant::now() + SERVE_FOR;
        while Instant::now() < until {
            match listener.accept() {
                Ok((stream, _)) => answer(stream, &path, &page, &policy),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    });
    spawned.ok()?;
    Some(format!("http://localhost:{port}/{token}"))
}

/// Answer one request: the page for its own path, 404 for anything else.
fn answer(mut stream: TcpStream, path: &str, page: &str, policy: &str) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < MAX_REQUEST {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
    }
    let line = String::from_utf8_lossy(&request);
    let line = line.lines().next().unwrap_or("");
    let wanted = line.strip_prefix("GET ").and_then(|rest| rest.split(' ').next()) == Some(path);
    let response = if wanted {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
             Content-Security-Policy: {policy}\r\nReferrer-Policy: strict-origin-when-cross-origin\r\n\
             X-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
            page.len()
        )
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
    };
    let _ = stream.write_all(response.as_bytes());
}

/// How far a playing video is, as its page reports it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Progress {
    /// Seconds played.
    pub at: f64,
    /// Length in seconds; 0 when not known yet.
    pub length: f64,
    pub muted: bool,
}

/// Where a played video stands.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    /// The browser is still starting.
    Loading,
    /// Loaded; the progress once the video has started.
    Playing(Option<Progress>),
    /// The platform will not play this video here (embedding off, removed, private).
    Unplayable,
    /// The video played to its end.
    Ended,
    /// The browser could not start, or stopped working.
    Broken,
}

/// The browser, filled in as WebView2 finishes starting it.
#[derive(Default)]
struct Browser {
    environment: Option<ICoreWebView2Environment>,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    broken: bool,
    /// The session ended before the browser was ready; it is closed as soon as it arrives.
    closed: bool,
}

/// One video being played: the window it is docked in and the browser inside it.
pub struct Session {
    host: HWND,
    browser: Rc<RefCell<Browser>>,
    /// Where it was last docked, so moving it again is skipped when nothing changed.
    docked: Option<RECT>,
    /// Out of sight beyond the screens (see [`Session::stow`]).
    stowed: bool,
}

impl Session {
    /// Start playing `url` in a window owned by `owner`, first placed at `slot` (screen pixels).
    /// `None` when the link is refused, cannot be embedded, or WebView2 is missing.
    pub fn start(url: &str, owner: HWND, slot: RECT) -> Option<Self> {
        if !allowed(url) {
            return None;
        }
        let video = video(url)?;
        if !webview::available() {
            error_log!("player: WebView2 runtime not found");
            return None;
        }
        let token = crate::system::secrets::random_b64(18);
        let data = fresh_profile(&token)?;
        let nonce = crate::system::secrets::random_b64(18);
        let address = serve(page(&video, &nonce), policy(&nonce), &token)?;
        let host = host_window(owner, slot)?;
        let browser = Rc::new(RefCell::new(Browser::default()));
        let shared = browser.clone();
        let started = webview::create_environment(&data, move |environment| {
            let made = environment.and_then(|environment| {
                let target = shared.clone();
                webview::create_controller(&environment, host, move |controller| attach(&target, controller, host, &address))?;
                shared.borrow_mut().environment = Some(environment);
                Ok(())
            });
            if let Err(e) = made {
                error_log!("player: browser did not start: {e}");
                shared.borrow_mut().broken = true;
            }
        });
        if let Err(e) = started {
            error_log!("player: browser did not start: {e}");
            let _ = unsafe { DestroyWindow(host) };
            return None;
        }
        stow(host);
        Some(Self { host, browser, docked: None, stowed: true })
    }

    /// The browser is up (the page may still be loading).
    pub fn ready(&self) -> bool {
        self.browser.borrow().webview.is_some()
    }

    pub fn status(&self) -> Status {
        let browser = self.browser.borrow();
        if browser.broken {
            return Status::Broken;
        }
        let Some(webview) = &browser.webview else { return Status::Loading };
        webview::title(webview).map_or(Status::Broken, |title| status_of(&title))
    }

    /// Put the video over `slot` (screen pixels) with `radius` corners, above Crest's window.
    pub fn dock(&mut self, slot: RECT, radius: i32) {
        if !unsafe { IsWindow(Some(self.host)) }.as_bool() {
            return;
        }
        if self.docked == Some(slot) {
            // Still in place; only make sure it stays above the notch.
            let _ = unsafe { SetWindowPos(self.host, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
            return;
        }
        let (width, height) = (slot.right - slot.left, slot.bottom - slot.top);
        unsafe {
            let _ = SetWindowPos(self.host, Some(HWND_TOPMOST), slot.left, slot.top, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            // The region belongs to the window afterwards; it is not freed here.
            let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, 2 * radius, 2 * radius);
            SetWindowRgn(self.host, Some(region), true);
        }
        if let Some(controller) = &self.browser.borrow().controller {
            unsafe {
                let _ = controller.put_Bounds(RECT { left: 0, top: 0, right: width, bottom: height });
                let _ = controller.put_IsVisible(true.into());
            }
        }
        self.docked = Some(slot);
        self.stowed = false;
    }

    /// Move the video out of sight while the notch moves or closes, or while it plays as sound
    /// only; `dock` brings it back. The window stays shown, just beyond every screen: a hidden
    /// window counts as a page nobody sees, and the browser would not start its video.
    pub fn stow(&mut self) {
        if self.docked.take().is_some() || !self.stowed {
            stow(self.host);
            self.stowed = true;
        }
    }

    /// Close the browser and its window.
    pub fn close(&mut self) {
        let mut browser = self.browser.borrow_mut();
        if browser.closed {
            return;
        }
        browser.closed = true;
        if let Some(controller) = browser.controller.take() {
            let _ = unsafe { controller.Close() };
        }
        browser.webview = None;
        browser.environment = None;
        let _ = unsafe { DestroyWindow(self.host) };
        self.docked = None;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// What the page's title says: see [`TITLE_PLAYING`] and the others.
fn status_of(title: &str) -> Status {
    let mut words = title.split_whitespace();
    match words.next() {
        Some(TITLE_ERROR) => Status::Unplayable,
        Some(TITLE_ENDED) => Status::Ended,
        Some(TITLE_PLAYING | TITLE_PAUSED) => {
            let mut number = || words.next().and_then(|w| w.parse::<f64>().ok()).filter(|n| n.is_finite() && *n >= 0.0);
            let progress = match (number(), number(), number()) {
                (Some(at), Some(length), muted) => Some(Progress { at, length, muted: muted == Some(1.0) }),
                _ => None,
            };
            Status::Playing(progress)
        }
        _ => Status::Playing(None),
    }
}

/// The browser has been created (or failed to be): lock it to the page and load it.
fn attach(browser: &Rc<RefCell<Browser>>, controller: windows::core::Result<ICoreWebView2Controller>, host: HWND, address: &str) {
    let mut state = browser.borrow_mut();
    let controller = match controller {
        Ok(controller) if state.closed => {
            let _ = unsafe { controller.Close() };
            return;
        }
        Ok(controller) => controller,
        Err(e) => {
            error_log!("player: browser did not start: {e}");
            state.broken = true;
            return;
        }
    };
    match setup(&controller, host, address) {
        Ok(webview) => {
            state.controller = Some(controller);
            state.webview = Some(webview);
        }
        Err(e) => {
            error_log!("player: browser setup failed: {e}");
            let _ = unsafe { controller.Close() };
            state.broken = true;
        }
    }
}

/// Settings, the navigation lock, the black background, the size, then the page itself.
fn setup(controller: &ICoreWebView2Controller, host: HWND, address: &str) -> windows::core::Result<ICoreWebView2> {
    let webview = webview::webview(controller)?;
    let settings = webview::settings(&webview)?;
    unsafe {
        settings.put_AreDevToolsEnabled(false.into()).ok()?;
        settings.put_AreDefaultContextMenusEnabled(false.into()).ok()?;
        settings.put_AreDefaultScriptDialogsEnabled(false.into()).ok()?;
        settings.put_AreHostObjectsAllowed(false.into()).ok()?;
        settings.put_IsWebMessageEnabled(false.into()).ok()?;
        settings.put_IsStatusBarEnabled(false.into()).ok()?;
        settings.put_IsZoomControlEnabled(false.into()).ok()?;
    }
    // Black under the page, so nothing white flashes while it loads (runtime 87 and later).
    if let Ok(controller) = controller.cast::<ICoreWebView2Controller2>() {
        let _ = unsafe { controller.put_DefaultBackgroundColor(Color { a: 255, r: 0, g: 0, b: 0 }) };
    }
    let page = address.to_string();
    webview::on_navigation(&webview, move |uri| uri == page)?;
    webview::on_new_window(&webview, |uri| {
        if allowed(uri) {
            dnd::open_url(uri);
        }
    })?;
    let mut area = RECT::default();
    unsafe {
        let _ = GetClientRect(host, &mut area);
        controller.put_Bounds(area).ok()?;
        let address = HSTRING::from(address);
        webview.Navigate(PCWSTR(address.as_ptr())).ok()?;
    }
    Ok(webview)
}

/// A new, empty browser profile for one video, in `%LOCALAPPDATA%\Crest\Player`. Earlier
/// profiles are deleted first; one a closing browser still holds is left for the next time. A
/// profile per video also keeps a new video from waiting on the last one's browser to exit.
fn fresh_profile(name: &str) -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Crest").join("Player");
    for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let _ = std::fs::remove_dir_all(entry.path());
    }
    Some(root.join(name))
}

/// Put `window` just left of the leftmost screen, still shown and the same size.
fn stow(window: HWND) {
    let mut area = RECT::default();
    unsafe {
        let _ = GetClientRect(window, &mut area);
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN) - area.right - STOW_GAP;
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let _ = SetWindowPos(window, None, left, top, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW);
    }
}

unsafe extern "system" fn host_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// The frameless, black window the browser lives in: a tool window (no taskbar button, not in
/// Alt+Tab) that never takes focus from the app the user is in, out of sight until docked.
fn host_window(owner: HWND, slot: RECT) -> Option<HWND> {
    static CLASS: Once = Once::new();
    let instance = unsafe { GetModuleHandleW(None) }.ok()?;
    let class = w!("CrestPlayer");
    CLASS.call_once(|| {
        let info = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_proc),
            hInstance: instance.into(),
            hbrBackground: HBRUSH(unsafe { GetStockObject(BLACK_BRUSH) }.0),
            lpszClassName: class,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&info) };
    });
    let (width, height) = (slot.right - slot.left, slot.bottom - slot.top);
    let created = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class,
            &HSTRING::from(TITLE),
            WS_POPUP | WS_CLIPCHILDREN,
            slot.left,
            slot.top,
            width,
            height,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
    };
    match created {
        Ok(window) => Some(window),
        Err(e) => {
            error_log!("player: window not created: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Play `url` in the real WebView2 for up to `secs`, pumping messages like the app does;
    /// returns the last status seen and whether the video really started (stops early once the
    /// page reports an error or the end).
    fn play_for(url: &str, secs: u64, shown: bool) -> (Status, bool) {
        use windows::Win32::System::Ole::OleInitialize;
        use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};
        let _ = unsafe { OleInitialize(None) };
        // Off screen: the test needs a real, shown window, not one in the user's way.
        let slot = RECT { left: -4000, top: -4000, right: -3360, bottom: -3640 };
        let owner = host_window(HWND::default(), slot).expect("owner window");
        let mut session = Session::start(url, owner, slot).expect("player starts");
        let until = Instant::now() + Duration::from_secs(secs);
        let mut status = Status::Loading;
        let mut started = false;
        while Instant::now() < until && !matches!(status, Status::Unplayable | Status::Ended | Status::Broken) {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            status = session.status();
            // Started with sound: the page reports it playing, not muted.
            started |= matches!(status, Status::Playing(Some(Progress { at, muted: false, .. })) if at > 0.0);
            if shown && session.ready() {
                session.dock(slot, 12);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        session.close();
        let _ = unsafe { DestroyWindow(owner) };
        (status, started)
    }

    /// Against the WebView2 runtime and YouTube: `cargo test webview2 -- --ignored --nocapture --test-threads=1`.
    #[test]
    #[ignore]
    fn webview2_plays_an_embeddable_video() {
        let (status, started) = play_for("https://www.youtube.com/watch?v=-jNK03W1M2g", 20, true);
        assert!(started && matches!(status, Status::Playing(Some(_))), "{status:?}, started {started}");
    }

    /// A song share plays as sound only: its window is never shown.
    #[test]
    #[ignore]
    fn webview2_plays_audio_without_showing_the_video() {
        let (status, started) = play_for("https://www.youtube.com/watch?v=-jNK03W1M2g", 20, false);
        assert!(started && matches!(status, Status::Playing(Some(_))), "{status:?}, started {started}");
    }

    /// The example video from TikTok's embed player documentation.
    #[test]
    #[ignore]
    fn webview2_plays_a_tiktok_video() {
        // A short video: it may well finish inside the window, which is reported as its end.
        let (status, started) = play_for("https://www.tiktok.com/@scout2015/video/6718335390845095173", 25, true);
        assert!(started && matches!(status, Status::Playing(_) | Status::Ended), "{status:?}, started {started}");
    }

    #[test]
    #[ignore]
    fn webview2_reports_a_video_it_cannot_play() {
        assert_eq!(play_for("https://www.youtube.com/watch?v=aaaaaaaaaaa", 20, true).0, Status::Unplayable);
    }

    #[test]
    fn reads_the_page_title() {
        assert_eq!(status_of("Crest player"), Status::Playing(None));
        assert_eq!(status_of("playing 12.5 161.0 0"), Status::Playing(Some(Progress { at: 12.5, length: 161.0, muted: false })));
        assert_eq!(status_of("paused 3 0 1"), Status::Playing(Some(Progress { at: 3.0, length: 0.0, muted: true })));
        assert_eq!(status_of("playing NaN x"), Status::Playing(None));
        assert_eq!(status_of("error"), Status::Unplayable);
        assert_eq!(status_of("ended"), Status::Ended);
    }

    #[test]
    fn only_video_links_are_allowed() {
        assert!(allowed("https://www.youtube.com/watch?v=mm0YkIJZJ4w&t=30s"));
        assert!(allowed("https://www.tiktok.com/@someone/video/7312345678901234567"));
        assert!(allowed("https://www.instagram.com/reel/C1a2b3c4/"));
        // Not https, another host, a look-alike host, or a link that could break the argument.
        assert!(!allowed("http://www.youtube.com/watch?v=x"));
        assert!(!allowed("https://example.com/watch?v=x"));
        assert!(!allowed("https://www.youtube.com.evil.example/watch?v=x"));
        assert!(!allowed("https://www.youtube.com@evil.example/watch?v=x"));
        assert!(!allowed("https://evil.example/?u=https://www.youtube.com/"));
        assert!(!allowed("https://www.youtube.com/watch?v=x\" --remote-debugging-port=9222"));
        assert!(!allowed("https://www.youtube.com/watch?v=x --disable-web-security"));
        assert!(!allowed("file:///C:/Windows/System32/calc.exe"));
        assert!(!allowed(&format!("https://www.youtube.com/watch?v={}", "a".repeat(600))));
    }

    #[test]
    fn links_become_embeds_from_the_id_alone() {
        let embed = |url: &str| video(url).map(|v| v.embed());
        assert_eq!(embed("https://www.youtube.com/watch?v=mm0YkIJZJ4w&t=30s").as_deref(), Some("https://www.youtube.com/embed/mm0YkIJZJ4w?autoplay=1&playsinline=1&start=30"));
        assert_eq!(embed("https://youtu.be/mm0YkIJZJ4w").as_deref(), Some("https://www.youtube.com/embed/mm0YkIJZJ4w?autoplay=1&playsinline=1"));
        assert_eq!(embed("https://www.youtube.com/shorts/abcDEF_123").as_deref(), Some("https://www.youtube.com/embed/abcDEF_123?autoplay=1&playsinline=1"));
        assert_eq!(embed("https://www.tiktok.com/@a.b/video/7312345678901234567").as_deref(), Some("https://www.tiktok.com/player/v1/7312345678901234567?autoplay=1&rel=0&description=0&music_info=0"));
        assert_eq!(embed("https://www.instagram.com/reel/C1a2b3c4/").as_deref(), Some("https://www.instagram.com/reel/C1a2b3c4/embed"));
        // An id with anything else in it is not embedded.
        assert_eq!(video("https://www.youtube.com/watch?v=a%22b"), None);
        assert_eq!(video("https://www.youtube.com/channel/UCxyz"), None);
        assert!(video("https://www.tiktok.com/@a/video/12ab").is_none());
    }

    #[test]
    fn the_local_page_serves_only_its_own_path() {
        let video = Video::YouTube("mm0YkIJZJ4w".into(), 12);
        let address = serve(page(&video, "NONCE"), policy("NONCE"), "TOKEN").unwrap();
        let port = address.strip_prefix("http://localhost:").expect("named host, not an IP").split_once('/').unwrap().0;
        let host = format!("127.0.0.1:{port}");
        let get = |path: &str| {
            let mut stream = TcpStream::connect(&host).unwrap();
            write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            answer
        };
        let found = get("/TOKEN");
        assert!(found.starts_with("HTTP/1.1 200"));
        assert!(found.contains("script-src 'nonce-NONCE' https://www.youtube.com"));
        assert!(found.contains("<title>Crest player</title>"));
        assert!(found.contains("onError:function(){document.title='error';}"), "the page reports a video that will not play");
        assert!(found.contains("if(e.data===0)document.title='ended';"), "and one that has ended");
        assert!(found.contains("videoId:'mm0YkIJZJ4w'") && found.contains("start:12"));
        assert!(get("/").starts_with("HTTP/1.1 404"));
        assert!(get("/favicon.ico").starts_with("HTTP/1.1 404"));
        // Other platforms are a plain frame of their own embed.
        let reel = page(&Video::Reel("C1a2b3c4".into()), "N");
        assert!(reel.contains("<iframe id=\"v\" src=\"https://www.instagram.com/reel/C1a2b3c4/embed\""));
        // TikTok reports its end and errors, and listens only to TikTok itself.
        let tiktok = page(&Video::TikTok("7312345678901234567".into()), "N");
        assert!(tiktok.contains("e.origin!=='https://www.tiktok.com'") && tiktok.contains("document.title='ended'"));
    }
}
