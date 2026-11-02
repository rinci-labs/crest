//! The video player inside the notch, for Tako media shares and song requests.
//!
//! Crest's own window is a layered surface that cannot host a web view, and WebView2 has no
//! bindings in the `windows` crate, so the video is Microsoft Edge in app mode (the engine
//! WebView2 embeds, shipped with Windows 10 and 11). Crest strips that window's frame, makes it a
//! topmost tool window with rounded corners, and docks it over the video slot it draws in the
//! notch, so it reads as part of the island. The app keeps it aligned as the shape moves.
//!
//! YouTube refuses its video-only embed on a page with no site behind it (error 153), so Crest
//! serves the one page that holds the embed itself, from `127.0.0.1` on a random port and path,
//! for a minute; Edge loads it from there. The page starts the video with sound and, if the
//! browser does not allow that, starts it muted (the player's own button unmutes).
//!
//! Safety rules:
//! - Only `https` links to the video platforms Tako supports are accepted ([`allowed`]); anything
//!   else is refused, so a donation can never point the player at an arbitrary page.
//! - The embed is rebuilt from the video id alone (letters, digits, `-`, `_`), never from the
//!   donor's text; the page's CSP allows only those platforms' frames and YouTube's player script.
//! - The local page answers only its own random path, only on the loopback address, for one
//!   minute, and nothing but that page.
//! - The window is InPrivate: the page gets no cookies, logins, or history from the user's Edge,
//!   and nothing it stores outlives the window. No separate profile folder is used on purpose:
//!   Edge signs any new profile into the Windows account and syncs passwords and extensions into
//!   it, InPrivate or not, so a "separate" profile would be a copy of the user's data on disk.
//! - Crest adds no other switches: if Edge is not running yet, the process it starts is the
//!   user's own browser, and Crest must not change how that behaves.
//! - The link is passed as one argument to `msedge.exe` (no shell). Crest only finds the window
//!   by the random token in its title, and only moves, clips, hides, or closes that window.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, GetWindowTextW, HWND_TOPMOST, IsWindow, PostMessageW,
    SW_HIDE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WM_CLOSE, WS_CAPTION, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP,
    WS_THICKFRAME,
};
use windows::core::{BOOL, PCWSTR, w};

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
/// Where Edge registers its executable for every user, and for this one.
const APP_PATH: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\msedge.exe");
/// How long the local page is served: Edge asks for it within a second or two.
const SERVE_FOR: Duration = Duration::from_secs(60);
/// A request line and headers fit in this; more is not a browser asking for the page.
const MAX_REQUEST: usize = 8 * 1024;
/// Height of the caption Chromium draws inside an app window, in DIP. Docking moves it above
/// the visible part of the window and clips it off.
const CAPTION: i32 = 32;
/// Page title prefix; the random token after it is how the window is found.
const TITLE: &str = "Crest player";

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
            Video::TikTok(id) => format!("https://www.tiktok.com/player/v1/{id}?autoplay=1"),
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

/// The page that holds the video: black, edge to edge. YouTube goes through its player script,
/// so a start with sound that the browser blocks can fall back to a muted start; the others are
/// a plain frame. `nonce` is the only inline script the CSP lets run.
fn page(video: &Video, token: &str, nonce: &str) -> String {
    let head = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{TITLE} {token}</title>\
         <style>html,body{{margin:0;height:100%;background:#000;overflow:hidden}}\
         #v,iframe{{border:0;width:100%;height:100%;display:block}}</style></head><body>"
    );
    let body = match video {
        Video::YouTube(id, start) => format!(
            "<div id=\"v\"></div>\
             <script nonce=\"{nonce}\" src=\"https://www.youtube.com/iframe_api\"></script>\
             <script nonce=\"{nonce}\">\
             function onYouTubeIframeAPIReady(){{new YT.Player('v',{{videoId:'{id}',\
             playerVars:{{autoplay:1,playsinline:1,rel:0,start:{start}}},\
             events:{{onReady:function(e){{e.target.playVideo();setTimeout(function(){{\
             if(e.target.getPlayerState()!==1){{e.target.mute();e.target.playVideo();}}}},1500);}}}}}});}}\
             </script>"
        ),
        other => {
            // Built from a checked id, but escaped anyway so the attribute stays well-formed.
            let src = other.embed().replace('&', "&amp;").replace('"', "&quot;");
            format!(
                "<iframe src=\"{src}\" allow=\"autoplay; encrypted-media; picture-in-picture; fullscreen\" \
                 allowfullscreen referrerpolicy=\"strict-origin-when-cross-origin\"></iframe>"
            )
        }
    };
    format!("{head}{body}</body></html>")
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

/// Serve `page` at `/token` on the loopback address for [`SERVE_FOR`]; returns its address.
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
    Some(format!("http://127.0.0.1:{port}/{token}"))
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

/// One video being played: how its window is recognised, and the window once it is found.
pub struct Session {
    /// The random part of the page title.
    token: String,
    /// Edge's window, once it has appeared and been docked.
    window: Option<HWND>,
    /// Where it was last docked, so moving it again is skipped when nothing changed.
    docked: Option<RECT>,
}

impl Session {
    /// Start playing `url`, with Edge's window first placed at `slot` (screen pixels). `None`
    /// when the link is refused, cannot be embedded, or Edge is missing.
    pub fn start(url: &str, slot: RECT) -> Option<Self> {
        if !allowed(url) {
            return None;
        }
        let video = video(url)?;
        let Some(edge) = edge() else {
            error_log!("player: Microsoft Edge not found");
            return None;
        };
        let token = crate::system::secrets::random_b64(18);
        let nonce = crate::system::secrets::random_b64(18);
        let address = serve(page(&video, &token, &nonce), policy(&nonce), &token)?;
        let (width, height) = (slot.right - slot.left, slot.bottom - slot.top);
        let spawned = Command::new(edge)
            .arg(format!("--app={address}"))
            .arg("--inprivate")
            .arg(format!("--window-size={width},{height}"))
            .arg(format!("--window-position={},{}", slot.left, slot.top))
            .spawn();
        if let Err(e) = spawned {
            error_log!("player: cannot start Edge: {e}");
            return None;
        }
        Some(Self { token, window: None, docked: None })
    }

    /// The window has been found and is still open.
    pub fn ready(&self) -> bool {
        self.window.is_some_and(|w| unsafe { IsWindow(Some(w)) }.as_bool())
    }

    /// The window was found once and has since been closed (by the user, or Edge quit).
    pub fn ended(&self) -> bool {
        self.window.is_some() && !self.ready()
    }

    /// Look for the window by its title; once found it loses its frame and taskbar button.
    pub fn find(&mut self) -> bool {
        if self.window.is_some() {
            return true;
        }
        let Some(window) = find_titled(&format!("{TITLE} {}", self.token)) else { return false };
        unsafe {
            let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
            let frame = WS_CAPTION.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(window, GWL_STYLE, ((style & !frame) | WS_POPUP.0) as isize);
            let ex = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(window, GWL_EXSTYLE, ((ex & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0) as isize);
        }
        self.window = Some(window);
        true
    }

    /// Put the video over `slot` (screen pixels) with `radius` corners, above Crest's window.
    pub fn dock(&mut self, slot: RECT, radius: i32) {
        let Some(window) = self.window.filter(|_| self.ready()) else { return };
        if self.docked == Some(slot) {
            // Still in place; only make sure it stays above the notch.
            let _ = unsafe { SetWindowPos(window, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
            return;
        }
        let caption = (CAPTION * unsafe { GetDpiForWindow(window) } as i32) / 96;
        let (width, height) = (slot.right - slot.left, slot.bottom - slot.top);
        unsafe {
            let flags = SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW;
            let _ = SetWindowPos(window, Some(HWND_TOPMOST), slot.left, slot.top - caption, width, height + caption, flags);
            // The region belongs to the window afterwards; it is not freed here.
            let region = CreateRoundRectRgn(0, caption, width + 1, height + caption + 1, 2 * radius, 2 * radius);
            SetWindowRgn(window, Some(region), true);
        }
        self.docked = Some(slot);
    }

    /// Hide the video while the notch moves or closes; `dock` shows it again.
    pub fn hide(&mut self) {
        if let Some(window) = self.window.filter(|_| self.ready())
            && self.docked.is_some()
        {
            let _ = unsafe { ShowWindow(window, SW_HIDE) };
            self.docked = None;
        }
    }

    /// Close the video's window.
    pub fn close(&mut self) {
        if let Some(window) = self.window.filter(|_| self.ready()) {
            let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        self.window = None;
        self.docked = None;
    }
}

/// The top-level window whose title contains `part`.
fn find_titled(part: &str) -> Option<HWND> {
    struct Search<'a> {
        part: &'a str,
        found: Option<HWND>,
    }
    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `Search` below, alive for the whole `EnumWindows` call.
        let search = unsafe { &mut *(data.0 as *mut Search) };
        let mut title = [0u16; 256];
        let len = unsafe { GetWindowTextW(window, &mut title) } as usize;
        if String::from_utf16_lossy(&title[..len]).contains(search.part) {
            search.found = Some(window);
            return false.into();
        }
        true.into()
    }
    let mut search = Search { part, found: None };
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.found
}

/// `msedge.exe` from its App Paths entry, machine-wide first.
fn edge() -> Option<PathBuf> {
    [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER].into_iter().find_map(|root| {
        let mut buffer = [0u16; 520];
        let mut size = (buffer.len() * 2) as u32;
        let read = unsafe { RegGetValueW(root, APP_PATH, PCWSTR::null(), RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size)) };
        if read.is_err() {
            return None;
        }
        let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        let path = PathBuf::from(String::from_utf16_lossy(&buffer[..len]).trim_matches('"'));
        path.is_file().then_some(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(embed("https://www.tiktok.com/@a.b/video/7312345678901234567").as_deref(), Some("https://www.tiktok.com/player/v1/7312345678901234567?autoplay=1"));
        assert_eq!(embed("https://www.instagram.com/reel/C1a2b3c4/").as_deref(), Some("https://www.instagram.com/reel/C1a2b3c4/embed"));
        // An id with anything else in it is not embedded.
        assert_eq!(video("https://www.youtube.com/watch?v=a%22b"), None);
        assert_eq!(video("https://www.youtube.com/channel/UCxyz"), None);
        assert!(video("https://www.tiktok.com/@a/video/12ab").is_none());
    }

    #[test]
    fn the_local_page_serves_only_its_own_path() {
        let video = Video::YouTube("mm0YkIJZJ4w".into(), 12);
        let address = serve(page(&video, "TOKEN", "NONCE"), policy("NONCE"), "TOKEN").unwrap();
        let host = address.strip_prefix("http://").unwrap().split_once('/').unwrap().0.to_string();
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
        assert!(found.contains("<title>Crest player TOKEN</title>"));
        assert!(found.contains("videoId:'mm0YkIJZJ4w'") && found.contains("start:12"));
        assert!(get("/").starts_with("HTTP/1.1 404"));
        assert!(get("/favicon.ico").starts_with("HTTP/1.1 404"));
        // Other platforms are a plain frame of their own embed.
        let reel = page(&Video::Reel("C1a2b3c4".into()), "T", "N");
        assert!(reel.contains("<iframe src=\"https://www.instagram.com/reel/C1a2b3c4/embed\""));
    }
}
