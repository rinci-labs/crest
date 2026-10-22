//! System tray icon and its menu.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, HICON, HMENU,
    MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
    PostMessageW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TRACK_POPUP_MENU_FLAGS, TrackPopupMenu, WM_APP, WM_NULL,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::shell::settings::{Messages, Settings, Size, Theme};
use crate::tools::Page;
use crate::tools::prayer::{self, Madhab, Method};

/// Message Windows sends when the tray icon is clicked.
pub const WM_TRAY: u32 = WM_APP + 1;

const ID_VISIBLE: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_SOUND: usize = 3;
const ID_LYRICS: usize = 4;
const ID_LOCK_KEYS: usize = 5;
const ID_BRIGHTNESS: usize = 6;
const ID_HIDE_CONTENT: usize = 7;
const ID_HOTKEY: usize = 8;
const ID_EXIT: usize = 9;
const ID_WIFI: usize = 10;
const ID_DOWNLOADS: usize = 11;
const ID_SCREENSHOTS: usize = 12;
const ID_BROWSER_MESSAGES: usize = 13;
const ID_MEETING: usize = 14;
const ID_WEATHER: usize = 15;
const ID_COMPACT_HUD: usize = 16;
const ID_CALENDAR: usize = 17;
const ID_ACTIVATE: usize = 18;
const ID_LOGS: usize = 19;
const ID_AGENT_NOTICES: usize = 20;
const ID_MAIL: usize = 21;
const ID_GOOGLE_CALENDAR: usize = 22;
const ID_PLUGINS: usize = 23;
const ID_GOOGLE_CONNECT: usize = 24;
const ID_GOOGLE_DISCONNECT: usize = 25;
const ID_PLUGINS_FOLDER: usize = 26;
const ID_FEEDS_FILE: usize = 27;
/// Consecutive choices starting at these ids.
const ID_MESSAGES: usize = 50;
const ID_PAGES: usize = 60;
const ID_SIZE: usize = 70;
const ID_THEME: usize = 100;
const ID_PRAYER_METHOD: usize = 120;
const ID_PRAYER_MADHAB: usize = 130;
const ID_PRAYER_LEAD: usize = 140;
/// Panel right-click menu.
const ID_MOVE_LEFT: usize = 1;
const ID_MOVE_RIGHT: usize = 2;
const ID_HIDE_PAGE: usize = 3;
const ID_SHOW_PAGE: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    ToggleVisible,
    ToggleAutostart,
    ToggleSound,
    ToggleLyrics,
    ToggleLockKeys,
    ToggleBrightness,
    ToggleHideContent,
    ToggleHotkey,
    ToggleWifi,
    ToggleDownloads,
    ToggleScreenshots,
    ToggleBrowserMessages,
    ToggleMeeting,
    ToggleWeather,
    ToggleCompactHud,
    ToggleAgentNotices,
    ToggleCalendar,
    /// Unread mail from the connected Google account.
    ToggleMail,
    /// Events from the connected Google Calendar (and subscribed feeds).
    ToggleGoogleCalendar,
    /// Run the commands in the plugins folder while the Plugins page is open.
    TogglePlugins,
    /// Start the browser sign-in for Google.
    GoogleConnect,
    GoogleDisconnect,
    /// Open the plugins folder.
    PluginsFolder,
    /// Open `calendars.txt`, the list of subscribed calendar feeds.
    FeedsFile,

    /// Prepare the package identity so incoming messages can be read (see `identity`).
    ActivateMessages,
    OpenLogs,
    /// Show/hide a panel page.
    TogglePage(Page),
    Messages(Messages),
    Size(Size),
    Theme(Theme),
    /// Prayer time convention, Asr school, and minutes of warning before each azan.
    PrayerMethod(Method),
    PrayerMadhab(Madhab),
    PrayerLead(u32),
    Exit,
}

/// Choices in the panel right-click menu, for the page currently open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageCommand {
    /// Move left (`-1`) or right (`1`).
    Move(i32),
    Hide,
    Show(Page),
}

pub struct PageMenu {
    pub page: Page,
    pub can_left: bool,
    pub can_right: bool,
    /// Pages currently hidden, offered to show again.
    pub hidden: Vec<Page>,
}

pub struct MenuState {
    pub visible: bool,
    pub autostart: bool,
    pub settings: Settings,
    /// Crest is installed as a package, so message notifications can be read if allowed.
    pub messages_available: bool,
    /// The user denied permission to read notifications.
    pub messages_denied: bool,
    /// The keyboard shortcut failed to register (already used by another app).
    pub hotkey_taken: bool,
    /// Crest runs from the install folder, so incoming messages can be activated.
    pub can_activate: bool,
}

pub struct Tray {
    data: NOTIFYICONDATAW,
}

impl Tray {
    pub fn new(hwnd: HWND, icon: HICON) -> Self {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..Default::default()
        };
        for (dst, src) in data.szTip.iter_mut().zip("Crest".encode_utf16()) {
            *dst = src;
        }
        let tray = Self { data };
        tray.add();
        tray
    }

    /// Called again when Explorer restarts, since the tray icon disappears with it.
    pub fn add(&self) {
        let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &self.data) };
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            let _ = DestroyIcon(self.data.hIcon);
        }
    }
}

fn check(on: bool) -> MENU_ITEM_FLAGS {
    if on { MF_CHECKED } else { MF_UNCHECKED }
}

fn item(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, label: &str) {
    let _ = unsafe { AppendMenuW(menu, MF_STRING | flags, id, &HSTRING::from(label)) };
}

/// Show the menu and wait for the user's choice. The menu runs its own message loop,
/// so do not call this while holding app state.
pub fn show_menu(hwnd: HWND, state: MenuState) -> Option<Command> {
    let s = state.settings;
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let options = CreatePopupMenu().ok()?;
        let messages = CreatePopupMenu().ok()?;
        let themes = CreatePopupMenu().ok()?;
        let pages = CreatePopupMenu().ok()?;
        let sizes = CreatePopupMenu().ok()?;
        let prayers = CreatePopupMenu().ok()?;

        for (i, mode) in Messages::ALL.iter().enumerate() {
            let usable = *mode == Messages::Off || state.messages_available;
            let flags = check(*mode == s.messages) | if usable { MF_UNCHECKED } else { MF_GRAYED };
            item(messages, flags, ID_MESSAGES + i, mode.label());
        }
        if !state.messages_available && state.can_activate {
            item(messages, MF_UNCHECKED, ID_ACTIVATE, "Enable incoming messages… (one-time Administrator prompt)");
        } else if !state.messages_available {
            item(messages, MF_GRAYED, 0, "Install Crest first to use incoming messages");
        } else if state.messages_denied && s.messages != Messages::Off {
            item(messages, MF_GRAYED, 0, "Permission denied: Windows Settings > Privacy > Notifications");
        }
        let _ = AppendMenuW(messages, MF_SEPARATOR, 0, PCWSTR::null());
        let browser_usable = state.messages_available && s.messages != Messages::Off;
        let browser_flags = check(s.browser_messages) | if browser_usable { MF_UNCHECKED } else { MF_GRAYED };
        item(messages, browser_flags, ID_BROWSER_MESSAGES, "Browser notifications (WhatsApp Web, etc.)");
        item(messages, check(s.hide_content), ID_HIDE_CONTENT, "Hide message text");

        item(options, check(s.sound), ID_SOUND, "Notification sounds");
        item(options, check(s.lyrics), ID_LYRICS, "Song lyrics (uses the internet)");
        item(options, check(s.weather), ID_WEATHER, "Weather (uses the internet)");
        item(options, check(s.meeting), ID_MEETING, "Meeting mode (while camera/microphone is in use)");
        let hotkey = if s.hotkey && state.hotkey_taken {
            "Win+Alt+N shortcut (used by another app)"
        } else {
            "Win+Alt+N shortcut"
        };
        item(options, check(s.hotkey), ID_HOTKEY, hotkey);
        let _ = AppendMenuW(options, MF_SEPARATOR, 0, PCWSTR::null());
        item(options, check(s.calendar), ID_CALENDAR, "Calendar on Home");
        item(options, check(s.compact_hud), ID_COMPACT_HUD, "Compact indicators");
        item(options, check(s.lock_keys), ID_LOCK_KEYS, "Caps Lock / Num Lock indicator");
        item(options, check(s.brightness), ID_BRIGHTNESS, "Brightness indicator");
        item(options, check(s.wifi), ID_WIFI, "Wi-Fi indicator");
        item(options, check(s.downloads), ID_DOWNLOADS, "Finished downloads");
        item(options, check(s.screenshots), ID_SCREENSHOTS, "Screenshots to shelf");
        item(options, check(s.agent_notices), ID_AGENT_NOTICES, "Coding agent notices");
        item(options, check(s.mail), ID_MAIL, "Mail (uses the internet)");
        item(options, check(s.google_calendar), ID_GOOGLE_CALENDAR, "Google Calendar events (uses the internet)");
        item(options, check(s.plugins), ID_PLUGINS, "Plugins");
        let google = if crate::feeds::google::connected() { "Google: Disconnect" } else { "Google: Connect…" };
        item(options, MF_UNCHECKED, if crate::feeds::google::connected() { ID_GOOGLE_DISCONNECT } else { ID_GOOGLE_CONNECT }, google);
        item(options, MF_UNCHECKED, ID_PLUGINS_FOLDER, "Open plugins folder");
        item(options, MF_UNCHECKED, ID_FEEDS_FILE, "Calendar feeds (calendars.txt)…");
        // Submenus are destroyed together with their parent menu.
        let _ = AppendMenuW(options, MF_POPUP, messages.0 as usize, w!("Incoming messages"));
        for (i, method) in Method::ALL.iter().enumerate() {
            item(prayers, check(*method == s.prayer_method), ID_PRAYER_METHOD + i, method.title());
        }
        let _ = AppendMenuW(prayers, MF_SEPARATOR, 0, PCWSTR::null());
        for (i, madhab) in Madhab::ALL.iter().enumerate() {
            item(prayers, check(*madhab == s.prayer_madhab), ID_PRAYER_MADHAB + i, &format!("Asr: {}", madhab.label()));
        }
        let _ = AppendMenuW(prayers, MF_SEPARATOR, 0, PCWSTR::null());
        for (i, lead) in prayer::LEADS.iter().enumerate() {
            let label = if *lead == 0 { "Notify at the azan only".to_string() } else { format!("Notify {lead} min before") };
            item(prayers, check(*lead == s.prayer_lead), ID_PRAYER_LEAD + i, &label);
        }
        let _ = AppendMenuW(options, MF_POPUP, prayers.0 as usize, w!("Prayer times"));
        let _ = AppendMenuW(options, MF_SEPARATOR, 0, PCWSTR::null());
        item(options, MF_UNCHECKED, ID_LOGS, "Open log folder");

        for (i, theme) in Theme::ALL.iter().enumerate() {
            item(themes, check(*theme == s.theme), ID_THEME + i, theme.label());
        }
        for (i, size) in Size::ALL.iter().enumerate() {
            item(sizes, check(*size == s.size), ID_SIZE + i, size.label());
        }
        for page in s.pages.order() {
            let index = Page::ALL.iter().position(|p| *p == page).unwrap_or(0);
            let locked = if page == Page::Home { MF_GRAYED } else { MF_UNCHECKED };
            item(pages, check(s.pages.shown(page)) | locked, ID_PAGES + index, page.label());
        }
        let _ = AppendMenuW(pages, MF_SEPARATOR, 0, PCWSTR::null());
        item(pages, MF_GRAYED, 0, "Order: right-click the panel");

        item(menu, check(state.visible), ID_VISIBLE, "Show Crest");
        item(menu, check(state.autostart), ID_AUTOSTART, "Start with Windows");
        let _ = AppendMenuW(menu, MF_POPUP, options.0 as usize, w!("Settings"));
        let _ = AppendMenuW(menu, MF_POPUP, pages.0 as usize, w!("Pages"));
        let _ = AppendMenuW(menu, MF_POPUP, sizes.0 as usize, w!("Size"));
        let _ = AppendMenuW(menu, MF_POPUP, themes.0 as usize, w!("Theme"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        item(menu, MF_UNCHECKED, ID_EXIT, "Quit");

        let chosen = track(hwnd, menu, TPM_BOTTOMALIGN);
        match chosen {
            ID_VISIBLE => Some(Command::ToggleVisible),
            ID_AUTOSTART => Some(Command::ToggleAutostart),
            ID_SOUND => Some(Command::ToggleSound),
            ID_LYRICS => Some(Command::ToggleLyrics),
            ID_LOCK_KEYS => Some(Command::ToggleLockKeys),
            ID_BRIGHTNESS => Some(Command::ToggleBrightness),
            ID_HIDE_CONTENT => Some(Command::ToggleHideContent),
            ID_HOTKEY => Some(Command::ToggleHotkey),
            ID_WIFI => Some(Command::ToggleWifi),
            ID_DOWNLOADS => Some(Command::ToggleDownloads),
            ID_SCREENSHOTS => Some(Command::ToggleScreenshots),
            ID_BROWSER_MESSAGES => Some(Command::ToggleBrowserMessages),
            ID_MEETING => Some(Command::ToggleMeeting),
            ID_WEATHER => Some(Command::ToggleWeather),
            ID_COMPACT_HUD => Some(Command::ToggleCompactHud),
            ID_AGENT_NOTICES => Some(Command::ToggleAgentNotices),
            ID_CALENDAR => Some(Command::ToggleCalendar),
            ID_MAIL => Some(Command::ToggleMail),
            ID_GOOGLE_CALENDAR => Some(Command::ToggleGoogleCalendar),
            ID_PLUGINS => Some(Command::TogglePlugins),
            ID_GOOGLE_CONNECT => Some(Command::GoogleConnect),
            ID_GOOGLE_DISCONNECT => Some(Command::GoogleDisconnect),
            ID_PLUGINS_FOLDER => Some(Command::PluginsFolder),
            ID_FEEDS_FILE => Some(Command::FeedsFile),
            ID_ACTIVATE => Some(Command::ActivateMessages),
            ID_LOGS => Some(Command::OpenLogs),
            ID_EXIT => Some(Command::Exit),
            id if id >= ID_PRAYER_LEAD => prayer::LEADS.get(id - ID_PRAYER_LEAD).map(|m| Command::PrayerLead(*m)),
            id if id >= ID_PRAYER_MADHAB => Madhab::ALL.get(id - ID_PRAYER_MADHAB).map(|m| Command::PrayerMadhab(*m)),
            id if id >= ID_PRAYER_METHOD => Method::ALL.get(id - ID_PRAYER_METHOD).map(|m| Command::PrayerMethod(*m)),
            id if id >= ID_THEME => Theme::ALL.get(id - ID_THEME).map(|t| Command::Theme(*t)),
            id if id >= ID_SIZE => Size::ALL.get(id - ID_SIZE).map(|s| Command::Size(*s)),
            id if id >= ID_PAGES => Page::ALL.get(id - ID_PAGES).map(|p| Command::TogglePage(*p)),
            id if id >= ID_MESSAGES => Messages::ALL.get(id - ID_MESSAGES).map(|m| Command::Messages(*m)),
            _ => None,
        }
    }
}

/// Panel right-click menu: move or hide the page currently open, or show a hidden page
/// again. Like [`show_menu`], do not call this while holding app state.
pub fn show_page_menu(hwnd: HWND, state: &PageMenu) -> Option<PageCommand> {
    let enabled = |on: bool| if on { MF_UNCHECKED } else { MF_GRAYED };
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        item(menu, enabled(state.can_left), ID_MOVE_LEFT, "Move page left");
        item(menu, enabled(state.can_right), ID_MOVE_RIGHT, "Move page right");
        let hide = format!("Hide {} page", state.page.label());
        item(menu, enabled(state.page != Page::Home), ID_HIDE_PAGE, &hide);
        if !state.hidden.is_empty() {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            for (i, page) in state.hidden.iter().enumerate() {
                item(menu, MF_UNCHECKED, ID_SHOW_PAGE + i, &format!("Show again: {}", page.label()));
            }
        }
        match track(hwnd, menu, TRACK_POPUP_MENU_FLAGS(0)) {
            ID_MOVE_LEFT => Some(PageCommand::Move(-1)),
            ID_MOVE_RIGHT => Some(PageCommand::Move(1)),
            ID_HIDE_PAGE => Some(PageCommand::Hide),
            id if id >= ID_SHOW_PAGE => state.hidden.get(id - ID_SHOW_PAGE).map(|p| PageCommand::Show(*p)),
            _ => None,
        }
    }
}

/// Show the menu at the cursor, wait for a choice, then destroy the menu.
unsafe fn track(hwnd: HWND, menu: HMENU, align: TRACK_POPUP_MENU_FLAGS) -> usize {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        // Without this the menu does not close when the user clicks outside it.
        let _ = SetForegroundWindow(hwnd);
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | align;
        let chosen = TrackPopupMenu(menu, flags, cursor.x, cursor.y, None, hwnd, None).0 as usize;
        let _ = DestroyMenu(menu);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        chosen
    }
}
