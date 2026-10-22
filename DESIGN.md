# Design

UI design and architecture of Crest. Working rules for contributors and AI agents are in
[AGENTS.md](AGENTS.md).

## Principles

- **A hanging island.** An ink-colored island hangs from the top center: straight sides, circular
  bottom corners, edges on whole pixels, and a quiet inset content deck below its floating
  navigation rail. Clicks outside the shape pass through to the window below.
- **Quiet color.** Neutral zinc text scale on a tinted body; one accent per theme, used for
  selection, primary buttons, and the mascot's highlights.
- **Stays out of the way.** Shrinks to a thin line when a window is maximized, hides in full
  screen, and opens only after the cursor rests briefly (200 ms).
- **Light and local.** One process, no heavy polling; data sources report through Windows events
  where available.
- **Private by default.** No account and no telemetry; features that use the internet stay off
  until the user turns them on.

## Architecture

```
main.rs ──► app::run()
             │
             ├─ layered Win32 window (WS_EX_LAYERED | TOPMOST | NOACTIVATE)
             ├─ Win32 timers (clock, open/close, notifications, music waveform, light polling)
             ├─ services / notification sources ──► Event ──► app state
             │
             └─ every redraw: state ──► ui::Scene ──► Renderer (Direct2D)
                                                 │
                                                 └─ hit-test ──► ui::Hit ──► app/input.rs
```

- **`app`** owns all state and the message loop; `impl App` is split into one file per concern.
  There is no second UI thread.
- **`ui::Scene`** is a snapshot of state for one frame; the renderer does not change state.
- **`ui::Hit`** is the hit-test result; `app/input.rs` decides what to do with it.
- **Tools** (`src/tools/`) hold page logic and data, separate from drawing.
- **Feeds** (`src/feeds/`) and **system** (`src/system/`) report through the service thread.

## Shape and motion

- Open: a slightly bouncy spring (`OPEN_BOUNCE = 0.72`). Close: smooth with no bounce
  (`CLOSE_BOUNCE = 1.0`).
- Open delay 200 ms, close delay 380 ms; after a file drop, the panel stays open for 2.5 seconds.
- On hover, the pill immediately grows a little (12 × 4 DIP) before the panel opens.
- Page change: content slides 28 DIP (`tokens::PAGE_SLIDE`) over 0.24 seconds.
- Navigation is a tab row at the top of the panel: tabs on the left, the page's caption on the
  right (e.g. "3 left", "Drink at 19:37"; on Home the weather, time while music plays,
  battery, and music wave). One 13 DIP icon per visible page in a 24 DIP slot (`TAB_SLOT`), the
  Settings gear after a thin divider. The current page sits on an accent-tinted key (`TAB_H`
  tall) that also shows the page name when the row has room beside the caption; a hovered icon
  gets a soft key. Pages have no title row of their own. Mouse wheel and two-finger swipe still
  step through pages (Settings is not stepped through). The open panel is 440 × 192 DIP (660 wide
  with the calendar or lyrics, 240 tall on Settings). The page deck keeps its 16 DIP gap to the
  bottom edge on the taller panel and never gets shorter than on a normal page.
- Page content is drawn `CONTENT_SHIFT` (14 DIP) below its own coordinates, under the tab row;
  the renderer applies the shift to drawing and click areas alike.
- Bottom corner radius grows from 12 to 28 DIP (`RADIUS_ISLAND`) as the tab opens.
- Spacing system: the deck sits 10 DIP (`DECK_INSET`) inside the island's sides and bottom, so
  its 18 DIP corner is concentric with the island's 28; content keeps 16 DIP of padding inside
  the deck (`INSET` 26 from the island edge); every page's content starts 16 DIP (`PAD`) inside the deck on all four
  sides (checked against the rendered previews); hover plates may bleed 6 DIP into it. Cards and
  tiles share one 10 DIP corner (`RADIUS_ROW`). The lyrics card is the column's content block on
  that padding, 12 DIP of padding inside it (`LYRIC_PAD`), and shows the sung line (up to two
  lines) with the next one below, as Apple Music does; the previous line fades out above. Hover highlights ease in over about 150 ms (`HOVER_TAU`).
- Notification numbers roll to the new value in 0.22 seconds.
- A shape animation counts as finished when the size difference is below 0.05 DIP.

Apart from `PAGE_SLIDE` and the corner radius, these values live in `src/app/config.rs`.

## Design tokens

Single source: `src/ui/tokens.rs`. Horizontal spacing and radii follow a 4 DIP grid; type sizes
are whole DIP so text lands on the pixel grid. Pages must not use new literals for spacing, radii, type
sizes, or opacity.

### Spacing and layout (DIP)

| Token | Value | Used for |
|---|---|---|
| `INSET` | 26 | Content inset from the panel's left/right edges (deck inset + 16) |
| `GUTTER` | 20 | Gap between columns on Home |
| `TIGHT` | 10 | Gap between adjacent elements |
| `TABS_Y` | 20 | Center line of the tab row (tabs and page caption) |
| `CONTENT_SHIFT` | 14 | Page content offset below the tab row |
| `HEADER_Y` | 22 | Center line of the welcome title (no tab row there) |
| `HERO_Y` | 80 | Center line of a page's main element (Focus/Timer ring and clock, Water ring, Home mascot), before the shift |
| `ROW_Y` | 140 | Button row at the bottom of a page, before the shift (16 DIP above the deck's bottom) |
| `TAB_SLOT` / `TAB_H` | 24 / 18 | Tab-bar icon slot and the key under the current/hovered icon |
| `ROW_H` | 24 | Task list rows and agent rows |
| `AGENDA_ROW_H` | 16 | Agenda rows in the calendar |
| `CONTROL_H` | 24 | Text fields and capsule buttons |
| `TILE_H` / `TILE_PITCH` | 80 / 76 | Shelf tiles |

### Radii (DIP)

| Token | Value | Used for |
|---|---|---|
| `RADIUS_KEY` | 4 | Small buttons, checkboxes |
| `RADIUS_ROW` | 10 | List rows, tiles, text fields |
| `RADIUS_CARD` | 14 | Large cards (file drop area, notification panel) |
| `RADIUS_DECK` | 18 | Page deck: `RADIUS_ISLAND` (28) minus `DECK_INSET` (10), concentric |

### Fills (theme color opacity)

| Token | Value | Used for |
|---|---|---|
| `FILL_SUBTLE` | 0.08 | Default button background |
| `FILL_GHOST` | 0.045 | Resting page deck and nested row surfaces |
| `FILL_SOFT` | 0.12 | Hovered button background |
| `FILL_STRONG` | 0.16 | Active selection |
| `TRACK` | 0.22 | Progress bar and ring tracks |
| `DIVIDER` | 0.12 | Divider between columns |
| `HAIRLINE` | 0.10 | Page-deck and outer-island edge |

### Type (DIP)

A small scale with nothing under 11 DIP: `TYPE_TINY` 11 · `TYPE_CAPTION` 12 · `TYPE_BODY` 13 ·
`TYPE_LABEL` 14 · `TYPE_TITLE` 17.

### Color

| Theme | Body | Accent |
|---|---|---|
| Ink (default) | `#0B0B0F` | `#8B7CFF` iris |
| Slate | `#1A1D23` | `#7DD3FC` sky |
| Ocean | `#071A24` | `#2DD4BF` teal |
| Ember | `#1C1210` | `#FB923C` orange |
| Blur (`frost` in settings files) | translucent `#18181F`; rendered inside Crest so the transparent window bounds stay invisible | `#A99DFF` |
| Paper | `#FAFAF7` | `#6D5DF6` |

Status colors are shared: green `#34D399`, amber `#FBBF24`, red `#F87171`, blue `#60A5FA`. Text
on a filled accent button uses the body color; text on blue uses ink `#0B0B0F`.

### Controls

The control set follows rust-ui's (shadcn's) component vocabulary — the same names, states, and
semantic tokens as the Leptos library's documentation, implemented against Direct2D. Only the
design language is shared; rust-ui itself is a Leptos/WASM library and cannot run in a Win32
layered window.

- **Button** (`Renderer::chip`, `Style`): `Primary`, `OnBlue`, `Secondary`, `Ghost`, and `Link`,
  mirroring rust-ui's default/secondary/ghost/link variants. Capsule height
  `CONTROL_H`; hovered = `pressed`. One helper decides fill, hairline, and label colour, so no
  page picks those by hand.
- **Input** (`Renderer::text_field`): `--input` fill and a `--border` hairline that brightens on
  hover; the focused state is `typing_box`, which adds a `--ring` outline and keeps the caret in
  view. Used for tasks, agenda items, the weather city, the Google client id/secret, and the
  plugin and feed fields.
- **Empty / Badge / Alert** (`Renderer::empty_state`, `badge`, `alert`): rust-ui's `Empty`,
  `Badge`, and `Alert`. An empty state is a glyph, a title, and a hint centred in a box, used by
  every page that can have no content (Tasks, Agents, Plugins) instead of four differently
  placed strings. A badge is a compact count on a secondary surface, or on a status colour when
  it carries meaning (agents waiting). An alert is a tinted plate with a coloured hairline, used
  where a message explains the page (a refused Google sign-in, a failed plugin).
- **Progress** is `bar_sized` (see below), the one track-and-fill shape every bar uses.
- **List cell** (`Renderer::list_cell`, `Cell`): the two-line row of every list page (Agents,
  Mail, Plugins), as in an iOS list: a lead area for a dot or avatar, the title with trailing
  text, and the subtitle. 38 DIP tall (`LIST_ROW_H`), two per page (`LIST_ROWS`) from
  `LIST_TOP` (6 DIP inside the deck), then a footer at `ROW_Y` with the page action and "+N more".
  Empty pages centre their state in `EMPTY_AREA`, between the deck top and the footer.
- **Capsule slider** (`Renderer::capsule_slider`): the Controls page's control-centre slider,
  26 DIP tall, filled from the left, icon inside the left end (mute button for volume), value at
  the right end in the fill's ink once the fill reaches it.
- **Row / Card / Icon button / Separator** (`Renderer::list_row`, `card`, `icon_button`,
  `separator`): the remaining surface primitives. `list_row` is a row with no resting fill (it
  already sits on the deck) that fills softly on hover; `card` is a quiet filled plate that brightens on hover; `icon_button`
  is the circular ghost button used for a page's single affordance; `separator` draws the
  hairline between columns and before the Settings gear. Lists no longer choose their own fills.
- **Segmented control** (`Renderer::segmented`): one capsule track, the current choice on the
  primary fill. Used for the Focus phase and the Settings size.
- **Switch**: 20 × 12 DIP track, primary fill when on; on Settings it sits at the right of a
  `RADIUS_ROW` row button.

### Color roles

`Palette` uses the shadcn/rust-ui semantic names: `body` (`--background`), `text`
(`--foreground`), `soft`/`muted` (`--muted-foreground`), `badge` (`--secondary`), `placeholder`
(`--input`), `accent` (`--primary`), and `on_accent` (`--primary-foreground`).

Themes are zinc/stone scales: Ink `#09090B` (zinc-950), Slate `#18181B` (zinc-900), Ocean
`#042F2E` (teal-950), Ember `#0C0A09` (stone-950), Paper `#FAFAFA` (zinc-50).

**Contrast rule.** Every dark theme's primary is a light pastel, so `on_accent` is ink there
(6.1:1 on Ink, 8.5–11.9:1 elsewhere) and white only on Paper's darker violet (4.6:1). White on
the pastels would be 1.7–3.3:1, which fails WCAG AA. Status colors (green, amber, red, blue) are
light in every theme, so they always take ink (7.2–11.9:1). `ui::tests::labels_stay_readable_on_every_fill`
pins both rules across all six themes.

### Mascot and icon

Two oval eyes (`EyeSize::PILL`, `HEADER`, `CLOCK`). The app icon is an iris rounded tile with the
same eyes, drawn in code and snapped to whole pixels at every size (16–150 px).

### Home columns

- Left column with the calendar: `home_left(inner)` = 54.5% of the content width, clamped to
  264–380 DIP and at most 70% of the content width.
- Player column while lyrics are shown: `lyric_left(inner)` = 46% of the content width, clamped to
  236–320 DIP and at most 60% of the content width.
- The widest shape is at most 94% of the monitor width (`MAX_SCREEN_SHARE`).

Column limits are covered by tests in `tokens.rs`.

### Home status

The right end of Home's tab row holds, in order of importance from the right: the music wave,
the time while music plays, the battery, and the weather. They are laid out right to left within
what the tab bar leaves (`Renderer::tabs_end`); an item that does not fit is left out, and the
weather first shortens to "26° · City", then "26°". The battery opens System and the weather opens
Controls; both get the tab row's soft hover key.

## Panel pages

Order from `Page::ALL`: Home, Controls, Shelf, Focus, Timer, Water, Prayer, Tasks, System,
Agents, Mail, Plugins. Mail and Plugins stay off the tab bar while their switch is off
(`Pages::set_feature`, `Settings::sync_pages`); the Pages menu still lists them.
Focus and Timer share one tab (`Pages::tab`): the stopwatch tab reopens whichever was used last,
and a Focus | Timer switch takes the caption slot of both pages (the session count and the timer
length move into their status lines). Hiding Focus gives Timer its own tab again; right-click
moves the shared tab as one. Pages can be hidden via the tray menu → Pages (Home is always present) and moved
left/right by right-clicking the panel. Hiding a page also turns off its feature.

- **Home** without music shows at-a-glance chips under the clock (tasks left, water, agents that
  need you or are working); each chip opens its page. Not shown on the lock screen. With the
  calendar column it merges three sources (see [Home calendar](#home-calendar)).
- **Settings** is not in `Page::ALL`: it cannot be hidden or moved and is opened from the gear.
  It opens on the taller panel (`View::PanelTall`) and holds, one row each on a 34 DIP rhythm
  (`SETTINGS_ROWS`): theme swatches with a S/M/L size control, six switches (Lyrics, Weather,
  Mail, Calendar, Google Cal, Plugins), the prayer method and Asr school, and agent hooks. Every control sends the same `tray::Command` as the tray menu
  (`Hit::Command`), so both paths behave identically; rarer options stay in the tray menu.
- **Mail** and **Plugins** are off by default. The tray menu repeats the Mail, Google Calendar
  events, and Plugins switches, and adds what Settings has no control for: Google:
  Connect/Disconnect, Open plugins folder, and Calendar feeds (`calendars.txt`).

### Home calendar

The calendar column shows one day at a time. Its rows come from three sources, in this order:

1. **Typed agenda entries** from the agenda on this PC: a checkbox, the title, and the time
   ("09.00"), ticked off in place. These are the only rows that can be edited from the panel.
2. **Subscribed iCal feeds** (`src/tools/feeds.rs` reads `calendars.txt`; `src/tools/calfeed.rs`
   parses RFC 5545): read-only. One feed per line as `name|https://…`, at most six, fetched
   again at most every 30 minutes. `webcal://` becomes `https`; a plain `http` address is
   refused, because the address is a secret. Timed and all-day events, multi-day events, `TZID`
   in Windows or common IANA names, `UTC`, floating times, and the usual `RRULE` subset are
   expanded into local wall-clock occurrences up to 400 days ahead.
3. **Google Calendar events** from the connected account (toggle "Google Cal"): the primary
   calendar's next two weeks, read-only. They come with the same request as mail, so they follow
   its 5-minute rhythm.

Typed agenda rows come first; feed and Google rows follow in start time. A feed or Google row
has a dot instead of a checkbox: green while the event is running, dim otherwise. Its title is
green while running, and the feed name sits with the time (`09.00 · Work`), or alone for an
all-day event; Google rows read `Google`. Feed rows cannot be ticked off or removed, and the
address is never logged.

### Prayer page

Today's prayer times for the saved city (the same `city.txt` as the weather), computed on this PC
by `src/tools/prayer.rs`; nothing is downloaded and the device location is never read. Without a
city the page is an empty state with **Set city**; the name is looked up once with Open-Meteo's
geocoder, exactly like the weather city.

- Formulas from PrayTimes.org: sun declination and equation of time (USNO low-precision
  algorithm), two passes, and the angle-based night share when Fajr or Isha would not occur
  (high latitudes in summer). Times are whole minutes; the UTC instants are turned into wall
  time with Windows' own zone rules (`clock::local_moment`), so daylight saving is handled.
- Methods: **Kemenag** (default; 20°/18° plus 2 minutes of ihtiyat), MWL 18°/17°, ISNA 15°/15°,
  Egypt 19.5°/17.5°, Umm al-Qura 18.5° with Isha 90 minutes after Maghrib, Karachi 18°/18°. Asr
  is Standard (shadow × 1) or Hanafi (× 2). A test pins Jakarta to the published October 2026
  timetable within a minute.
- Layout: the caption names the city (click to change it); the hero is the next prayer with its
  time and countdown ("14:51 · in 1 h 31 min"); six even tiles show the day, the next one on an
  accent plate, passed ones dimmed; the bottom row picks the reminder (Azan, 5m, 10m, 15m, 30m)
  and steps through the methods.
- Notices: `Hud::Prayer`, checked on the minute tick. One arrives `prayer-lead` minutes before
  each azan (accent) and one at the azan (green), both with the reminder chime; clicking the
  notice opens the page; Sunrise has no azan
  and no notice. Hiding the page turns the notices off. Home shows "Asr 14:51" as a glance chip.
- Settings file keys: `prayer-method`, `prayer-asr` (`standard`/`hanafi`), `prayer-lead`
  (0, 5, 10, 15, or 30). The tray menu has **Settings → Prayer times** with all three.

### Agents page

Live coding-agent sessions on this PC. Each reader in `src/feeds/agents/` documents its sources;
in short:

| Agent | Source | Liveness |
|---|---|---|
| Claude Code | `~\.claude\sessions\<pid>.json` (`status`: busy, idle, waiting) | pid alive |
| Codex | `~\.codex\sessions\…
ollout-*.jsonl` tail (`task_started`, `task_complete`, `turn_aborted`) | `codex.exe` running, written in the last 30 min |
| Cursor | `~\.cursor\projects\<slug>	erminals\*.txt`, `agent-transcripts` | task pid alive, chat written in the last 5 min |
| Copilot CLI | `~\.copilot\session-state\<id>\events.jsonl` tail, `workspace.yaml` | written in the last 15 min |
| omp | `~\.omp
un\daemons\<hash>\clients\*.json` + transcript head | pid alive |

States, in sort order: **Needs you** (orange, waiting for permission or an answer), **Working**
(green ring), **Done** (accent, finished in the last 10 minutes), **Idle** (dim).

- `agents::Listener` runs on its own thread. Folder change notifications wake it at once; a 5 s
  tick catches exited processes. Scans are at least 400 ms apart while files keep changing.
- Kept quiet on purpose: two roomy rows (42 DIP, `agents::VISIBLE`), the rest summed up as
  "+N more" and reached with the mouse wheel (up to 12 sessions).
- Row: state dot, title, and a short status on the right ("Needs you" in orange, "Working",
  "Done · 3m", or the idle age). A dim second line names the agent in its tint and the project;
  only a session that needs you adds its waiting reason.
- Caption: "N need you" when any session waits, otherwise nothing. An **Install hooks** action is
  always available; it merges display-only lifecycle relays into Claude, Cursor, and Copilot
  configuration without replacing third-party hooks. Codex and omp keep using native readers.
  Once every relay is present the action reads "Hooks on" in muted text; the Settings page marks
  each installed provider with a check.
- Other tools join with one command: `crest.exe --agent-hook <tool> [event]` writes the same
  bounded status record. The tool may instead name itself in the payload with `crest_agent` (or
  `coucou_agent`), and may leave the event to the payload's `hook_event_name`. Vendor event names
  (Gemini CLI, Antigravity, Copilot, OpenCode, Hermes) are translated to the canonical ones, an
  unknown name is kept as written, and an unrecognised tool gets a stable tint from its name so
  `Kind::Other` rows never look like the built-in agents. A permission request is answered
  `{"permissionDecision":"ask"}` (Copilot) or `{"permission":"ask"}` (Cursor): Crest has no
  approval card, so it must never approve, and silence would make the agent hang.
- Clicking a row brings the agent's window forward: the agent pid's parent chain is walked (at
  most 6 steps) to the first process with a visible top-level window (terminal or editor). When
  the pid is unknown (Codex, Copilot CLI, Cursor chats), a window whose title contains the
  project folder name is used instead. Hovering a row exposes a separate project-folder action.
- A notice appears when a session starts waiting or a running session finishes (tray toggle
  "Coding agent notices"). Clicking it switches to the agent's window, or opens the Agents page
  when no window is found.
- No network, no databases, no full conversations: only small records, bounded heads and tails,
  lifecycle event names, tool names, project paths, and file times. Hook prompts and command
  arguments are never persisted.

### Mail page

Unread Gmail, read-only. Crest asks Google for `gmail.readonly`, so it lists message ids and the
subject and sender headers of each — never a body — and has no way to send, delete, or mark
anything read. `src/feeds/google.rs` owns the whole exchange; `src/system/datetime.rs` turns the
`Date` header into the short age ("12m"). Off by default; it refreshes only while the page is
open.

- Without an account the page is the one-time setup, laid out like a sheet: the title with
  **Connect…** on its right, one hint line (or the last failure as an alert), then full-width
  boxes for the client id and the client secret, all inside the deck. The text typed here is what Connect uses; both are saved in
  `google-client.txt` as plain text.
- **Connect** binds a loopback listener on `127.0.0.1` first, then opens the browser with a PKCE
  (S256) challenge and a random `state`, and waits for the reply on that port. The refresh token
  comes back encrypted with DPAPI for this Windows user (`google.txt`); the account's address is
  kept in `google-account.txt`. Crest never sees the password.
- Row: an avatar plate with the sender's initials (`Mail::initials`, one or two letters from
  the first and last word), the sender in label weight on the first line with the age in a fixed
  right-hand column so subjects start on one line, and the subject dim below. Three rows fit
  (`MAIL_ROWS`); the row that is hovered shows an open-envelope glyph. Clicking a row, the
  caption, or the "Open Gmail" link opens `https://mail.google.com`. The caption shows
  "N unread", or the status while checking. With nothing unread the page is the shared
  `empty_state`: "You'''re all caught up" with the signed-in address.
- **New mail arrives as a notice, like a message or an agent.** A refresh compares the unread
  ids with the previous list (`fresh_mail`); the first read after start only records them, so an
  old unread message is never announced. When one does arrive, a `Hud::Mail` shows the sender's
  initials, the sender, the subject, and "+N" for others that came with it; clicking it opens
  Gmail, and it is silent in quiet mode like the other notices.
- Refreshed at most every 5 minutes, and only while the page is open. A revoked or expired token
  clears the list, forgets the token, and asks for a new sign-in once instead of retrying.
- The tray menu carries one **Google** entry, **Google: Connect…** before an account is
  connected and **Google: Disconnect** after; Disconnect deletes the token and drops the rows.
  The lock screen shows nothing of the account, senders, or subjects.

### Plugins page

Local commands whose output becomes rows. `src/feeds/plugins.rs` reads one `<name>.plugin` file
per plugin from `%APPDATA%\Crest\plugins`, plain text:

```text
command = powershell -NoProfile -File "%APPDATA%\Crest\plugins\gmail.ps1"
every = 15
```

- The command prints one row per line as `title | detail | url`. Only the title is required; a
  row with an `http(s)` URL opens it in the browser when clicked (local paths are ignored, so a
  plugin cannot launch a program this way). Blank lines are skipped and unrecognised text is a
  title; output is capped at 16 KiB, with six rows kept per plugin.
- Off by default. Commands run only while the Plugins page is open, at most every `every` minutes
  (minimum 1, default 15), with a 20 s timeout and `CREATE_NO_WINDOW`, below normal priority.
- Turning the feature on writes `example.plugin` once, with the format in comments.
- Row: a green dot, the title, and the plugin's name on the right; a dim second line carries the
  detail, or "opens in the browser" for a link row. A plugin that fails shows a red dot with the
  reason.
- This page runs local code the user wrote, under the user's own permissions. A plugin may use
  the internet on its own if its command does; Crest never passes data to it. The tray menu has
  **Plugins** and **Open plugins folder**; the caption ("N plugins") reloads the folder.
