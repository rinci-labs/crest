# Changelog

All notable changes to Crest. The format follows [Keep a Changelog](https://keepachangelog.com),
and versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Changed

- Focus and Timer share one tab with a Focus | Timer switch; the tab bar is down to nine pages
  and the gear on a fresh install.
- Every notice that is not a reaction to your own key press now has a sound: screenshots saved
  to the shelf and Wi-Fi losing internet joined the rest (messages stay silent, the chat app
  already plays its own). Clicking the timer notice stops its alarm.
- One spacing system for every page: a 192 DIP panel (240 on Settings), the deck 10 DIP inside
  the island with a concentric 18 DIP corner (island 28), 16 DIP of padding inside it, and the
  bottom row 16 DIP above the deck's edge. Nothing is squeezed against an edge any more.
- Hover highlights ease in (about 150 ms) on rows, cards, buttons, segments, and tabs.
- The tab bar only carries pages that do something: Mail and Plugins join it when their switch
  is on (both are off by default). The capsule keeps an even 2 DIP around its keys.
- Controls page in control-centre style: full-width capsule sliders with the icon inside
  (brightness only on displays that have it) and equal toggle tiles for Microphone, Quiet, and
  the weather city, each showing its state.
- Home's status beside the tabs fits the room the tabs leave: the weather shortens to
  "26° · City", then "26°", instead of running under the tabs. The battery opens System and the
  weather opens Controls.
- Lyrics column: three evenly spaced lines centred in their card; the lines around the current
  one stay on one line.
- Agents, Mail, and Plugins share one list row (38 DIP, two lines, 6 DIP inside the deck) and one
  footer row with the page's action and "+N more". Rows only fill on hover.

### Fixed

- Mail rows no longer overlap, and only the row under the cursor lights up.
- Controls keeps even gaps without a brightness slider: sliders and tiles are one centred block.
- Empty pages are one centred block (icon on a soft disc, title, hint, then the action), and
  Plugins no longer repeats its folder link in the caption.
- The column divider no longer runs into the lyrics card; it fades out as the lyrics slide in.
- Lyrics card: the same 12 DIP of padding on every side (the text used to run into its right
  edge), and the sung line (up to two lines) with the next one below instead of three cramped
  lines; the accent bar is gone and "Searching for lyrics…" sits centred in the card.
- Every page's content now starts the same 16 DIP inside the page on all four sides (Home,
  player, Shelf, Focus, Timer, Water, Tasks, System, lists, Settings, Mail setup); cards and tiles
  share one corner radius.
- Google setup on the Mail page fits inside the page: Connect moved to the title row, one hint
  line, two full-width boxes.
- Focus caption ("2 today") is no longer cut off; the welcome no longer says the tabs are at the
  bottom.
- Empty states (Tasks, Agents, Mail, Plugins, Prayer) centre their title and hint under the icon.
- Home's greeting no longer sits on the top edge of the page deck.
- The Water caption ("Drink at 19:37") fits beside the tab row instead of being cut off.

### Added

- Prayer page: today's Fajr, Sunrise, Dhuhr, Asr, Maghrib, and Isha for the saved city, the
  next azan with a countdown, and a notice 5, 10, 15, or 30 minutes before each azan (or only at
  the azan) plus one when it is time. Times are calculated on this PC (Kemenag, MWL, ISNA,
  Egypt, Umm al-Qura, Karachi; Standard or Hanafi Asr), with no timetable download. Home shows
  the next prayer as a glance chip. Tray menu: Settings → Prayer times.
- Settings opens on a taller panel, with a Prayer row (method and Asr school).
- Settings page in the panel (gear at the end of the tab bar): theme swatches, size, and the
  most used feature switches. Switches that use the internet are marked with a globe.
- At-a-glance chips under the clock on Home (tasks left, water, agents that need you); each opens
  its page.
- Agents page: click a row to switch to the agent's terminal or editor window; the mouse wheel
  scrolls through every session.
- Clicking an agent notice switches to that agent's window when it can be found.
- New mail arrives as a notice while Crest runs: the sender, the subject, and "+N" for others
  that came with it; clicking it opens Gmail. The first read after start is silent, so unread
  messages that predate Crest are not announced.
- Mail page: unread Gmail subjects and senders, read-only (`gmail.readonly`). Each row has the
  sender'''s initials on an avatar plate, a fixed age column, and an open-envelope action on hover;
  with nothing unread the page shows "You'''re all caught up`. Connect with your
  own Google Cloud OAuth desktop client: the sign-in uses PKCE and a loopback redirect on
  `127.0.0.1`, and the refresh token is kept DPAPI-encrypted for this Windows user. Off by
  default; refreshed every 5 minutes while the page is open.
- Google Calendar events on Home's calendar: the primary calendar's next two weeks, read-only.
  Tray menu: Google: Connect / Disconnect.
- Calendar feeds: iCal subscriptions in `calendars.txt`, one `name|https://…` line each, read
  from Google Calendar's secret address, Outlook, iCloud, or Fastmail. Fetched at most every
  30 minutes; `webcal://` is accepted, plain `http` is rejected, and the address is never logged.
  Feed rows show the feed name with the time (`09.00 · Work`) and turn green with a dot while the
  event is running.
- Plugins page: one `<name>.plugin` file per plugin in `%APPDATA%\Crest\plugins` with `command`
  and `every = <minutes>`. The command prints `title | detail | url` rows, and a row with a URL
  opens in the browser. Off by default; runs only while the page is open, with a 20 s timeout and
  no console window. An `example.plugin` is written when the feature is first turned on.
- Tray menu: Mail, Google Calendar events, Plugins, Google: Connect / Disconnect, Open plugins
  folder, and Calendar feeds (`calendars.txt`).
- The Mail page explains a refused sign-in instead of waiting: Cancel stops a pending connect,
  and a rejected token exchange shows Google's own reason (`invalid_grant`,
  `redirect_uri_mismatch`, `invalid_client`, …) rather than "HTTP 400". The client secret is
  required, since a Desktop app client rejects the exchange without it.
- Agent hooks, installable per provider from the Settings page: display-only lifecycle relays for
  Claude Code, Cursor, and Copilot that merge into the existing configuration and never replace
  other hooks. The Agents page shows "Hooks on" once every relay is in place.
- Any other coding tool can report to the Agents page too: `crest.exe --agent-hook <tool>
  [event]`, or a payload naming the tool with `crest_agent` (Coucou's `coucou_agent` works as
  well). Vendor event names for Gemini CLI, Antigravity, Copilot, OpenCode, and Hermes are
  translated, and permission requests are answered "ask" so the tool re-prompts in its own
  terminal instead of waiting on Crest.

### Fixed

- The lyrics column is a clean rounded card. Its halo was drawn per line and cut by a rectangular
  clip, which sliced the card's corners into straight lines; the card is now drawn once as a
  rounded shape, and its top edge sits inside the page deck so the deck's own straight line no
  longer crosses it. Lines fade towards the edge by opacity instead of being cut.
- Clicking the lyrics button now resizes the panel at once. It changed the setting but never asked
  for a redraw, so the card could stay collapsed until some later event happened to refresh it.

- A new song no longer keeps the previous song'''s elapsed time and progress bar. Windows can
  publish the new title a beat before the new timeline, so the position still belonged to the
  track that had just ended; a title change now starts at zero. The seek preview, the current
  lyric line, its scroll position, and the old lyrics are cleared at the same moment, so the
  player and the lyrics column change together instead of showing the previous song.
- The tab bar keeps the same width on every page. It used to reserve space for the current
  page'''s name and for that page'''s caption, so the row widened and narrowed as the user moved
  between pages; the reserve is now the widest label in the row and the caption reserve scales
  with the panel. A name is shown only where it genuinely fits beside the caption.

### Changed

- The panel's control set now follows rust-ui's (shadcn's) component vocabulary: one `chip`
  button helper with Primary / OnBlue / Secondary / Outline / Ghost / Link variants replacing
  twelve hand-rolled fill-and-label combinations, one shared `text_field` for every input, and a
  `Palette` named after the semantic tokens (`--background`, `--foreground`,
  `--muted-foreground`, `--primary`, `--primary-foreground`, `--input`, `--border`). Only the
  design language is borrowed: rust-ui is a Leptos/WASM library and cannot run in a Win32
  layered window. The port covers Button, Input, Empty, Badge, Alert, Row, Card, Icon button,
  Separator, and Progress.
- Theme bodies follow the zinc/stone scales (Ink `#09090B`, Slate `#18181B`, Ocean `#042F2E`,
  Ember `#0C0A09`, Paper `#FAFAFA`).
- Labels on accent and status fills use ink wherever that measures better: white on the pastel
  primaries was only 1.7–3.3:1, ink is 6.1–11.9:1. A test checks every theme against WCAG AA.
- Larger type scale (11–17 DIP instead of 10–16).
- Tabs moved to a row at the top of the panel, with the page's caption (and Home's weather,
  time, battery, and music wave) on the right; pages lost their title row and use the space
  below. The current tab shows its name when there is room.
- Agents page is calmer: two roomy rows with title and status, then agent and project (plus the
  waiting reason when a session needs you); the rest are summed up as "+N more".
- Focus phase and Settings size use one shared segmented control.
- Focus and Timer center their clock on the progress ring; the main element of every page sits
  on one line (`HERO_Y`).
- Volume and brightness sliders use the theme accent; Quick controls now has a speaker icon in
  the tab bar (the gear belongs to Settings).
- Page list is Home, Controls, Shelf, Focus, Timer, Water, Tasks, System, Agents, Mail, Plugins
  (Settings stays outside it), and the Settings page now shows six switches: Lyrics, Weather,
  Mail, Calendar, Google Cal, and Plugins. The other options moved to the tray menu.
- Home's calendar merges typed agenda entries, subscribed feeds, and Google events. Typed rows
  stay on top; feed and Google rows are sorted by start time, all-day first. Feed rows are
  read-only and cannot be ticked off.

## [0.1.0] - 2026-10-22

First release.

### Added

- Tab at the top center of the screen that opens into a panel on hover, with spring motion,
  click-through outside the shape, a thin line over maximized windows, and auto-hide in full
  screen.
- Music player for any Windows media session: cover, drag-to-seek progress, playback buttons,
  cover-tinted glow, and optional synced lyrics from LRCLIB in the pill and a sliding column.
- Notices for volume, brightness, power, Bluetooth (with battery), Wi-Fi, Caps/Num Lock, finished
  downloads, new screenshots, camera/microphone use, timers, agenda items, and incoming chat
  messages (optional, via a locally signed identity package).
- Pages: Home (clock or music, calendar and agenda), Quick controls, Shelf with a Share zone,
  Focus, Timer, Water, Tasks, System monitor, and Agents (running Claude Code, Cursor, and omp
  sessions read from local files).
- Optional weather from Open-Meteo, meeting mode, quiet hour, Win+Alt+N shortcut, and touchpad
  gestures.
- Six themes (Ink, Slate, Ocean, Ember, Frost, Paper), three sizes, and a mascot with moods.
- Per-user install and uninstall without administrator rights, Start menu entry, and start with
  Windows.
