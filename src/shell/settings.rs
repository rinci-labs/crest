//! Settings changed from the tray menu, stored in `settings.txt` as `key=value`.

use crate::shell::store;
use crate::tools::Pages;
use crate::tools::prayer::{self, Madhab, Method};

const FILE: &str = "settings.txt";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    Ink,
    Slate,
    Ocean,
    Ember,
    /// Desktop Acrylic where supported, with a translucent body as the fallback.
    Frost,
    Paper,
}

impl Theme {
    pub const ALL: [Theme; 6] = [Theme::Ink, Theme::Slate, Theme::Ocean, Theme::Ember, Theme::Frost, Theme::Paper];

    /// Name shown in the menu.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Ink => "Ink",
            Theme::Slate => "Slate",
            Theme::Ocean => "Ocean",
            Theme::Ember => "Ember",
            Theme::Frost => "Blur",
            Theme::Paper => "Paper",
        }
    }

    /// Name in the settings file.
    fn key(self) -> &'static str {
        match self {
            Theme::Ink => "ink",
            Theme::Slate => "slate",
            Theme::Ocean => "ocean",
            Theme::Ember => "ember",
            Theme::Frost => "frost",
            Theme::Paper => "paper",
        }
    }
}

/// Message notifications from chat apps shown in Crest.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Messages {
    Off,
    #[default]
    WhatsApp,
    /// WhatsApp, Telegram, Discord, and other popular chat apps.
    AllChat,
}

impl Messages {
    pub const ALL: [Messages; 3] = [Messages::Off, Messages::WhatsApp, Messages::AllChat];

    pub fn label(self) -> &'static str {
        match self {
            Messages::Off => "Off",
            Messages::WhatsApp => "WhatsApp only",
            Messages::AllChat => "All chat apps",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Messages::Off => "off",
            Messages::WhatsApp => "whatsapp",
            Messages::AllChat => "all",
        }
    }
}

/// Size of the shape, as a multiple of the display scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Small, Size::Medium, Size::Large];

    pub fn label(self) -> &'static str {
        match self {
            Size::Small => "Small",
            Size::Medium => "Medium",
            Size::Large => "Large",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Large => "large",
        }
    }

    /// Multiplier on the display DPI.
    pub fn factor(self) -> f32 {
        match self {
            Size::Small => 0.88,
            Size::Medium => 1.0,
            Size::Large => 1.15,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    /// Sound for important events (reminders, focus, Bluetooth, battery, privacy, messages).
    pub sound: bool,
    /// Fetch lyrics from the internet (LRCLIB). Off by default.
    pub lyrics: bool,
    /// Notify when Caps Lock / Num Lock is toggled.
    pub lock_keys: bool,
    /// Notify when display brightness changes.
    pub brightness: bool,
    /// Notify when Wi-Fi connects or disconnects.
    pub wifi: bool,
    /// Notify when a file in the Downloads folder finishes downloading.
    pub downloads: bool,
    /// New screenshots (Win+Shift+S, PrtScn) go to the shelf automatically.
    pub screenshots: bool,
    /// Win+Alt+N shortcut to open/close the panel.
    pub hotkey: bool,
    /// Meeting mode: while the camera/microphone is in use, message text is hidden and Crest stays silent.
    pub meeting: bool,
    /// Weather from Open-Meteo (uses the internet). Off by default.
    pub weather: bool,
    /// Compact pill-height volume, brightness, Caps Lock, and charger indicators.
    pub compact_hud: bool,
    /// Notice when a coding agent needs input or finishes.
    pub agent_notices: bool,
    /// Wide home page with a calendar and agenda on its right.
    pub calendar: bool,
    /// Also show events from the connected Google Calendar (uses the internet).
    pub google_calendar: bool,
    /// Mail rows on the Mail page (uses the internet).
    pub mail: bool,
    /// Run the commands in the plugins folder while the Plugins page is open.
    pub plugins: bool,
    /// Lyrics column beside the music player (lyrics button in the panel). Only applies when
    /// `lyrics` is on.
    pub lyric_panel: bool,
    pub messages: Messages,
    /// Show only "New message from …", without the message text.
    pub hide_content: bool,
    /// Browser notifications also show as messages (e.g. WhatsApp Web). Off by default,
    /// because notifications from other sites in the same browser show up too.
    pub browser_messages: bool,
    /// Prayer time convention and Asr school (see `tools::prayer`).
    pub prayer_method: Method,
    pub prayer_madhab: Madhab,
    /// Minutes before each azan for the reminder notice; 0 = only at the azan.
    pub prayer_lead: u32,
    pub theme: Theme,
    pub size: Size,
    pub pages: Pages,
    /// The first-run welcome has been dismissed.
    pub welcomed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let mut pages = Pages::default();
        // Mail and Plugins are off by default, so their tabs start off the bar too.
        pages.set_feature(crate::tools::Page::Mail, false);
        pages.set_feature(crate::tools::Page::Plugins, false);
        Self {
            sound: true,
            lyrics: false,
            lock_keys: true,
            brightness: true,
            wifi: true,
            downloads: true,
            screenshots: true,
            hotkey: true,
            meeting: true,
            weather: false,
            compact_hud: true,
            agent_notices: true,
            calendar: true,
            google_calendar: false,
            mail: false,
            plugins: false,
            lyric_panel: false,
            messages: Messages::default(),
            hide_content: false,
            browser_messages: false,
            prayer_method: Method::default(),
            prayer_madhab: Madhab::default(),
            prayer_lead: prayer::DEFAULT_LEAD,
            theme: Theme::default(),
            size: Size::default(),
            pages,
            // An older settings file without this key means Crest has been in use for a while.
            welcomed: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let lines = store::load(FILE);
        let mut settings = Self::parse(&lines);
        // No settings file yet: Crest is running for the first time on this machine.
        if lines.is_empty() {
            settings.welcomed = false;
        }
        settings
    }

    pub fn save(&self) {
        store::save(FILE, self.lines());
    }

    /// Pages that only work with their feature on stay off the tab bar while it is off.
    pub fn sync_pages(&mut self) {
        self.pages.set_feature(crate::tools::Page::Mail, self.mail);
        self.pages.set_feature(crate::tools::Page::Plugins, self.plugins);
    }

    fn parse(lines: &[String]) -> Self {
        let mut settings = Self::default();
        for line in lines {
            let Some((key, value)) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) else { continue };
            let yes = value == "yes";
            match key {
                "sound" => settings.sound = yes,
                "lyrics" => settings.lyrics = yes,
                "lock-keys" => settings.lock_keys = yes,
                "brightness" => settings.brightness = yes,
                "wifi" => settings.wifi = yes,
                "downloads" => settings.downloads = yes,
                "screenshots" => settings.screenshots = yes,
                "hotkey" => settings.hotkey = yes,
                "meeting" => settings.meeting = yes,
                "weather" => settings.weather = yes,
                "compact-hud" => settings.compact_hud = yes,
                "agent-notices" => settings.agent_notices = yes,
                "calendar" => settings.calendar = yes,
                "google-calendar" => settings.google_calendar = yes,
                "mail" => settings.mail = yes,
                "plugins" => settings.plugins = yes,
                "lyric-panel" => settings.lyric_panel = yes,
                "welcomed" => settings.welcomed = yes,
                "pages" => settings.pages = Pages::parse(value),
                "size" => {
                    if let Some(size) = Size::ALL.into_iter().find(|s| s.key() == value) {
                        settings.size = size;
                    }
                }
                "prayer-method" => {
                    if let Some(method) = Method::ALL.into_iter().find(|m| m.key() == value) {
                        settings.prayer_method = method;
                    }
                }
                "prayer-asr" => {
                    if let Some(madhab) = Madhab::ALL.into_iter().find(|m| m.key() == value) {
                        settings.prayer_madhab = madhab;
                    }
                }
                "prayer-lead" => {
                    if let Some(lead) = value.parse().ok().filter(|m| prayer::LEADS.contains(m)) {
                        settings.prayer_lead = lead;
                    }
                }
                "hide-content" => settings.hide_content = yes,
                "browser-messages" => settings.browser_messages = yes,
                "messages" => {
                    if let Some(mode) = Messages::ALL.into_iter().find(|m| m.key() == value) {
                        settings.messages = mode;
                    }
                }
                "theme" => {
                    if let Some(theme) = Theme::ALL.into_iter().find(|t| t.key() == value) {
                        settings.theme = theme;
                    }
                }
                _ => {}
            }
        }
        settings.sync_pages();
        settings
    }

    fn lines(&self) -> Vec<String> {
        let yes = |on: bool| if on { "yes" } else { "no" };
        vec![
            format!("sound={}", yes(self.sound)),
            format!("lyrics={}", yes(self.lyrics)),
            format!("lock-keys={}", yes(self.lock_keys)),
            format!("brightness={}", yes(self.brightness)),
            format!("wifi={}", yes(self.wifi)),
            format!("downloads={}", yes(self.downloads)),
            format!("screenshots={}", yes(self.screenshots)),
            format!("hotkey={}", yes(self.hotkey)),
            format!("meeting={}", yes(self.meeting)),
            format!("weather={}", yes(self.weather)),
            format!("compact-hud={}", yes(self.compact_hud)),
            format!("agent-notices={}", yes(self.agent_notices)),
            format!("calendar={}", yes(self.calendar)),
            format!("google-calendar={}", yes(self.google_calendar)),
            format!("mail={}", yes(self.mail)),
            format!("plugins={}", yes(self.plugins)),
            format!("lyric-panel={}", yes(self.lyric_panel)),
            format!("messages={}", self.messages.key()),
            format!("hide-content={}", yes(self.hide_content)),
            format!("browser-messages={}", yes(self.browser_messages)),
            format!("prayer-method={}", self.prayer_method.key()),
            format!("prayer-asr={}", self.prayer_madhab.key()),
            format!("prayer-lead={}", self.prayer_lead),
            format!("theme={}", self.theme.key()),
            format!("size={}", self.size.key()),
            format!("pages={}", self.pages.to_text()),
            format!("welcomed={}", yes(self.welcomed)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let mut pages = Pages::default();
        pages.set_shown(crate::tools::Page::Water, false);
        let settings = Settings {
            sound: false,
            lyrics: true,
            lock_keys: false,
            brightness: true,
            wifi: false,
            downloads: true,
            screenshots: false,
            hotkey: false,
            meeting: false,
            weather: true,
            compact_hud: false,
            agent_notices: false,
            calendar: false,
            google_calendar: true,
            mail: true,
            plugins: true,
            lyric_panel: true,
            messages: Messages::AllChat,
            hide_content: true,
            browser_messages: true,
            prayer_method: Method::Makkah,
            prayer_madhab: Madhab::Hanafi,
            prayer_lead: 15,
            theme: Theme::Frost,
            size: Size::Large,
            pages,
            welcomed: false,
        };
        let mut settings = settings;
        settings.sync_pages();
        assert_eq!(Settings::parse(&settings.lines()), settings);
        let messy = vec!["theme = missing".to_string(), "garbage".to_string()];
        assert_eq!(Settings::parse(&messy), Settings::default());
        // A partial file keeps defaults for the missing keys.
        let partial = vec!["lyrics=yes".to_string(), "theme=ink".to_string()];
        let parsed = Settings::parse(&partial);
        assert!(parsed.lyrics && parsed.sound);
        // A lead that is not one of the offered choices keeps the default.
        let odd = Settings::parse(&["prayer-lead=7".to_string()]);
        assert_eq!(odd.prayer_lead, prayer::DEFAULT_LEAD);
    }
}
