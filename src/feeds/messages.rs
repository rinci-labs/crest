//! Incoming messages from chat apps (WhatsApp, Telegram, and others), read from Windows
//! notifications via `UserNotificationListener`.
//!
//! Browser notifications (e.g. WhatsApp Web in Brave) are read too, but Windows only provides
//! their title and body, not the sending site's address. So Crest can't tell which site sent
//! them: they are all marked as browser messages, and only shown when the user turns on
//! "Browser notifications".
//!
//! Windows only allows this for apps with package identity. So the feature only works after
//! incoming messages are turned on from the tray menu (see `identity`) and the user grants
//! notification access once. Without that, everything here fails quietly.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use windows::ApplicationModel::Package;
use windows::Foundation::{Size, TypedEventHandler};
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{
    KnownNotificationBindings, NotificationKinds, UserNotification, UserNotificationChangedEventArgs,
    UserNotificationChangedKind,
};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, IsIconic, IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow,
};
use windows::core::{BOOL, Ref, Result};
use windows_future::AsyncOperationCompletedHandler;

use crate::system::dnd;
use crate::feeds::media::decode_square;
use crate::system::services::{Event, Notifier};

/// App logo side in pixels; sharp for 26 DIP up to 150% scale.
pub const LOGO_PX: u32 = 48;

/// Recognized chat apps (matched against their display name).
const CHAT_APPS: [&str; 13] = [
    "whatsapp", "telegram", "discord", "signal", "messenger", "instagram", "line", "slack", "teams",
    "zalo", "wechat", "skype", "viber",
];

/// Recognized browsers, for messages from web versions (e.g. WhatsApp Web).
const BROWSERS: [&str; 10] = ["brave", "chrome", "edge", "firefox", "opera", "vivaldi", "chromium", "arc", "zen", "yandex"];

pub struct Message {
    pub app: String,
    /// AppUserModelID, to open the app when the notice is clicked.
    pub aumid: String,
    pub sender: String,
    pub text: String,
    /// App logo (premultiplied BGRA, `LOGO_PX` square).
    pub logo: Option<Arc<Vec<u8>>>,
    /// From a browser: could be from any site (see the module docs).
    pub from_browser: bool,
}

impl Message {
    pub fn is_whatsapp(&self) -> bool {
        self.app.to_lowercase().contains("whatsapp")
    }
}

/// The app name contains one of `names` as a whole word ("line" does not match "Outline").
fn has_word(app: &str, names: &[&str]) -> bool {
    let app = app.to_lowercase();
    app.split(|c: char| !c.is_alphanumeric()).any(|word| names.contains(&word))
}

fn is_chat(app: &str) -> bool {
    has_word(app, &CHAT_APPS)
}

fn is_browser(app: &str) -> bool {
    has_word(app, &BROWSERS)
}

/// A classified notification.
#[derive(Debug, PartialEq)]
struct Parsed {
    app: String,
    from_browser: bool,
    sender: String,
    text: String,
}

/// Classifies a notification from `app` by its text lines: first line = sender, the rest =
/// body. Only chat apps and browsers are accepted.
fn classify(app: &str, lines: Vec<String>) -> Option<Parsed> {
    let from_browser = !is_chat(app) && is_browser(app);
    if !is_chat(app) && !from_browser {
        return None;
    }
    let mut lines = lines.into_iter();
    let sender = lines.next().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| app.to_string());
    let text = lines.filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join(" ");
    Some(Parsed { app: app.to_string(), from_browser, sender, text })
}

/// Opens the chat app when its message notice is clicked. Browser messages go to a WhatsApp
/// window if there is one (e.g. an active WhatsApp Web tab), otherwise to the browser window.
pub fn open(hwnd: HWND, aumid: &str, app: &str, from_browser: bool) {
    if from_browser && (focus_window_titled("whatsapp") || focus_window_titled(app)) {
        return;
    }
    dnd::open_app(hwnd, aumid);
}

/// Brings forward the window whose title contains `name` (e.g. a "WhatsApp" browser tab).
pub fn focus_window_titled(name: &str) -> bool {
    struct Search {
        needle: String,
        found: Option<HWND>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut title = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut title) };
        if len > 0 && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let title = String::from_utf16_lossy(&title[..len as usize]).to_lowercase();
            if title.contains(&search.needle) {
                search.found = Some(hwnd);
                return false.into();
            }
        }
        true.into()
    }
    if name.trim().is_empty() {
        return false;
    }
    let mut search = Search { needle: name.to_lowercase(), found: None };
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    let Some(window) = search.found else { return false };
    unsafe {
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        SetForegroundWindow(window).as_bool()
    }
}

/// Crest runs with package identity (see `packaging/`); without it Windows does not allow
/// reading notifications.
pub fn packaged() -> bool {
    Package::Current().is_ok()
}

/// Asks for permission to read notifications if not asked before. The result arrives as
/// [`Event::MessagesAccess`]. Must be called from the window thread since it may show a dialog.
pub fn request_access(notify: Notifier) {
    let Ok(listener) = UserNotificationListener::Current() else {
        notify.send(Event::MessagesAccess(false));
        return;
    };
    match listener.GetAccessStatus() {
        Ok(UserNotificationListenerAccessStatus::Allowed) => notify.send(Event::MessagesAccess(true)),
        Ok(UserNotificationListenerAccessStatus::Unspecified) => {
            let asked = listener.RequestAccessAsync().and_then(|op| {
                let reply = notify.clone();
                op.SetCompleted(&AsyncOperationCompletedHandler::new(move |op, _| {
                    let allowed = op
                        .as_ref()
                        .and_then(|op| op.GetResults().ok())
                        .is_some_and(|status| status == UserNotificationListenerAccessStatus::Allowed);
                    reply.send(Event::MessagesAccess(allowed));
                    Ok(())
                }))
            });
            if let Err(e) = asked {
                error_log!("could not request notification access: {e}");
                notify.send(Event::MessagesAccess(false));
            }
        }
        // Denied, or the app has no package identity.
        _ => notify.send(Event::MessagesAccess(false)),
    }
}

/// Listens for new notifications while alive.
pub struct Monitor {
    listener: UserNotificationListener,
    token: i64,
}

impl Monitor {
    pub fn start(notify: Notifier) -> Result<Self> {
        let listener = UserNotificationListener::Current()?;
        let reader = listener.clone();
        let logos: Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>> = Arc::default();
        let token = listener.NotificationChanged(&TypedEventHandler::new(
            move |_, args: Ref<UserNotificationChangedEventArgs>| {
                if let Some(args) = args.as_ref()
                    && args.ChangeKind()? == UserNotificationChangedKind::Added
                    && let Some(message) = read(&reader, args.UserNotificationId()?, &logos)
                {
                    notify.send(Event::Message(message));
                }
                Ok(())
            },
        ))?;
        Ok(Self { listener, token })
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.listener.RemoveNotificationChanged(self.token);
    }
}

/// The notification's text lines.
fn lines_of(notification: &UserNotification) -> Option<Vec<String>> {
    let binding = notification
        .Notification()
        .ok()?
        .Visual()
        .ok()?
        .GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?)
        .ok()?;
    let texts = binding.GetTextElements().ok()?;
    Some((0..texts.Size().ok()?).filter_map(|i| texts.GetAt(i).ok()?.Text().ok()).map(|s| s.to_string()).collect())
}

/// Reads one notification; only from chat apps or browsers.
fn read(listener: &UserNotificationListener, id: u32, logos: &Mutex<HashMap<String, Arc<Vec<u8>>>>) -> Option<Message> {
    let notification = listener.GetNotification(id).ok()?;
    let info = notification.AppInfo().ok()?;
    let display = info.DisplayInfo().ok()?;
    let app = display.DisplayName().ok()?.to_string();
    let Parsed { app, from_browser, sender, text } = classify(&app, lines_of(&notification)?)?;
    let aumid = info.AppUserModelId().map(|s| s.to_string()).unwrap_or_default();

    let mut cache = logos.lock().unwrap_or_else(|e| e.into_inner());
    let logo = match cache.get(&aumid) {
        Some(logo) => Some(logo.clone()),
        None => {
            let size = Size { Width: LOGO_PX as f32, Height: LOGO_PX as f32 };
            let logo = display.GetLogo(size).ok().and_then(|r| decode_square(&r.into(), LOGO_PX).ok()).map(Arc::new);
            if let Some(logo) = &logo {
                cache.insert(aumid.clone(), logo.clone());
            }
            logo
        }
    };
    Some(Message { app, aumid, sender, text, logo, from_browser })
}

/// Check (`crest.exe --check-notifications <file>`): writes the access status and the
/// notifications in the notification center, along with how Crest classifies them. Message
/// contents and sender names are not written; only the app name and line count.
///
/// With `listen`, Crest also listens for new notifications for that long and records each one
/// (to confirm that new notification events actually arrive).
pub fn diagnose(out: &Path, listen: Duration) -> std::io::Result<()> {
    let mut report = vec![format!("package identity: {}", if packaged() { "yes" } else { "no" })];
    let listener = UserNotificationListener::Current().ok();
    let status = listener.as_ref().and_then(|l| l.GetAccessStatus().ok());
    report.push(format!(
        "notification access: {}",
        match status {
            Some(UserNotificationListenerAccessStatus::Allowed) => "allowed",
            Some(UserNotificationListenerAccessStatus::Denied) => "denied",
            Some(_) => "never asked",
            None => "unavailable",
        }
    ));
    if let (Some(listener), Some(UserNotificationListenerAccessStatus::Allowed)) = (&listener, status) {
        let list = listener.GetNotificationsAsync(NotificationKinds::Toast).and_then(|op| op.join());
        match list {
            Ok(list) => {
                report.push(format!("notifications in the notification center: {}", list.Size().unwrap_or(0)));
                for notification in list {
                    report.push(format!("- {}", describe(&notification)));
                }
            }
            Err(e) => report.push(format!("could not read the notification list: {e}")),
        }
        if !listen.is_zero() {
            report.push(format!("listening for new notifications for {} seconds:", listen.as_secs()));
            std::fs::write(out, report.join("\r\n"))?;
            listen_for(listener, listen, out, &mut report)?;
            report.push("done".to_string());
        }
    }
    std::fs::write(out, report.join("\r\n"))
}

/// Records notifications added or removed during `listen`; the file is rewritten on every event.
fn listen_for(listener: &UserNotificationListener, listen: Duration, out: &Path, report: &mut Vec<String>) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel::<(bool, u32)>();
    let handler = TypedEventHandler::new(move |_, args: Ref<UserNotificationChangedEventArgs>| {
        if let Some(args) = args.as_ref() {
            let added = args.ChangeKind()? == UserNotificationChangedKind::Added;
            let _ = tx.send((added, args.UserNotificationId()?));
        }
        Ok(())
    });
    let token = match listener.NotificationChanged(&handler) {
        Ok(token) => token,
        Err(e) => {
            report.push(format!("could not listen for new notifications: {e}"));
            return Ok(());
        }
    };
    let deadline = Instant::now() + listen;
    while let Ok((added, id)) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        let line = match (added, listener.GetNotification(id)) {
            (true, Ok(notification)) => format!("+ new: {}", describe(&notification)),
            (true, Err(_)) => "+ new, but gone before it could be read".to_string(),
            (false, _) => "- a notification was removed".to_string(),
        };
        report.push(line);
        std::fs::write(out, report.join("\r\n"))?;
    }
    let _ = listener.RemoveNotificationChanged(token);
    Ok(())
}

/// Summary of one notification without message contents or sender name.
fn describe(notification: &UserNotification) -> String {
    let info = notification.AppInfo().ok();
    let app = info
        .as_ref()
        .and_then(|i| i.DisplayInfo().ok())
        .and_then(|d| d.DisplayName().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let aumid = info.and_then(|i| i.AppUserModelId().ok()).map(|s| s.to_string()).unwrap_or_default();
    let lines = lines_of(notification).unwrap_or_default();
    let verdict = match classify(&app, lines.clone()) {
        Some(parsed) if parsed.from_browser => "browser message (shown when Browser notifications is on)".to_string(),
        Some(parsed) => format!("{} message", parsed.app),
        None => "ignored".to_string(),
    };
    format!("{app} [{aumid}]: {} lines, {verdict}", lines.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_chat_apps() {
        assert!(is_chat("WhatsApp"));
        assert!(is_chat("Telegram Desktop"));
        assert!(is_chat("Microsoft Teams"));
        assert!(!is_chat("Windows Security"));
        assert!(!is_chat("Outline"), "'line' inside another word does not count");
    }

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn browser_notifications_are_marked() {
        let web = classify("Brave", lines(&["Mom", "What time are you coming home?"])).unwrap();
        assert!(web.from_browser);
        assert_eq!((web.app.as_str(), web.sender.as_str(), web.text.as_str()), ("Brave", "Mom", "What time are you coming home?"));
        assert!(classify("Google Chrome", lines(&["Office Group", "Meeting at 3"])).unwrap().from_browser);
        assert!(classify("Microsoft Edge", lines(&["Mom"])).is_some_and(|p| p.from_browser && p.text.is_empty()));
        // The WhatsApp app itself is not a browser message; other apps are ignored.
        assert!(!classify("WhatsApp", lines(&["Mom", "Hi"])).unwrap().from_browser);
        assert!(classify("NVIDIA App", lines(&["New driver", "Install now"])).is_none());
        assert!(classify("Snipping Tool", lines(&["Screenshot copied"])).is_none());
    }
}
