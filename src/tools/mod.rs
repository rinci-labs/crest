//! Widgets in the Crest panel. These modules only hold data and its rules (no drawing and no
//! Windows API), so they are easy to test.

pub mod agenda;
pub mod calfeed;
pub mod feeds;
pub mod focus;
pub mod prayer;
pub mod shelf;
pub mod tasks;
pub mod timer;
pub mod water;

/// Panel page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    /// Clock, or the music player when media is present.
    Home,
    /// Volume, brightness, microphone, mute, and weather city.
    Controls,
    Shelf,
    Focus,
    Timer,
    Water,
    /// Today's prayer times and the next azan.
    Prayer,
    Tasks,
    /// System monitor and privacy status.
    System,
    /// Running coding-agent sessions (omp, Claude Code, Cursor).
    Agents,
    /// Unread mail from Gmail, when a Google account is connected.
    Mail,
    /// Rows produced by local plugin commands.
    Plugins,
    /// Theme, size, and feature switches. Opened from the gear at the end of the tab bar; not
    /// in [`Page::ALL`], so it cannot be hidden or moved and is never stepped through.
    Settings,
}

impl Page {
    pub const ALL: [Page; 12] = [
        Page::Home,
        Page::Controls,
        Page::Shelf,
        Page::Focus,
        Page::Timer,
        Page::Water,
        Page::Prayer,
        Page::Tasks,
        Page::System,
        Page::Agents,
        Page::Mail,
        Page::Plugins,
    ];

    /// Name shown in menus.
    pub fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Controls => "Controls",
            Page::Shelf => "Shelf",
            Page::Focus => "Focus",
            Page::Timer => "Timer",
            Page::Water => "Water",
            Page::Prayer => "Prayer times",
            Page::Tasks => "Tasks",
            Page::System => "System",
            Page::Agents => "Agents",
            Page::Mail => "Mail",
            Page::Plugins => "Plugins",
            Page::Settings => "Settings",
        }
    }

    /// Name in the settings file.
    fn key(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::Controls => "controls",
            Page::Shelf => "shelf",
            Page::Focus => "focus",
            Page::Timer => "timer",
            Page::Water => "water",
            Page::Prayer => "prayer",
            Page::Tasks => "tasks",
            Page::System => "system",
            Page::Agents => "agents",
            Page::Mail => "mail",
            Page::Plugins => "plugins",
            Page::Settings => "settings",
        }
    }
}

/// Page order and which pages are hidden. Home is always shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pages {
    order: [Page; 12],
    /// One bit per page, in [`Page::ALL`] order.
    hidden: u16,
    /// Pages whose feature is switched off (Mail, Plugins): kept off the tab bar so it only
    /// carries pages that do something, but still "shown" in the Pages menu. Not saved; it
    /// follows the feature switches (see `Settings::sync_pages`).
    off: u16,
}

impl Default for Pages {
    fn default() -> Self {
        Self { order: Page::ALL, hidden: 0, off: 0 }
    }
}

impl Pages {
    /// All pages in the user's order, including hidden ones.
    pub fn order(&self) -> [Page; 12] {
        self.order
    }

    pub fn shown(&self, page: Page) -> bool {
        page == Page::Settings || self.hidden & bit(page) == 0
    }

    /// Pages on the tab bar, in order: shown, and their feature is on. Timer shares Focus's
    /// tab while both are shown (see [`Pages::tab`]).
    pub fn visible(&self) -> impl Iterator<Item = Page> + '_ {
        self.order.iter().copied().filter(|p| self.on_bar(*p) && self.tab(*p) == *p)
    }

    fn on_bar(&self, page: Page) -> bool {
        self.shown(page) && self.off & bit(page) == 0
    }

    /// The tab that carries `page`. Focus and Timer are one tab: a timer either way, switched
    /// inside the page. A page with its own tab is its own tab.
    pub fn tab(&self, page: Page) -> Page {
        if page == Page::Timer && self.on_bar(Page::Focus) { Page::Focus } else { page }
    }

    /// Keep a page off the tab bar while its feature is switched off.
    pub fn set_feature(&mut self, page: Page, on: bool) {
        if on {
            self.off &= !bit(page);
        } else {
            self.off |= bit(page);
        }
    }

    /// Position of the page among the shown ones (for the indicator dots and swipe direction).
    pub fn index(&self, page: Page) -> usize {
        let tab = self.tab(page);
        self.visible().position(|p| p == tab).unwrap_or(0)
    }

    /// Next (`step` = 1) or previous (`step` = -1) shown page, wrapping at the ends.
    pub fn step(&self, from: Page, step: i32) -> Page {
        let visible: Vec<Page> = self.visible().collect();
        let from = self.tab(from);
        let Some(at) = visible.iter().position(|p| *p == from) else { return Page::Home };
        visible[(at as i32 + step).rem_euclid(visible.len() as i32) as usize]
    }

    /// Show or hide; Home and Settings cannot be hidden.
    pub fn set_shown(&mut self, page: Page, shown: bool) {
        if matches!(page, Page::Home | Page::Settings) {
            return;
        }
        if shown {
            self.hidden &= !bit(page);
        } else {
            self.hidden |= bit(page);
        }
    }

    /// Can the page move left (`-1`) or right (`1`) among the shown pages?
    pub fn can_move(&self, page: Page, delta: i32) -> bool {
        let i = self.index(page) as i32 + delta;
        page != Page::Settings && self.shown(page) && i >= 0 && i < self.visible().count() as i32
    }

    /// Swap places with the neighbouring shown page.
    pub fn move_by(&mut self, page: Page, delta: i32) {
        if !self.can_move(page, delta) {
            return;
        }
        let neighbour = self.step(page, delta);
        let a = self.order.iter().position(|p| *p == page).unwrap_or(0);
        let b = self.order.iter().position(|p| *p == neighbour).unwrap_or(0);
        self.order.swap(a, b);
    }

    /// "home,shelf,-water,…": the order, with a "-" prefix marking hidden pages.
    pub fn to_text(self) -> String {
        let names: Vec<String> =
            self.order.iter().map(|p| format!("{}{}", if self.shown(*p) { "" } else { "-" }, p.key())).collect();
        names.join(",")
    }

    /// Inverse of [`Pages::to_text`]. Unknown names are skipped; pages not mentioned are
    /// appended at the end (e.g. pages added in a later version).
    pub fn parse(text: &str) -> Self {
        let mut order = Vec::new();
        let mut pages = Self { order: Page::ALL, hidden: 0, off: 0 };
        for name in text.split(',').map(str::trim) {
            let (hidden, key) = match name.strip_prefix('-') {
                Some(key) => (true, key),
                None => (false, name),
            };
            if let Some(page) = Page::ALL.into_iter().find(|p| p.key() == key)
                && !order.contains(&page)
            {
                order.push(page);
                pages.set_shown(page, !hidden);
            }
        }
        for page in Page::ALL {
            if !order.contains(&page) {
                order.push(page);
            }
        }
        pages.order.copy_from_slice(&order);
        pages
    }
}

fn bit(page: Page) -> u16 {
    1u16 << Page::ALL.iter().position(|p| *p == page).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_wrap_around() {
        let pages = Pages::default();
        assert_eq!(pages.step(Page::Home, -1), Page::Plugins);
        assert_eq!(pages.step(Page::Plugins, 1), Page::Home);
        assert_eq!(pages.step(Page::Shelf, 1), Page::Focus);
    }

    #[test]
    fn hidden_pages_are_skipped_and_order_round_trips() {
        let mut pages = Pages::default();
        pages.set_shown(Page::Water, false);
        pages.set_shown(Page::Home, false);
        assert!(pages.shown(Page::Home), "home is always shown");
        assert_eq!(pages.step(Page::Timer, 1), Page::Prayer, "Timer steps on from its shared tab");
        assert_eq!(pages.tab(Page::Timer), Page::Focus);
        assert_eq!(pages.visible().count(), 10);

        pages.move_by(Page::System, -1);
        assert_eq!(
            pages.visible().collect::<Vec<_>>(),
            [
                Page::Home,
                Page::Controls,
                Page::Shelf,
                Page::Focus,
                Page::Prayer,
                Page::System,
                Page::Tasks,
                Page::Agents,
                Page::Mail,
                Page::Plugins,
            ]
        );
        assert!(!pages.can_move(Page::Home, -1));
        assert!(!pages.can_move(Page::Plugins, 1));
        assert!(pages.can_move(Page::Tasks, 1), "tasks still has a neighbour on the right");

        assert_eq!(Pages::parse(&pages.to_text()), pages);
        assert_eq!(pages.to_text(), "home,controls,shelf,focus,timer,-water,prayer,system,tasks,agents,mail,plugins");
        // Unknown names are skipped; pages not mentioned are appended at the end.
        let partial = Pages::parse("tasks, junk, -shelf");
        assert_eq!(partial.order()[0], Page::Tasks);
        assert!(!partial.shown(Page::Shelf));
        assert_eq!(partial.visible().count(), 10);
        // Older settings (from before newer pages existed) still parse; new pages show at the end.
        let old = Pages::parse("home,shelf,focus,water,tasks,system");
        assert_eq!(old.order()[6..], [Page::Controls, Page::Timer, Page::Prayer, Page::Agents, Page::Mail, Page::Plugins]);
        assert!(old.shown(Page::Timer));
        // With Focus hidden, Timer gets a tab of its own again.
        let mut alone = Pages::default();
        alone.set_shown(Page::Focus, false);
        assert!(alone.visible().any(|p| p == Page::Timer));
        assert!(old.shown(Page::Mail), "new pages show up for older settings");

        // A page whose feature is off leaves the tab bar but stays shown in the Pages menu.
        let mut gated = Pages::default();
        gated.set_feature(Page::Mail, false);
        assert!(gated.shown(Page::Mail) && !gated.visible().any(|p| p == Page::Mail));
        assert_eq!(gated.step(Page::Agents, 1), Page::Plugins, "stepping skips it");
        assert_eq!(gated.to_text(), Pages::default().to_text(), "not saved");
    }
}
