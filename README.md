# Crest

A small panel that hangs from the top center of your Windows screen: music with synced lyrics,
quick notices, a file shelf, a calendar, and a handful of widgets. Native, local, no account of
its own.

## Features

- **Music**: cover art, title, a draggable progress bar, and playback buttons for anything in the
  Windows media controls (Spotify, YouTube and YouTube Music in a browser, Media Player, and more).
  Optional synced lyrics in the pill and in a sliding lyrics column. Two-finger swipe on the pill
  changes tracks.
- **Notices**: volume, brightness, charger and low battery, Bluetooth, Wi-Fi, Caps/Num Lock,
  finished downloads, new screenshots, camera/microphone in use, and incoming messages (optional).
- **Coding agents**: a live list of Claude Code, Codex, Cursor, GitHub Copilot CLI, and omp
  sessions on this PC, each marked *needs you*, *working*, *done*, or *idle*, with a notice the
  moment an agent asks for permission or finishes a task. Rows jump back to the agent and expose
  its project folder. The Agents page can explicitly install local lifecycle hooks for Claude,
  Cursor, and Copilot; Codex and omp continue to use their native local session records. Hooks are
  display-only, preserve existing hook entries, and never approve or deny an agent action.
  Any other tool can report the same way: run `crest.exe --agent-hook <tool> [event]` from its
  hook, or send a payload with `crest_agent` (or Coucou's `coucou_agent`) naming the tool. Vendor
  event names for Gemini CLI, Antigravity, Copilot, OpenCode, and Hermes are translated, and a
  permission request is answered "ask", so the agent re-prompts in its own terminal.
- **Mail**: unread Gmail subjects and senders on the Mail page, read-only (`gmail.readonly`).
  Sign-in needs a Desktop app OAuth client; while its consent screen is in Testing, add your
  address under **Test users**, or Google answers `Error 403: access_denied`.
  Off by default and refreshed only while its page is open; connecting a Google account is
  described under [Install](#google-mail-and-calendar-optional).
- **Calendar**: Home's calendar merges three sources: typed agenda entries (tick them off when
  done), subscribed iCal feeds, and Google Calendar events. A feed row names its feed
  (`09.00 · Work`) and turns green with a dot while the event is running. The iCal list works
  with Google Calendar's secret address, Outlook, iCloud, and Fastmail.
- **Plugins**: one `<name>.plugin` file per plugin in `%APPDATA%\Crest\plugins`, plain text with a
  `command` and an `every` interval. The command prints rows of `title | detail | url`; a row
  with a URL opens in the browser when clicked. Off by default: a plugin runs local code you
  wrote, and Crest passes it no data.
- **Pages**: Home (clock or music, plus calendar and agenda), Quick controls (volume, brightness,
  microphone, quiet mode), Shelf (drop files on Crest, or on the Share zone), Focus, Timer, Water,
  Prayer (today's prayer times and a notice before each azan, calculated on this PC),
  Tasks, System (CPU, GPU, RAM, disk, network), Agents, Mail, and Plugins.
- **Look**: spring motion, a mascot that glances and reacts, six themes (Ink, Slate, Ocean, Ember,
  Frost, Paper), three sizes. Shrinks to a thin line over maximized windows, hides in full screen,
  and shows limited content on the lock screen.

## Install

Requires Windows 10 version 2004 or later, or Windows 11 (64-bit).

1. Download **crest.exe** from [Releases](https://github.com/rinci-labs/crest/releases).
2. Open it. Crest has no paid code-signing certificate, so Windows may show *"Windows protected
   your PC"*: click **More info → Run anyway**. See [Security](#security) to verify the file.
3. Choose **Yes** to install to `%LOCALAPPDATA%\Programs\Crest` (no administrator rights). Crest
   appears in the Start menu and starts with Windows. Choose **No** to run it once.

Settings live under the tray icon. **Update**: open a newer `crest.exe` and choose Yes.
**Uninstall**: Windows Settings → Apps → Crest → Uninstall; Crest asks whether to delete your data.

### Incoming messages (optional)

Windows only lets apps with a *package identity* read notifications. To show WhatsApp, Telegram,
and other chat messages, open the tray menu → Settings → Incoming messages → **Enable incoming
messages…** and approve the administrator prompt once. Crest then:

1. builds a small package that holds only Crest's name and logo (no program),
2. creates a signing key and a "Crest Local" certificate on this PC and signs the package,
3. **deletes the signing key immediately**, so the certificate cannot sign anything else,
4. trusts the certificate's public part, registers the package for your account, and restarts.

Windows then asks for permission to read notifications. Uninstalling Crest removes the package and
certificate.

### Google mail and calendar (optional)

Crest uses your own OAuth client, so no Crest server is involved. One-time setup:

1. Create a free project in [Google Cloud Console](https://console.cloud.google.com), enable the
   **Gmail API** and the **Google Calendar API**, and create an OAuth client of type **Desktop
   app**.
2. Open the Mail page (turn it on in Settings or the tray menu) and paste the client id, and the
   client secret if Google shows one, then press **Connect**.

Crest opens your browser: the sign-in uses PKCE and returns to a temporary port on `127.0.0.1`.
Crest never sees your password. It asks only for `gmail.readonly` and `calendar.readonly`, keeps
the refresh token encrypted with DPAPI for this Windows user in `%APPDATA%\Crest\google.txt`, and
stores the client id and secret as plain text in `google-client.txt`. Mail stays off by default
and refreshes only while its page is open, every 5 minutes. The tray menu has one **Google** entry:
**Google: Connect…** while no account is connected, **Google: Disconnect** afterwards.

### Calendar feeds and plugins (optional)

Both live under `%APPDATA%\Crest` and each has an entry in the tray menu:

- **Calendar feeds**: `%APPDATA%\Crest\calendars.txt`, one `name|https://…` line per feed. A
  Google Calendar secret iCal address, Outlook, iCloud, and Fastmail all work; `webcal://` is
  accepted. Feeds are read-only and fetched at most every 30 minutes. The address is a secret, so
  plain `http` is rejected and the address is never logged.
- **Plugins**: `%APPDATA%\Crest\plugins`, one `<name>.plugin` file per plugin with `command = …`
  and `every = <minutes>`. Turning plugins on for the first time writes an `example.plugin` that
  explains the format. Commands run only while the Plugins page is open, at most every `every`
  minutes, with a 20 s timeout and no console window.

## Privacy

- No Crest account, no telemetry, no ads.
- Data is plain text in `%APPDATA%\Crest`, readable with Notepad, except the Google refresh token
  in `google.txt`, which is encrypted with DPAPI for this Windows user.
- Features that use the internet are **off by default**:
  - **Lyrics** from [LRCLIB](https://lrclib.net): title, artist, album, and duration of the
    current track.
  - **Weather** from [Open-Meteo](https://open-meteo.com): the city name you type (once), then that
    city's coordinates every 30 minutes. Your location is never read.
  - **Mail and Google Calendar**, only after you connect an account: Crest reads unread message
    subjects and senders (no bodies) and the next two weeks of events from Google's APIs. The
    request goes only to Google, with the OAuth client you created; no Crest server is involved.
- **Calendar feeds**: Crest sends a request to each address in `calendars.txt` at most every 30
  minutes, so that host sees this PC's IP address. The address is a secret in itself: plain `http`
  is rejected, and neither the address nor the events are logged.
- **Plugins** run a command you wrote, on this PC, with your permissions. Crest passes them no
  data; a plugin may use the internet on its own if its command does. Only `http` and `https` URLs
  in its output open in the browser, and Crest does not fetch them itself.
- Messages are read from Windows notifications on this PC and never stored or sent. "Hide message
  content" and meeting mode hide the text.
- Camera and microphone are never turned on; Crest only sees which apps use them. From the
  keyboard, Crest only watches Caps Lock and Num Lock.
- The error log (`log.txt`) records the kind of failure only, never content or file names.

## Security

- Open source. The only dependencies are Microsoft's official
  [windows-rs](https://github.com/microsoft/windows-rs) crates.
- Releases are built by GitHub Actions from this repository (`.github/workflows/release.yml`).
  Verify a download with `SHA256SUMS.txt` on the release page, or:

  ```bash
  gh attestation verify crest.exe -R rinci-labs/crest
  ```

- The administrator step for messages reads no outside files or input; the package is built and
  signed in the Windows-owned Temp folder before it is copied to the install folder.
- Report vulnerabilities privately through *Security → Report a vulnerability*. See
  [SECURITY.md](SECURITY.md).

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by
[SignPath Foundation](https://signpath.org).

Only `crest.exe` built by the [release workflow](.github/workflows/release.yml) from this
repository is signed, on a GitHub-hosted runner, and only after a team member approves the request.

| Role | Members |
|---|---|
| Committers and reviewers | [@rafaalrazzak](https://github.com/rafaalrazzak) |
| Approvers | [@rafaalrazzak](https://github.com/rafaalrazzak) |

Every pull request from outside the team is reviewed by a team member before it is merged. All
team members use multi-factor authentication for GitHub and SignPath.

Privacy: Crest sends no data to other networked systems unless the user turns on a feature that
does so. See [Privacy](#privacy) for each one.

## Build

Requires [Rust](https://rustup.rs) (version pinned in `rust-toolchain.toml`) and Visual Studio
Build Tools with the C++ workload and Windows SDK.

```bash
cargo build --release   # target\release\crest.exe
cargo test              # also renders every panel state to target\preview\*.bmp
cargo clippy --all-targets
```

- `crest.exe --install` installs without a dialog.
- `packaging\update.cmd` copies a fresh release build over the installed copy.
- Debug builds (`cargo run`) open a console with logs. Set `CREST_INSTALL_ROOT=<folder>` to test
  the installer without touching the real installation.
- `crest.exe --check-notifications` writes the notification permission status to
  `%TEMP%\crest-notification-check.txt`, without message content or sender names.
- The app icon and package logos are drawn in code: `cargo test write_ -- --ignored`, then
  `packaging\logos.ps1`.

## Project layout

| Path | Contents |
|---|---|
| `src/main.rs` | Entry point: CLI commands, DPI awareness, single-instance guard |
| `src/app/` | The window and its state, split by concern: `input`, `events`, `schedule`, `frame` (motion and drawing), `views`, `hud`, `mascot`, `playback`, `files`, `commands`, `config` |
| `src/ui/` | Direct2D renderer (`mod.rs`), page layouts (`pages.rs`), design tokens (`tokens.rs`), spring motion (`motion.rs`), and render tests |
| `src/tools/` | Page logic and data: agenda, focus, shelf, tasks, timer, water, page order, and iCal feeds (`feeds.rs`, `calfeed.rs`) |
| `src/feeds/` | Outside data: media sessions, lyrics, weather, messages, Google mail and calendar (`google.rs`), plugins (`plugins.rs`), coding agents (`agents/`) |
| `src/system/` | Windows wrappers: audio, brightness, power, Bluetooth, network, privacy, stats, desktop, drag and drop, thumbnails, sounds, share, clock, HTTP, watch, service thread, `datetime.rs`, `secrets.rs` |
| `src/shell/` | Install, package identity, autostart, tray, settings, storage, and the error log |
| `build.rs`, `packaging/` | Exe manifest, icon, identity package, update and logo scripts |

## Performance

Crest draws only when something moves and otherwise sleeps on the message queue (idle CPU about
0%). Clock strings are rebuilt once per minute, not per frame. Measured on Windows 11 a few seconds
after start: about 15 MB private memory and about 50 MB working set (most of it shared system
libraries for Direct2D and WinRT).

## License

[MIT](LICENSE). Lyrics from [LRCLIB](https://lrclib.net), weather from
[Open-Meteo](https://open-meteo.com).
