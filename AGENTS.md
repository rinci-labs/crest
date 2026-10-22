# Guide for coding agents

This file is for AI agents (Claude Code, Cursor, omp, Codex, etc.) and contributors changing code
in this repository. The UI design and architecture are in [DESIGN.md](DESIGN.md); installation and
usage are in the [README](README.md).

## Overview

Crest is a top-of-screen panel for Windows, written in Rust. A single binary
(`crest.exe`), no extra runtime, runs locally with no account. Windows 10 2004+ and Windows 11
(64-bit) only.

## Commands

```sh
cargo build                     # debug build (console window, debug_log! enabled)
cargo build --release --locked  # release build, as in CI
cargo test --locked             # all tests, same as the test workflow
cargo clippy --all-targets      # lint; clippy is installed via rust-toolchain.toml
```

The compiler version is pinned in `rust-toolchain.toml` (1.99.1). Do not change it without a
reason, because release builds on GitHub Actions use the same version.

Before calling work done: `cargo build` and `cargo test` must pass without errors.

## Layout

| Path | Contents |
|---|---|
| `src/main.rs` | Entry point: CLI commands (`--install`, `--uninstall`, ...), single-instance mutex, DPI, then `app::run()` |
| `src/app/mod.rs` | `App` state, window creation, message loop, `wndproc`, shared helpers |
| `src/app/*.rs` | `impl App` split by concern: `input`, `events`, `schedule`, `frame`, `views`, `hud`, `mascot`, `playback`, `files`, `commands`; constants in `config.rs` |
| `src/ui/mod.rs` | `Renderer` (Direct2D + DirectWrite), `Scene`, view types, hit-testing, app icon |
| `src/ui/pages.rs` | Drawing for each panel page |
| `src/ui/tokens.rs` | Design tokens: spacing, radii, type sizes, opacity, motion |
| `src/ui/motion.rs` | Spring animation |
| `src/ui/tests.rs` | Renderer tests and `target/preview` images |
| `src/tools/` | Page logic: agenda, focus, shelf, tasks, timer, water; `Page`/`Pages` in `mod.rs` |
| `src/feeds/` | Media sessions, lyrics, weather, incoming messages; `agents/` has one reader per coding agent, shared `util.rs`, and the folder `listener.rs` |
| `src/system/` | Windows wrappers and the background service thread (`services.rs`) |
| `src/shell/` | Install, identity package, autostart, tray, settings, storage, error log |
| `packaging/` | Manifest (`crest.exe.manifest`), icon (`crest.ico`), identity package, update and logo scripts |
| `.github/workflows/` | `test.yml` (tests, clippy, release build on Blacksmith runners), `release.yml` (release, GitHub-hosted) |

## Code rules

- **Language:** comments, doc comments, UI text, and commit messages are in English. Identifiers
  are in English too (`Session`, `scan`, `agents_page`).
- **Dependencies:** only Microsoft's official crates from the windows-rs repository (`windows`,
  `windows-core`, etc.). Do not add third-party crates. When adding a new `windows` feature in
  `Cargo.toml`, give a short reason if it is not obvious.
- **UI numbers:** spacing, radii, type sizes, and opacity come from `ui::tokens`, not new
  literals in `ui/mod.rs` or `ui/pages.rs`. See [DESIGN.md](DESIGN.md).
- **Logging:** `debug_log!` is for debug builds only. `error_log!` is also written in release
  builds, so **never** include message content, track titles, file names, folder names, or any
  other user data.
- **Network:** only lyrics (LRCLIB) and weather (Open-Meteo), both off by default. New features
  must not send data out without the user's explicit consent.
- **User data:** stored as plain text files in `%APPDATA%\Crest` so it can be read with Notepad.
  Do not use binary formats or databases.
- **External-file scanners** (such as `feeds/agents/`): read small files only, bound the read size,
  never read full conversation contents, and drop entries whose pid is no longer alive.
- **`unsafe`:** only around Win32/COM calls, with a comment when there is a non-obvious
  requirement (for example a handle intentionally left open).
- Follow the surrounding style: small modules per feature, named constants with doc comments,
  small value types (`Ring`, `EyeSize`, `Header`) instead of long argument lists, no
  abstractions that are not used yet.
- `cargo clippy --all-targets` must stay free of warnings.

## Adding a panel page

1. Add a variant to `tools::Page` and to `Page::ALL`, along with its `label()` and its settings
   `key()`.
2. Add view data to `ui::Scene` if needed.
3. Write a drawing function `fn <name>_page(&self, s: &Scene, x0: f32, x1: f32, a: f32)` in
   `ui/pages.rs` and hook it into the page `match`.
4. Add click targets to `ui::Hit` and handle them in `app/input.rs`.
5. Use tokens from `ui::tokens`; add new tokens there only when really needed.
