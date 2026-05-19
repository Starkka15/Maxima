# Maxima-Draconis — engineering reference for Claude agents

This is the **Maxima-Draconis fork** — the EA authentication and launch backend used by [Draconis](https://github.com/AA-EION/Draconis), a native macOS launcher for Titanfall 2 on CrossOver / Wine. This file is the living engineering reference for anyone picking up the repo cold: architecture, gotchas, diagnostics, and a running changelog.

If you're updating this file, the rule is: **state of the world first, history at the bottom**. Don't make new sessions read three months of changelog before learning what the code currently does.

---

## What Maxima is

Open-source replacement for the EA Desktop / Origin launcher. **Not** a macOS-native app — `maxima-cli` / `maxima-bootstrap` / `maxima-service` are Windows binaries that run **inside the CrossOver bottle** alongside Titanfall 2. The only piece that runs on the macOS host is `MaximaHelper.app`, a tiny Swift background agent that bridges EA's `qrc://` OAuth redirect from the user's browser into the bottle.

The Draconis fork is tested *only* for Titanfall 2 on macOS via CrossOver. Other configurations may work but aren't supported here.

### Multi-OS compatibility principle

Even though the active maintenance target is macOS/CrossOver, **the code must remain compatible with the other OSes upstream supports** — Linux (native + musl) and native Windows:

- All `#[cfg(unix)]`, `#[cfg(target_os = "linux")]`, `#[cfg(target_os = "macos")]`, `#[cfg(windows)]`, `#[cfg(not(windows))]` gates that exist in upstream must be preserved when editing the affected file.
- Don't introduce hard `panic!()` or `unimplemented!()` on a code path that other OSes hit at runtime.
- Don't add `#[cfg]`-gated dependencies that would skip building on other targets without a clear reason; if you need to, scope the gate as narrowly as possible.
- `maxima-ui` and `maxima-tui` are **upstream graphical / TUI frontends**. They are built and shipped in this fork's Windows installer (`maxima.exe`, `maxima-tui.exe`) — the UI is wired up for future use even though Draconis currently invokes only the CLI side. On Linux they are excluded from CI because `maxima-ui` transitively pulls `rustix 0.37` via `accesskit_unix → zbus → async-io`, which doesn't build on modern nightly. The Windows target uses a different rustix path (no unix backend) and compiles fine, so we ship them there. **Do not delete them from the workspace.**
- The Linux CI job builds `maxima-cli` + `maxima-bootstrap` to make sure the cross-platform code paths actually compile on a non-macOS unix. The Windows CI job builds the three Draconis-relevant crates **and** the NSIS installer. If you touch `#[cfg(unix)]` or `#[cfg(windows)]` blocks, make sure those jobs still pass.

In short: macOS/CrossOver is what we **test**, but the codebase is **portable** to the same targets upstream supports.

---

## Component layout

```
macOS host
├── Draconis.app           — SwiftUI launcher (in AA-EION/Draconis)
│   └── Contents/Resources/
│       └── MaximaHelper.app — qrc:// → http://127.0.0.1:31033 bridge
│                              (built from MaximaHelper/ in this repo)
│
└── CrossOver bottle (Wine prefix)
    └── Program Files/Maxima/
        ├── maxima-cli.exe         — auth + launch CLI (also runs `serve` mode)
        ├── maxima-bootstrap.exe   — link2ea:// / origin2:// / qrc:// handler
        ├── maxima-service.exe     — background service (DLL injection, registry setup)
        ├── maxima.exe             — upstream GUI (shipped, not yet wired into Draconis)
        ├── maxima-tui.exe         — upstream TUI (shipped, not yet wired into Draconis)
        └── Uninstall.exe          — NSIS uninstaller
```

> **Path note:** Wine on macOS uses `Program Files`, not `Program Files (x86)`, so the install lands at `drive_c/Program Files/Maxima/`. The NSIS script uses `$PROGRAMFILES` which resolves correctly under both layouts.

Build outputs:

- `installer/MaximaSetup.exe` — NSIS bundle that installs everything in the bottle and registers the protocol handlers in Wine's registry. Cross-compiled on macOS via `mingw-w64` + `nsis`.
- `MaximaHelper/build/MaximaHelper.app` — built on macOS with Xcode CLT.
- `MaximaHelper.zip` — release asset Draconis downloads at build time.

---

## Workspace inventory

```
maxima-lib/          Core library — auth, launch, license, library, LSX,
                     RTM, OOA, cloudsync, /authorize HTTP server, Steam
                     install helpers. All other crates depend on this.
maxima-cli/          CLI frontend — `maxima-cli launch <slug>` (legacy
                     orchestrated launch), `maxima-cli serve` (passive
                     auth-only mode), plus utility subcommands.
maxima-bootstrap/    Protocol handler binary — registered for link2ea://,
                     origin2://, qrc:// in Wine's registry. Parses the URL,
                     validates the offer_id, and either forwards to a
                     running Maxima via HTTP /authorize or spawns
                     maxima-cli launch as fallback.
maxima-service/      Windows background service — registry setup, DLL
                     injection for KYBER. Windows-only (no-op `main` on
                     other targets). Not exercised in the Draconis flow.
maxima-tui/          Terminal UI (upstream, ratatui-based). Shipped in the
                     installer but not invoked by Draconis yet.
maxima-ui/           Graphical UI (upstream, eframe/egui). Patched in this
                     fork: wgpu renderer (glow can't get a GL 3.3 core
                     context under Wine on macOS); two CPU-burning busy
                     loops fixed; red-placeholder background swapped to
                     transparent so the dark theme shows; friend-presence
                     event spam dedup'd in the event thread. Validated
                     end-to-end on CrossOver: login + library + install +
                     launch (TF2). Shipped in the installer; not invoked
                     by Draconis yet.
maxima-resources/    Logo assets (`logo.ico`, `logo.png`) + `winres`-based
                     build-time helper that embeds Windows .exe metadata
                     (icon, CompanyName, FileDescription, etc.) when
                     building on Windows; no-op on other targets. Used as
                     a `[build-dependencies]` entry by every frontend
                     crate. (Translations live in `maxima-ui`, not here.)
MaximaHelper/        Native macOS Swift app (build.sh + Info.plist +
                     Sources/main.swift). Bridges qrc:// from the host
                     browser into the bottle via http://127.0.0.1:31033.
installer/           NSIS script (maxima-setup.nsi) + cross-build script
                     (build.sh, uses mingw-w64 + makensis).
images/              Repo images — banners, screenshots.
.github/workflows/   build-ci.yml (push CI), release.yml (tag release),
                     block-upstream-pr.yml (prevent accidental PRs to
                     upstream).
```

Key entry points:

| File                                          | What it does                                                       |
|-----------------------------------------------|--------------------------------------------------------------------|
| `maxima-cli/src/main.rs`                      | CLI argparse + subcommand dispatch (Launch, Serve, ListGames, …)   |
| `maxima-bootstrap/src/main.rs`                | Protocol URL parser + auth-server probe + HTTP forward / spawn     |
| `maxima-lib/src/auth_server.rs`               | `GET /` + `POST /authorize?offer_id=X` over plain TCP, port 13219  |
| `maxima-lib/src/steam.rs`                     | `STEAM_GAMES` table, Steam install path discovery (registry + VDF) |
| `maxima-lib/src/core/launch.rs`               | `start_game()` — license preflight, env vars, spawn the game       |
| `maxima-lib/src/core/auth/login.rs`           | OAuth flow + `remid`-cookie fallback for macOS/CrossOver           |
| `maxima-lib/src/core/mod.rs`                  | `Maxima` struct, `start_lsx` (with probe), `start_auth_server`     |
| `maxima-lib/src/lsx/connection.rs`            | LSX socket lifecycle + ConnectionState (game_version, etc.)        |
| `maxima-lib/src/lsx/service.rs`               | LSX TCP listener on port 3216 + accept loop                        |
| `maxima-lib/src/lsx/request/license.rs`       | Denuvo token fetch (env override: `MAXIMA_DENUVO_TOKEN`)           |
| `maxima-lib/src/util/registry.rs`             | Windows registry: install check + protocol registration            |
| `maxima-lib/src/unix/wine.rs`                 | Wine detection, registry setup via `regedit /S`                    |
| `maxima-lib/src/util/dll_injector.rs`         | KYBER DLL injection (Windows-only, UTF-16 paths)                   |
| `MaximaHelper/Sources/main.swift`             | NSApplicationDelegate that handles `qrc://` URLs                   |
| `installer/maxima-setup.nsi`                  | NSIS script, takes `/DBIN_DIR` for binary location                 |

---

## Current architecture: two launch paths

A bottle running this fork can authenticate games **two ways**. They use the same underlying `maxima-lib` code; the difference is whether Maxima is treated as a long-running auth service or as an on-demand orchestrator.

### Path A: `serve` + bootstrap-forwarded launch  *(preferred for Draconis / Steam)*

```
┌──────────────────────────────────────────────────────────────────────┐
│ Terminal 1: `maxima-cli.exe serve` (started by user / Draconis)      │
│                                                                      │
│   maxima-cli                                                         │
│     ├── log in (cached refresh token, or OAuth on first run)         │
│     ├── start_lsx()  →  TCP listen 127.0.0.1:3216                    │
│     └── start_auth_server() → TCP listen 127.0.0.1:13219             │
│            (HTTP: GET / + POST /authorize?offer_id=X)                │
│                                                                      │
│   maxima.playing() = None  (no game launched yet)                    │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│ Steam / Draconis / user starts Titanfall2.exe                        │
│                                                                      │
│   Titanfall2.exe                                                     │
│     ├── starts up                                                    │
│     ├── DRM stub: "I need Origin / EA auth"                          │
│     ├── emits link2ea://launchgame/Origin.OFR.50.0001456?…           │
│     └── EXITS — expects the link2ea handler to re-launch it with     │
│                  EA auth context (EAGenericAuthToken, EALsxPort, …)  │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│ Wine routes link2ea:// to maxima-bootstrap.exe                       │
│                                                                      │
│   maxima-bootstrap                                                   │
│     ├── parses URL, validates Origin.OFR.<digits>.<digits>           │
│     ├── TCP probe 127.0.0.1:13219 with 200ms timeout                 │
│     ├── alive → POST http://127.0.0.1:13219/authorize?offer_id=X     │
│     │             [&cmd_params=...] with 60s timeout                 │
│     └── exits (logs outcome to %TEMP%/maxima_execution.log)          │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│ `serve`'s auth_server handles POST /authorize                        │
│                                                                      │
│   handle_authorize(offer_id="Origin.OFR.50.0001456")                 │
│     ├── auth_storage.logged_in()  → must be true                     │
│     ├── library.game_by_base_offer(offer_id)  → must be Some(…)      │
│     ├── steam install-path lookup (path_override for Steam-          │
│     │   installed TF2; falls back to offer.execute_path otherwise)   │
│     └── launch::start_game(Online(offer_id), LaunchOptions{…})       │
│           ├── request_and_save_license  → .dlf on disk               │
│           ├── builds full EA-* env (EALsxPort, EAGenericAuthToken,   │
│           │   EAAccessTokenJWS, EALaunchEAID, ContentId, …)          │
│           ├── spawns bootstrap with base64(BootstrapLaunchArgs)      │
│           │   → bootstrap runs Titanfall2.exe with that env          │
│           └── maxima.playing = Some(ActiveGameContext)               │
│     → returns 200 OK {"status":"ok"} after the spawn is in flight    │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│ Re-launched TF2 connects to serve's LSX                              │
│                                                                      │
│   Connection::new(serve.maxima_arc)                                  │
│     └── maxima.playing() = Some(ctx) (set by launch::start_game)     │
│           → standard active-launch branch                            │
│           → request_license does real OOA fetch on TF2's request     │
│           → set_presence updates RTM                                 │
│                                                                      │
│   LSX handshake → Challenge → GetProfile → … → game runs             │
└──────────────────────────────────────────────────────────────────────┘
```

Key property: **`/authorize` does NOT just preflight the license — it
also spawns the game.** TF2's Origin DRM stub emits `link2ea://` and
exits, expecting whoever handles the URL to re-launch it. Bootstrap
forwards to `/authorize`, which calls `launch::start_game`, which
spawns a fresh TF2 with the full EA env in place. This is the same
code path the upstream UI's "Play" button takes — `serve` just lets
us reuse a single logged-in session across many launches instead of
re-bootstrapping from scratch each time. **`maxima.playing` ends up
`Some(...)` on the server, so the LSX flow goes down the standard
active-launch branch (not catornot's external-LSX branch).**

The `serve` loop also calls `maxima.update()` once per second, so
when the game exits the server detects it (`update_playing_status`
runs the cloud-save sync and clears `playing`), leaving the auth
server ready for the next launch.

### Path B: legacy `maxima-cli launch <slug>` *(fallback when `serve` isn't running)*

```
┌──────────────────────────────────────────────────────────────────────┐
│ Anything emits link2ea:// (or user runs `maxima-cli launch X`)       │
│                                                                      │
│   bootstrap parses URL                                               │
│     ├── TCP probe 127.0.0.1:13219                                    │
│     └── DEAD → spawns `maxima-cli.exe launch <offer_id>`             │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│ Fresh maxima-cli process: full orchestrated launch                   │
│                                                                      │
│   maxima-cli launch                                                  │
│     ├── login (cached / OAuth)                                       │
│     ├── start_lsx()                                                  │
│     │     ├── probe 127.0.0.1:3216 — anything listening?             │
│     │     │     YES (e.g. UI running) → skip our bind, defer to it   │
│     │     │     NO → bind ourselves                                  │
│     │     └── listening                                              │
│     ├── resolve slug → offer_id (EA library / Origin pattern /       │
│     │                            STEAM_GAMES table)                  │
│     ├── set SteamAppId / SteamGameId env vars if slug is numeric     │
│     ├── launch::start_game(LaunchOptions { steam_launch })           │
│     │     ├── request_and_save_license → .dlf on disk                │
│     │     ├── set EAEntitlementSource / EAExternalSource /           │
│     │     │   EALaunchOwner = "Steam" or "EA" per steam_launch       │
│     │     ├── spawn bootstrap with base64(BootstrapLaunchArgs)       │
│     │     │   bootstrap then runs the game executable                │
│     │     └── maxima.playing = Some(ActiveGameContext)               │
│     └── poll loop until playing becomes None (game exits)            │
└──────────────────────────────────────────────────────────────────────┘
```

Path B is what upstream Maxima does. It still works for the cases where the game **isn't** running yet at the moment `link2ea://` fires (e.g. a modern EA-Desktop title launched via a desktop shortcut that hands the launch off to EA Desktop). For Titanfall 2 launched via Steam it's the wrong path — TF2 is already running and waiting — but it's preserved as the bootstrap fallback so users who never start `serve` still get a working launch.

### Why the split

Before this rewrite, every `link2ea://` invocation **fully re-bootstrapped Maxima**:

1. Steam launches `Titanfall2.exe` directly (Steam does NOT emit link2ea here for TF2 — old game, predates EA Desktop).
2. TF2 starts, emits `link2ea://launchgame/Origin.OFR.50.0001456?platform=PCWIN`, and exits expecting a relaunch.
3. Wine → bootstrap → spawns a fresh `maxima-cli launch …` process.
4. That maxima-cli re-does OAuth login (or refreshes the cached token), restarts LSX, etc., then calls `launch::start_game` which spawns Titanfall2.exe again with EA env vars.
5. Works in principle, but: every launch pays the full Maxima startup cost; and on macOS/CrossOver the surface area kept tripping different failure modes (LSX port race, console-visibility, file-corruption-after-PI).

Path A centralizes the auth-provider role in a long-running `serve` process. When `link2ea://` fires, bootstrap doesn't restart Maxima — it forwards to the running `serve` over HTTP, which already has cached login state. `serve` then calls the same `launch::start_game` path the upstream UI does (so the EA env vars end up on the re-spawned TF2 identically), but without paying for a fresh `maxima-cli` process startup each time.

**An earlier draft of Path A skipped the game-spawn entirely** — the theory was that TF2 stayed alive polling LSX after emitting link2ea, so we just needed the auth server to refresh `.dlf` and TF2's polling would reconnect. Empirically TF2 *exits* after emitting link2ea (it expects EA Desktop to relaunch it), so a preflight-only `/authorize` leaves the game closed and the user sees "TF2 opens for a moment and then closes". The current design (spawn the game from `/authorize`) is the corrected version, aligned with upstream issue [#27](https://github.com/ArmchairDevelopers/Maxima/issues/27) ("Protocol handler should then use the obtained parameters to launch the game process").

---

## URI protocols Maxima owns

| Scheme       | Registered by                                            | Where         | Handler does                                                          |
|--------------|----------------------------------------------------------|---------------|-----------------------------------------------------------------------|
| `qrc://`     | `MaximaHelper.app`                                       | macOS host    | GETs `http://127.0.0.1:31033/auth?<query>` inside the bottle           |
| `qrc://`     | `maxima-bootstrap.exe`                                   | Wine registry | Same target as above (host handler wins when Draconis is installed)   |
| `link2ea://` | `maxima-bootstrap.exe`                                   | Wine registry | Probe + HTTP forward to `/authorize`, else spawn `maxima-cli launch`  |
| `origin2://` | `maxima-bootstrap.exe`                                   | Wine registry | Same as `link2ea://`. Reads real `offerIds` (no longer hardcoded BF2) |

The `qrc://` listener on `127.0.0.1:31033` is **only up during an interactive OAuth login** (inside `core/auth/login.rs::begin_oauth_login_flow`). After the login completes that listener exits. It is **not** the same server as the `/authorize` HTTP endpoint, which lives on port `13219` and runs for the lifetime of `maxima-cli serve` / a UI session.

MaximaHelper.app's bundle id is `com.armchairdevelopers.maxima.helper`. **The Draconis fork's Info.plist must remain signed-sealed** — see "Signing gotcha" below.

---

## EA identifiers cheat sheet

| Thing                     | TF2 value                               |
|---------------------------|-----------------------------------------|
| Steam App ID              | `1237970` (resolved via `STEAM_GAMES` table when EA library lookup fails) |
| EA Origin offer id        | `Origin.OFR.50.0001456` (real TF2 offer id, NOT `0002694` / `0002148` which are Apex / Battlefront 2) |
| MaximaHelper bundle id    | `com.armchairdevelopers.maxima.helper`  |
| MaximaHelper qrc port     | `127.0.0.1:31033` inside Wine            |
| LSX port                  | `127.0.0.1:3216` (override via `MAXIMA_LSX_PORT`)            |
| Authorize HTTP port       | `127.0.0.1:13219` (override via `MAXIMA_AUTHORIZE_PORT`)     |

---

## Deltas vs upstream `ArmchairDevelopers/Maxima`

Everything below is on top of upstream `master` at `cbde5f0`. Categorized so we can tell what's macOS-specific from what could go upstream.

### 1. New infrastructure (macOS / Draconis-specific)

- **`MaximaHelper/`** — native Swift macOS background agent. Replaces upstream's AppleScript helper with a bundle-signable binary LaunchServices honors for `qrc://`. Universal arm64 + x86_64, built via `swiftc` from `MaximaHelper/build.sh`. Bundle id `com.armchairdevelopers.maxima.helper`; listens for `qrc://`, forwards query to `http://127.0.0.1:31033/auth?<query>` inside the bottle.
- **`installer/`** — NSIS-based Windows installer (`maxima-setup.nsi`) and cross-compile script (`build.sh`). Drops `maxima-cli.exe`, `maxima-bootstrap.exe`, `maxima-service.exe`, `maxima.exe`, `maxima-tui.exe` into the bottle. Registers `link2ea://` / `origin2://` / `qrc://` in Wine's registry with backup/restore semantics for the pre-Maxima state. Cross-compiled on macOS via `mingw-w64` + `nsis`. Supports `/DBIN_DIR=<path>` to override the cargo target dir.
- **`.github/workflows/`** — three workflows: `build-ci.yml` (push CI matrix Linux/Windows/macOS), `release.yml` (tag-triggered: builds the helper on macOS + the installer on Windows + assembles the GitHub release on Ubuntu), `block-upstream-pr.yml` (prevents accidentally PR-ing fork-specific changes upstream).
- **`.cargo/config.toml`** + **`rust-toolchain.toml`** — nightly pin (required by upstream's `#![feature(slice_pattern)]` etc.) and MinGW cross-compiler hookup.

### 2. Code changes (most of these could go upstream)

#### Bootstrap protocol handlers (`maxima-bootstrap/src/main.rs`)
- Implemented `link2ea://` (was `todo!()` upstream).
- `origin2://` reads real `offerIds` from the URL instead of hardcoded `Origin.OFR.50.0002148`. **(Generic — useful for every EA title.)**
- `qrc://` no longer panics on URLs missing `login_successful.html?` (was indexing `[1]` on a split vec without bounds checking).
- Both `link2ea` and `origin2` validate `offer_id` against `Origin.OFR.<digits>.<digits>` or `<1..=10 digits>` before invoking anything — defends against `link2ea://launchgame/--login=stolen_token` flag injection.
- **NEW (Session 2026-05-18):** Both protocols probe `127.0.0.1:13219` and forward via HTTP `POST /authorize` when a Maxima auth server is running. Falls back to spawning `maxima-cli launch` only if no server answers.
- `KYBER_INTERFACE_PORT` forwarded from parent env (was hardcoded `3005`).
- Non-zero exits from spawned `maxima-cli` are surfaced as errors (used to be swallowed silently).
- All protocol-handler invocations append a line to `%TEMP%/maxima_execution.log` — bootstrap is a GUI-subsystem binary with no console, this is its only feedback channel.

#### CLI runtime (`maxima-cli/src/main.rs`)
- **NEW (Session 2026-05-16):** `Mode::Serve { no_rtm }` subcommand — passive auth-only mode. Starts LSX + auth_server, optionally logs in to RTM for friends presence, then parks. See "Path A" above.
- **NEW:** Console + stdio rewire prologue so the CLI is visible when spawned by GUI-subsystem `maxima-bootstrap`. Calls `AllocConsole()` if no console attached, then `SetStdHandle(STD_*_HANDLE, CreateFileA("CONOUT$"|"CONIN$"))` so Rust's `println!` actually reaches the new window.
- **NEW:** Panic hook writing to `%LOCALAPPDATA%\Maxima\Logs\maxima-cli.panic.log` before unwinding — catches panics that fire before the regular logger is initialized.
- **NEW:** `main()` is plain `fn`, builds tokio runtime manually with `Builder::new_multi_thread().enable_all()`. The previous `#[tokio::main]` macro built the runtime before user code, which defeated the panic hook.
- **`Mode::Launch`** (legacy path B) now:
  - Resolves slug via EA library lookup, then EA-offer passthrough, then `STEAM_GAMES` table fallback for Steam-only owners with unlinked accounts.
  - Sets `SteamAppId` / `SteamGameId` / `SteamClientLaunch` / `SteamPath` env vars when slug matches `<1..=10 digits>` (Steam App ID pattern).
  - Resolves Steam install path via `lookup_steam_game` + `resolve_steam_install_path` (registry + `libraryfolders.vdf` parse) when no `--game-path` is given.
  - Per-game launch args (e.g. `-noOriginStartup` for Northstar, `-multiple` for Source-engine titles) are NOT auto-injected. Callers pass them via `--game-args`, `MAXIMA_LAUNCH_ARGS`, or `cmd_params` on the `link2ea://` URL — Maxima stays universal.
- `Mode::GetGameBySlug` actually prints slug/offer_id/content_id/display_name/installed (was a no-op stub upstream).
- **`Mode::ListGames { json }`** — when `--json` is passed, emits a JSON array on stdout (slug, name, offer_id, content_id, installed, install_path, version, has_cloud_save, extra_offers) and suppresses the logger's stdout output for the duration of the command. Designed for Draconis pre-flight detection: "what does Maxima know about this user's library, in machine-readable form?". File-sink logging is unaffected, so debugging traces still land in `%LOCALAPPDATA%\Maxima\Logs\maxima-cli.log`. Per-title-specific detection (TF2 binaries, Northstar markers, etc.) is intentionally kept out of Maxima — that's the consumer's job, since Maxima needs to remain universal across EA titles.
- **`Mode::Install { slug, path, build_id?, replace_files, only_listed_files, json }`** — non-interactive install driver. Resolves `slug` against the EA library (same chain as `Mode::Launch` minus the unlinked-Steam passthrough), picks the live build (or `--build-id` override), optionally deletes a comma-separated list of files passed via `--replace-files` (relative to `--path`, with `..` segments rejected), then either:
  - **Default** — queues via `QueuedGameBuilder` + `install_now` and polls every second until `content_manager().current()` returns None. Re-downloads anything the size-only entry-state check marks `Borked`. Use for fresh installs and missing-file recovery.
  - **`--only-listed-files`** — bypasses `install_now` entirely. Pulls just the files named in `--replace-files` directly from the build's zip manifest via `ZipDownloader::download_single_file` (the same primitive `Mode::DownloadSpecificFile` uses), leaves every other file on disk alone. Designed for surgical replace ops like the Steam-CEG fix where the user has a working install except for a handful of corrupted/DRM-touched binaries. Without this flag, applying a Steam-CEG fix against a TF2 install re-downloads ~50% of the manifest because Steam-vs-EA distribution sizes legitimately differ for many files; with the flag, it's seconds and ~few MB.
- In `--json` mode emits JSONL on stdout: `{"event":"progress","percent":N,"build_id":"…"}` per tick (default) or `{"event":"progress","current_file":"…","files_done":i,"total_files":n}` (strict), terminator `{"event":"done","elapsed_secs":…,…}` on success or `{"event":"error","message":"…"}` on failure (also non-zero exit). Designed so Draconis can drive a real-time install progress bar without scraping log lines. Same install flow the interactive "Install Game" menu uses; this is just the headless CLI form.

#### Steam helpers — new module (`maxima-lib/src/steam.rs`)
- Lifted from `maxima-cli/src/main.rs` so the auth server can use it too. Contains:
  - `STEAM_GAMES` table (currently just TF2: app id `1237970` → `Origin.OFR.50.0001456`, `Titanfall2/Titanfall2.exe`).
  - `lookup_steam_game(steam_app_id)`, `lookup_steam_game_by_offer(origin_offer_id)` (reverse lookup, used by `auth_server`).
  - `resolve_steam_install_path(SteamGameEntry)` — Steam install discovery: registry (`HKLM\SOFTWARE\(Wow6432Node\)Valve\Steam\InstallPath`), then `Program Files (x86)\Steam` / `Program Files\Steam` defaults, then `libraryfolders.vdf` parse. **Windows only**; returns `None` on other targets (Wine builds use the cfg(windows) path).
  - `EA_OFFER_ID_PATTERN`, `STEAM_APP_ID_PATTERN` regexes.

#### Authorize HTTP server — new module (`maxima-lib/src/auth_server.rs`)
- Plain `tokio::net::TcpListener` + manual HTTP parsing (same pattern `core/auth/login.rs` uses for the OAuth callback — avoids pulling in `actix-web`).
- `GET /` → `200 OK` body `maxima-auth-server`. Bootstrap's liveness probe.
- `POST /authorize?offer_id=<id>` → resolve offer, refresh `.dlf` via `request_and_save_license`, return `200 OK {"status":"ok"}`. **Does not spawn the game** — that's the architectural distinction from `Mode::Launch`.
- Errors map to HTTP status: `400` missing offer_id, `401` not logged in, `404` offer not in library or install path not found, `502` upstream EA / library failure.
- Default port `13219`; override with `MAXIMA_AUTHORIZE_PORT`.

#### LSX server cooperation (`maxima-lib/src/core/mod.rs::Maxima::start_lsx`)
- Probes `127.0.0.1:<port>` synchronously with 200ms timeout before binding. If a server is already listening (e.g. `serve` in another window, or the UI), logs and returns without trying to bind.
- Without this, the bootstrap-spawned `maxima-cli launch` would also bind 3216 (under Wine this can race the existing `serve` listener and steal the game's connection).

#### LSX response handlers (`maxima-lib/src/lsx/`)
- **`request/license.rs`** — `playing()=None` case no longer panics on `unwrap()`. Returns an empty `attr_License` so the game falls back to its on-disk `.dlf` (which `/authorize` deposited just before TF2's polling reconnected).
- **`request/profile.rs::handle_set_presence_request`** — `playing()=None` returns `ErrorSuccess` without trying to broadcast presence (catornot patch).
- **`request/profile.rs::handle_profile_request`** — `attr_IsSubscriber` / `attr_IsSteamSubscriber` reflect `env::var("SteamAppId").is_ok()`. (Empirical: toggling this didn't fix the File-corruption symptom; left in because it's at least less wrong than hardcoded `false` when running under Steam.)
- **`request/challenge.rs`** — captures `Version` and `Title` from the client's challenge response into `ConnectionState`.
- **`request/game.rs::handle_all_game_info_request`** — `InstalledVersion` / `AvailableVersion` / `DisplayName` echo the captured Challenge values (fallback to upstream's hardcoded `1.0.1.3` / `Titanfall® 2 Deluxe Edition` if Challenge hasn't arrived yet). `EntitlementSource` is still hardcoded `"STEAM"` — see "Pending code quality items" below.
- **`request/progressive_install.rs`** — echoes `attr_ItemId` from the request instead of hardcoded `Origin.OFR.50.0001456`.
- **`connection.rs::Connection::new`** — accepts connections when `maxima.playing()=None` instead of rejecting with `LSXConnectionError::GameContext`. Ported from `catornot/Maxima@patch-external-lsx` (upstream PR #42 by p0358). PID lookup / Kyber injection is skipped in that branch since there's no `ActiveGameContext` to read from.

#### Launch & env vars (`maxima-lib/src/core/launch.rs`)
- `LaunchOptions.steam_launch: bool` flips `EAEntitlementSource` / `EAExternalSource` / `EALaunchOwner` between `"EA"` and `"Steam"`. (Empirical: didn't fix File-corruption either; kept in because it's at least consistent with the surrounding env when launching via Steam.)
- `LaunchMode::Offline(_)` implemented (was `todo!()`). Looks up the offer, requires `path_override`, sets `EALaunchOfflineMode=true`. Draconis doesn't expose this yet.
- `path_override` skips `offer.is_installed()` (covers Steam-installed games EA Desktop has no record of).
- `LaunchMode::OnlineOffline(_)` now calls `needs_license_update()` before re-requesting, matching the `Online` branch. From upstream `fix/license-update-online-offline`.

#### Auth / login (`maxima-lib/src/core/auth/login.rs`)
- `begin_oauth_login_flow` uses `tokio::select!` between the TCP listener and stdin. Users whose browser can't emit `qrc://` (macOS Safari blocking custom URL schemes, Wine-bottle browsers without registered handlers, etc.) can paste either a full OAuth redirect URL or just a `remid` cookie value and the flow extracts the auth code via a redirect probe.
- Multi-line on-screen hint walks the user through copying the `remid` cookie from EA's DevTools storage.

#### Wine / Windows-side (`maxima-lib/src/unix/wine.rs`, `util/registry.rs`)
- `setup_wine_registry()` adds a bare `HKLM\Software\Origin` key (without `Electronic Arts\` prefix) that some EA titles check.
- `regedit` runs with `/S` (silent) so it doesn't block on a confirmation dialog under Wine.
- stderr is piped and concatenated into `WineError::Command` output instead of being swallowed.
- (Intentionally **not** taken from upstream `fix/wine-registry-setup`: the part that *disabled* `link2ea`/`origin2` protocol registration. We need them.)

#### DLL injector (`maxima-lib/src/util/dll_injector.rs`)
- `GetModuleHandleA` / `LoadLibraryA` → `GetModuleHandleW` / `LoadLibraryW` with UTF-16. Fixes injection on non-ASCII install paths. Ported from upstream `fix/non-ascii-characters`. Windows-only file; benefits native Windows users equally.

#### Logging (`maxima-lib/src/util/log.rs`)
- `init_logger_named(name)` variant — names the per-process log file (`maxima-cli.log` vs `maxima-bootstrap.log`).
- All logger output is mirrored to a file in addition to stdout. Default: `%LOCALAPPDATA%\Maxima\Logs\<name>.log` on Windows, `$XDG_DATA_HOME/maxima/logs/<name>.log` on unix. Override via `MAXIMA_LOG_FILE`. Each session writes a `===== maxima log session opened (pid=…) =====` header.
- `set_stdout_suppressed(bool)` — runtime toggle that drops stdout output from the logger while keeping the file sink intact. Set by `maxima-cli` immediately after `Args::parse()` when a `--json` subcommand is detected, so JSON output on stdout stays parseable. The ANSI-support warning was also moved from `println!` to `eprintln!` so it never lands on stdout even before suppression kicks in.

#### UI runtime (`maxima-ui`)
- **Renderer switched glow → wgpu** ([maxima-ui/Cargo.toml](maxima-ui/Cargo.toml), [maxima-ui/src/main.rs](maxima-ui/src/main.rs)). Root cause: eframe 0.28's glow path asks glutin for an OpenGL 3.3 Core context without `WGL_CONTEXT_FORWARD_COMPATIBLE_BIT_ARB`, which Wine's `macdrv` rejects ("OS X only supports forward-compatible 3.2+ contexts" → `ERROR_INVALID_VERSION_ARB`). Glutin then tries GLES fallback, but Wine's CrossOver build doesn't expose `WGL_EXT_create_context_es_profile` and `EGL not compiled in!`. eframe 0.28 doesn't expose a knob to set the forward-compat flag, so the cleaner path is wgpu. Added `"wgpu"` to eframe features and set `renderer: eframe::Renderer::Wgpu` in `NativeOptions`. wgpu picks Vulkan via MoltenVK 1.2.10 on Apple Silicon. **The custom glow renderers (`AppBgRenderer`, `GameViewBgRenderer`) auto-disable** because their constructors early-return `None` via `cc.gl.as_ref()?`, and all call sites are `if let Some(...)`. Background gradients disappear silently on macOS; the rest of the UI works. Could be upstreamed.
- **Swapchain nudge workaround for wgpu+MoltenVK 1.2.10** ([maxima-ui/src/main.rs](maxima-ui/src/main.rs)). MoltenVK creates the initial swapchain in `VK_SUBOPTIMAL_KHR` and renders black until something forces a swapchain recreate (a window resize works). Workaround: send `ViewportCommand::InnerSize(current + 1px, 0)` on the first `update()` call, tracked by a `swapchain_nudged: bool` field on `MaximaEguiApp`. UI shows content from frame 0 on. Harmless on non-Wine targets (1px resize at startup is invisible). macOS/CrossOver-specific in motivation but applied unconditionally to keep things simple.
- **Busy-loop fixes** ([maxima-ui/src/bridge_thread.rs:412](maxima-ui/src/bridge_thread.rs:412), [maxima-ui/src/ui_image.rs:213](maxima-ui/src/ui_image.rs:213)) — addresses upstream issue [#41](https://github.com/ArmchairDevelopers/Maxima/issues/41) (~200% CPU at idle). Both threads called `try_recv()` in a tight loop with no sleep on `Empty`, pegging two cores. Fix: added `tokio::time::sleep(5ms)` / `(10ms)` on the `Empty` branch and a proper `break` on `Disconnected` (previously also looped forever post-shutdown). Idle CPU drops from ~200% to single digits. Upstreambar.
- **Central panel background fix** ([maxima-ui/src/main.rs:539](maxima-ui/src/main.rs:539)). Upstream set `panel_frame.fill = Color32::RED` on the `CentralPanel`, relying on `AppBgRenderer` to paint a gradient on top and mask it. With wgpu the glow-only `AppBgRenderer::new(cc)` returns `None` (no GL context), so the raw red showed through and the UI looked like a placeholder error screen. Changed to `Color32::TRANSPARENT` so the underlying `window_fill` (black, configured in `Visuals`) shows. Clean dark UI under wgpu, no behaviour change under glow.
- **Friend-presence event dedup** ([maxima-lib/src/rtm/client.rs:81](maxima-lib/src/rtm/client.rs:81), [maxima-ui/src/event_thread.rs](maxima-ui/src/event_thread.rs)). Upstream `EventThread::run` looped every 500ms and, for **every** friend in the moka cache, emitted a `FriendStatusResponse` event **and** called `request_repaint()` inside the loop — even when the presence hadn't changed. With 16 friends online that's ~32 forced repaints per second when nothing is changing, which keeps the UI rendering continuously and burns CPU/GPU. Fix: derive `PartialEq, Eq` on `RichPresence`, keep a `HashMap<String, RichPresence>` of last-emitted presence, only emit + repaint when the new presence differs from the cached one. Idle-with-friends-online drops to **0 repaints per second** from the event thread. Upstreambar.

#### Env-driven overrides
- `MAXIMA_DENUVO_TOKEN` — short-circuits `RequestLicense` in the LSX handler and returns this token verbatim. Useful for offline debugging.
- `MAXIMA_LSX_PORT` — overrides the LSX listen port (default 3216).
- `MAXIMA_AUTHORIZE_PORT` — overrides the authorize HTTP port (default 13219).
- `MAXIMA_LOG_FILE` — overrides the file logger destination.
- `MAXIMA_DISABLE_WINE_VERIFICATION` — skips the Wine / runtime version check at startup.

### 3. Removed from upstream
- The original AppleScript-based macOS helper. Replaced by `MaximaHelper/Sources/main.swift`.
- Stale `todo.md` / `changes.md` tracking files.

---

## End-to-end flow (concrete walkthrough, Draconis vanilla + Steam-installed TF2)

This is the **currently recommended** flow on macOS/CrossOver. Use Path A from "Current architecture: two launch paths" above as the reference; this is the concrete instantiation.

```
1. User clicks Launch in Draconis (vanilla mode).
2. Draconis runs Titanfall2.exe directly via cxstart --bottle "Titanfall 2".
   (For Northstar mode Draconis runs `steam.exe -applaunch 1237970
   -northstar` instead; the same authentication chain still applies
   once Steam starts the game with the Northstar hooks loaded.)
3. Titanfall2.exe starts. Its Origin DRM stub checks for a running EA
   launcher. None found, so it emits the protocol URL:
     link2ea://launchgame/Origin.OFR.50.0001456?platform=PCWIN&theme=tf2
   TF2 then begins polling 127.0.0.1:3216 (its hardcoded LSX port) and
   stays alive until something answers.
4. Wine routes the link2ea:// URL to maxima-bootstrap.exe.
5. maxima-bootstrap parses the URL, validates the offer_id shape, then:
     5a. Probes 127.0.0.1:13219 (auth server). 200ms timeout.
     5b. If alive → POSTs http://127.0.0.1:13219/authorize?offer_id=…
         with a 60s timeout, then exits.
     5c. If dead → spawns `maxima-cli.exe launch Origin.OFR.50.0001456`
         (the upstream Path B behavior) and waits for it to finish.
6. (Path A) The running maxima-cli serve handles the authorize POST:
     - Confirms it's still logged in (auth_storage.logged_in()).
     - Confirms the offer is in the EA library.
     - Resolves the Steam install path for path_override
       (lookup_steam_game_by_offer + resolve_steam_install_path).
     - Calls launch::start_game(LaunchMode::Online(offer_id), …):
         · request_and_save_license → writes …/EA Services/License/
           <content_id>.dlf
         · sets EALsxPort / EAGenericAuthToken / EAAccessTokenJWS /
           EALaunchEAID / ContentId / … env vars
         · spawns bootstrap (Mode::Launch) which spawns Titanfall2.exe
           with that env
         · maxima.playing = Some(ActiveGameContext)
     - Returns 200 OK to the original bootstrap (the one that handled
       the link2ea:// URL). That bootstrap exits.
7. The newly-spawned TF2 has the full EA env and connects to LSX on
   127.0.0.1:3216 — serve's listener. Connection::new sees
   playing()=Some(ctx), takes the standard active-launch branch.
8. LSX handshake completes:
     Challenge → ChallengeAccepted (captures game version + title)
     GetConfig / GetProfile / GetSetting / GetGameInfo /
     GetAllGameInfo / IsProgressiveInstallationAvailable / …
     RequestLicense → real OOA fetch, returns Denuvo token.
9. TF2 has its license, has its LSX session, runs normally.
10. When the game eventually exits, serve's update_playing_status
    notices the bootstrap child returned, runs the cloud-save sync
    (if enabled and the offer has cloud saves), and clears
    maxima.playing. serve stays running for the next launch.
```

When `serve` is NOT running, step 5 takes branch 5c and a fresh `maxima-cli launch` process re-does the full bootstrap (login + LSX + launch::start_game). Same end state — the game is spawned with EA env vars — but every link2ea pays the full Maxima startup cost.

**Operationally: start `serve` before launching TF2.** Both paths end in `launch::start_game`; `serve` just amortizes login across launches.

---

## Why NorthstarLauncher.exe is *not* in the flow

`NorthstarLauncher.exe` in the TF2 directory **hard-codes a Win32 attempt to start Origin** (via a path to `Origin.exe`, not via `origin2://`). On macOS / Wine there is no Origin install, and our `origin2://` handler doesn't get a chance to intercept. Result: `[*] Starting Origin... [*] Waiting for Origin...` hangs forever.

Draconis works around this by launching Northstar mode via Steam's `-northstar` launch option (`steam.exe -applaunch 1237970 -northstar -noOriginStartup -multiple`), so Steam invokes `Titanfall2.exe` with the flag and Northstar's `wsock32` proxy hooks load. `NorthstarLauncher.exe` is never invoked.

If you want to fix Northstar to work standalone here, the right place is to make Northstar's "start Origin" step use `origin2://` (so maxima-bootstrap can catch it). That's an upstream Northstar issue, not Maxima's.

Credit to [catornot](https://github.com/catornot) for documenting the `-noOriginStartup` requirement and contributing the external-LSX patch in the first place.

---

## Signing gotcha (relevant when packaging MaximaHelper)

The upstream zipped `MaximaHelper.app` ships **linker-signed only**:

```
codesign -dv MaximaHelper.app
  CodeDirectory ... flags=0x20002(adhoc,linker-signed)
  Info.plist=not bound
  Sealed Resources=none
  Identifier=MaximaHelper_arm64                    ← not the real CFBundleIdentifier
```

LaunchServices **silently refuses to honor URL handler claims** from a bundle whose Info.plist isn't sealed into the signature. Draconis fixes this at build time by re-signing the cached helper:

```bash
codesign --force --deep --sign - MaximaHelper.app
# → Identifier=com.armchairdevelopers.maxima.helper
# → Info.plist entries=13, Sealed Resources files=1
```

If you ever change how `MaximaHelper.app` is signed at release time in this repo, make sure the final artifact is properly bundle-signed (not just linker-signed), or downstream `NSWorkspace.setDefaultApplication(at:toOpenURLsWithScheme: "qrc")` will silently no-op and `qrc://` will stay bound to whatever was registered before.

---

## CI

Two workflows. Both use **Rust nightly** (required by `#![feature(slice_pattern)]` in `maxima-ui/src/main.rs` and similar feature gates elsewhere — inherited from upstream).

### `build-ci.yml` — push CI

Fires on every push to any branch except `v*` tags. Matrix: Linux, Windows, macOS.

| Job             | What it builds                                                                                                          |
|-----------------|-------------------------------------------------------------------------------------------------------------------------|
| ubuntu-latest   | `cargo build --release --target x86_64-unknown-linux-musl -p maxima-cli -p maxima-bootstrap` (skips UI/TUI)             |
| windows-latest  | `cargo build --release` (full workspace → all 5 binaries), then `makensis /DBIN_DIR="..\target\release"`                |
| macos-latest    | `bash MaximaHelper/build.sh --output ./dist --no-register`, then sanity check that `Info.plist` declares `qrc://`        |

What CI does **not** validate:

- `maxima-ui` / `maxima-tui` on Linux — pull `rustix 0.37` via `accesskit_unix → zbus 3 → async-process 1.8 → async-io 1.13`, which doesn't build on modern nightly because of `rustc_attrs` namespace reservation.
- `MaximaSetup.exe` actually installing into a Wine bottle. We sanity-check size (>100KB) but never run it.
- `MaximaHelper.app`'s code signature — it ships linker-signed (adhoc) and Draconis re-signs it at consumption time with `codesign --force --deep --sign -`.

### `release.yml` — tag release

Fires on `v*` tags or `workflow_dispatch`. Three jobs:

1. **`build-helper`** (macOS) — builds `MaximaHelper.app`, sanity-checks layout + Info.plist, zips with `--symlinks`, uploads `MaximaHelper.zip`.
2. **`build-installer`** (Windows) — builds the full workspace, runs `makensis`, sanity-checks installer size > 100KB, uploads `MaximaSetup.exe` + a separate `maxima-binaries-win64` artifact with the loose `.exe`s.
3. **`release`** (Ubuntu) — downloads both artifacts and creates a non-prerelease GitHub release. Asset names are fixed: `MaximaHelper.zip` and `MaximaSetup.exe` (Draconis hardcodes these names in `Scripts/fetch-maxima-helper.sh` and `MaximaService.downloadAndInstall`, so do not rename).

### `block-upstream-pr.yml`

Trivial guard that fires on `pull_request_target` and fails if the PR base is `ArmchairDevelopers/Maxima`. Prevents accidentally sending fork-specific changes upstream.

---

## Diagnostics

### Is the helper registered for qrc:// on the host?

```bash
swift -e 'import AppKit; let u = URL(string: "qrc://probe")!; \
  print(NSWorkspace.shared.urlForApplication(toOpen: u)?.path ?? "NONE")'
```

Should print `/Applications/Draconis.app/Contents/Resources/MaximaHelper.app`. If not, Draconis's `registerHelper()` failed or another bundle is winning.

### Is the helper signature healthy?

```bash
codesign -dv /Applications/Draconis.app/Contents/Resources/MaximaHelper.app 2>&1 \
  | grep -E '(Identifier|Info.plist|Sealed Resources)'
```

Want to see `Identifier=com.armchairdevelopers.maxima.helper`, `Info.plist entries=13`, `Sealed Resources version=2`. If it says `Identifier=MaximaHelper_arm64` or `Info.plist=not bound`, the helper wasn't re-signed.

### Are there stale helper copies LS knows about?

`mdfind 'kMDItemCFBundleIdentifier == "com.armchairdevelopers.maxima.helper"'` only sees indexed paths. For the full LS view:

```bash
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$LSREG" -dump | awk '
  /^-{20}/{block=""; next}
  {block=block $0 "\n"}
  /claimed schemes:.*qrc:/{matches=matches block}
  END{print matches}
' | grep '^path:'
```

Common offenders: mounted `Draconis-vX.dmg` (`/Volumes/Draconis [N]/...`), Xcode `DerivedData/Draconis-*/Build/Products/Debug/Draconis.app`, ad-hoc unzips in `/private/tmp/MaximaHelper.app`. Draconis v0.3.7+ auto-unregisters these via `NSWorkspace.urlsForApplications(withBundleIdentifier:)` before calling `setDefaultApplication`.

### Is maxima-bootstrap actually being invoked?

Inside the bottle, maxima-bootstrap appends to `%TEMP%/maxima_execution.log` on every invocation. On a CrossOver bottle that's typically `~/Library/Application Support/CrossOver/Bottles/<bottle>/drive_c/users/crossover/Temp/maxima_execution.log`. If this file isn't growing when a launch is attempted, the protocol handler registration is broken and TF2's `link2ea://` is going nowhere.

### Is the auth server up? Did bootstrap forward?

When `serve` is running, the maxima-cli log file (`%LOCALAPPDATA%\Maxima\Logs\maxima-cli.log`) should contain `Authorize HTTP server listening on 127.0.0.1:13219`. When bootstrap forwards a request, the `maxima_execution.log` line is:

```
Forwarding link2ea offer=Origin.OFR.50.0001456 to auth server at http://127.0.0.1:13219/authorize?offer_id=…
Auth server accepted link2ea authorize for Origin.OFR.50.0001456 (body: {"status":"ok"})
```

If you see `No auth server on 127.0.0.1:13219; falling back to maxima-cli launch …` instead, `serve` isn't running (or it crashed) and bootstrap fell through to Path B.

### Quick port probe from inside the bottle

```cmd
:: Bottle PowerShell / cmd
Test-NetConnection 127.0.0.1 -Port 3216    :: LSX
Test-NetConnection 127.0.0.1 -Port 13219   :: Authorize HTTP
```

Or from the macOS host (works because Wine forwards ports to the host loopback):

```bash
nc -zv 127.0.0.1 3216
nc -zv 127.0.0.1 13219
```

### Capturing Wine debug logs in CrossOver

CrossOver's `cxstart` detaches Wine into its own process group, so stdout/stderr from the spawned binary do **not** reach your shell. Two gotchas you'll hit if you try the upstream-Wine recipes:

1. **`cxstart` is not in `$PATH` by default.** It lives at `/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin/cxstart`. Either export that dir into `$PATH` or use the full path.
2. **`WINEDEBUG` is overridden by CrossOver** with its own bottle-config default. The env var that actually wins is **`CX_DEBUGMSG`** (same syntax as `WINEDEBUG`: comma-separated `+channel` entries).

To capture full Wine traces from a detached cxstart-launched process, set `CX_LOG` (output file path) and `CX_DEBUGMSG` (channels):

```bash
export PATH="/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin:$PATH"

CX_LOG=/tmp/maxima.cxlog \
CX_DEBUGMSG=+wgl,+opengl \
cxstart --bottle "Titanfall 2" -- 'C:\Program Files\Maxima\maxima.exe'
```

Useful `CX_DEBUGMSG` channels for the kinds of bugs we hit here:
- `+wgl,+opengl` — WGL context creation, GL version info, extension list (used to diagnose the eframe glow failure).
- `+seh,+unwind` — exception/crash unwinding (already on by default in CrossOver's bottle env).
- `+module,+loaddll` — DLL load order, useful when an injection or LoadLibrary fails.
- `+process` — `GetEnvironmentVariable` traces, useful when something's reading or failing to read an env var.

For wgpu-internal logging, pass `RUST_LOG` via `cxstart --env`:

```bash
cxstart --bottle "Titanfall 2" --env "RUST_LOG=wgpu_core=info,wgpu_hal=info" -- 'C:\Program Files\Maxima\maxima.exe'
```

The maxima.log file inside the bottle is at `~/Library/Application Support/CrossOver/Bottles/<bottle>/drive_c/users/crossover/AppData/Local/Maxima/Logs/maxima.log` and gets a `===== maxima log session opened (pid=…) =====` header per process start.

### Steam vs vanilla launch contract (Draconis ↔ here)

Draconis v0.4.0+:

- Vanilla launch: runs `Titanfall2.exe` directly. The binary's own Steam DRM stub self-relaunches via `steam://run/1237970` if needed; the EA path triggers `link2ea://` which reaches maxima-bootstrap.
- Northstar launch: runs `steam.exe -applaunch 1237970 -novid -northstar -noOriginStartup -multiple`. Steam routes through TF2, the Northstar hooks load, EA auth still goes via link2ea:// → maxima-bootstrap.

Draconis never calls `maxima-cli.exe` directly. If you see `maxima-cli launch 1237970` in any log, it's from an old Draconis (≤ v0.3.9) — they shouldn't exist in v0.4.0+ flows.

---

## Release flow for this repo

Draconis pulls the latest release of this fork at build time via `Scripts/fetch-maxima-helper.sh`:

```
GET https://api.github.com/repos/AA-EION/Maxima-Draconis/releases/latest
→ download MaximaHelper.zip asset
→ unzip into Draconis/Resources/MaximaHelper.app
→ codesign --force --deep --sign - to seal the Info.plist
→ xcodegen + xcodebuild bundles it into Draconis.app
```

A new MaximaHelper release flows into the next Draconis build automatically as long as the assets are named `MaximaHelper.zip` and `MaximaSetup.exe`.

Tag the release as `vX.Y.Z` (lowercase v). The bottle installer is downloaded by Draconis on demand via `MaximaService.downloadAndInstall` — it fetches the latest release's `MaximaSetup.exe`, copies it into the bottle's `drive_c/windows/Temp/`, runs it silently with `/S`.

---

## Working on this repo

```bash
# Cross-compile a single binary for Windows
cargo +nightly build --release --target x86_64-pc-windows-gnu -p maxima-cli
cargo +nightly build --release --target x86_64-pc-windows-gnu -p maxima-bootstrap
cargo +nightly build --release --target x86_64-pc-windows-gnu -p maxima-service

# Or build the full workspace (UI + TUI + lib + all)
cargo +nightly build --release --target x86_64-pc-windows-gnu

# Build the macOS helper
bash MaximaHelper/build.sh

# Cross-compile the full installer (mingw + nsis)
bash installer/build.sh                  # → installer/MaximaSetup.exe

# Quick cargo check (faster than build) — useful during refactors
cargo check --target x86_64-pc-windows-gnu -p maxima-lib -p maxima-cli -p maxima-bootstrap
```

Anything that affects the Draconis integration — protocol handler registration, offer_id resolution, Info.plist contents in MaximaHelper, `MaximaSetup.exe`'s install location — is worth flagging in the release notes so Draconis can adapt.

---

## Upstream branch survey (as of 2026-05-14)

Evaluated all 14 upstream branches. Only these were complete and merged-ready:

| Branch | Status in this fork |
|--------|---------------------|
| `feat/license-token-override` | ✅ Already merged (commit `6ab4631`) |
| `fix/license-update-online-offline` | ✅ Already merged (commit `246bc53`) |
| `fix/non-ascii-characters` | ✅ Applied 2026-05-14 |
| `fix/wine-registry-setup` | ✅ Partially applied (registry additions + silent regedit; the part that disabled link2ea/origin2 was intentionally skipped) |
| `catornot/Maxima@patch-external-lsx` | ✅ Applied 2026-05-15, defensive coverage extended in `license.rs` on 2026-05-18 |

The remaining branches (`feat/server`, `feature/umu-launcher`, `feat/new-ci`, etc.) are either stale (6–20 months old), have unresolved conflicts, or are WIP with no clear completion signal. Do not merge them without a full review.

---

## Open issues

### "Engine Error: File corruption detected" after `IsProgressiveInstallationAvailableResponse`

**Status as of 2026-05-19 — RESOLVED.** Root cause confirmed empirically: **Steam CEG (Custom Executable Generation) signing on `Titanfall2.exe` / `Titanfall2_trial.exe` triggers Wine's `ntdll-Junction_Points` patch path during runtime validation**, surfacing in-game as the generic "File corruption" dialog. **Fix shipped in v0.11.0** via `maxima-cli install --replace-files "Titanfall2.exe,Titanfall2_trial.exe" --only-listed-files` against the Steam install dir — replaces just the two CEG-signed launcher binaries with the EA originals (~3 MB download, <60 s, leaves the rest of the install untouched). Game then runs end-to-end through the full LSX flow (`GetProfile` → `QueryEntitlements` → `SetPresence` → Main Menu). See the "Update 2026-05-19 (later) — root cause" and "Update 2026-05-19 (CEG fix confirmed end-to-end)" sub-sections below for the diagnostic trail and the final confirmation. The investigation narrative is preserved below for context.

**Symptom.** When `maxima-cli launch` (Path B) is the LSX server for a Steam-launched TF2, the game completes Challenge → ChallengeAccepted → GetConfig → GetProfile → GetSetting → GetGameInfo → GetAllGameInfo → IsProgressiveInstallationAvailable, then closes the LSX connection and shows the "File corruption detected" engine error. Never reaches `RequestLicense` or `GetAuthCode`.

**What was ruled out (toggled, symptom unchanged).**
- `IsSubscriber=false` ↔ `true`
- `IsSteamSubscriber=false` ↔ `true`
- `InstalledVersion="0"` ↔ real version captured at Challenge
- `IsProgressiveInstallationAvailableResponse.ItemId` hardcoded ↔ echoed
- `EAExternalSource="EA"` ↔ `"Steam"` env var
- Northstar `wsock32.dll` proxy removed
- ItemId echoed vs hardcoded

**First mitigation attempt (early Session 2026-05-18) — abandoned.** The original `/authorize` was preflight-only (refresh `.dlf`, no game spawn) on the theory that TF2 stayed running after emitting link2ea. Empirically TF2 *exits* and waits for the launcher to re-spawn it, so this design produced a new symptom: "TF2 opens for a moment and then closes" without ever reaching the file-corruption point. Reverted.

**Current design (late Session 2026-05-18).** `/authorize` calls `launch::start_game` — same code path the upstream UI's "Play" button takes. The flow ends up identical to Path B from the EA-side perspective (full env vars, license preflight, `maxima.playing=Some(ctx)`, standard active-launch LSX branch), just with cached login state.

**The user has not yet confirmed this resolves the file-corruption symptom in TF2.** Pending feedback. If the symptom returns, it's the same root cause we were debugging before (LSX response is not the issue — toggling the response fields didn't help in earlier sessions).

**Remaining hypotheses, in rough order of plausibility.**
1. **Steam DRM IPC.** TF2's Steam wrapper calls `SteamAPI_Init()` which needs `steam.exe` running and a valid Steam session. If Steam isn't actively running, init fails and TF2 reports the failure as file-corruption (known misleading error). The "UI is open" baseline that the user reports works on Windows might just be coincident with them having Steam running for unrelated reasons. Verify by checking the bottle's process list for `steam.exe` during a working vs failing run.
2. **`.dlf` mismatch via hardware-hash.** When Path B runs `request_and_save_license` with `playing=Some`, the OOA license is bound to a hardware hash computed inside maxima-cli's process. If TF2's own internal validation computes a different hash (different process-time WMI reads, version-2 vs version-4 hash composition), the `.dlf` signature won't validate. Path A also calls `request_and_save_license`, but the LSX side returns an empty token under `playing()=None` so TF2 isn't told to validate via LSX. Validate by exporting `MAXIMA_DENUVO_TOKEN` to short-circuit license fetch entirely.
3. **A local file check tied to a missing registry / file artifact.** Possibly `C:\Program Files (x86)\Origin Games\Titanfall2\__Installer\` or some EA-Desktop-only marker file. Not investigated.

### Update 2026-05-18 (later) — Origin in-game login window + still corrupting

Once the bootstrap → /authorize → launch::start_game chain was wired end-to-end (with the OPAQUE→JWS fallback below), the user reports:

- TF2 actually launches now (no more "opens for a moment and closes").
- TF2 then shows the **in-game Origin login window** (the deprecated EA launcher's embedded SSO prompt) asking for credentials.
- After logging in with EA credentials, TF2 proceeds and shows "Engine Error: File corruption detected" — same symptom as before.

This is real progress: the LSX flow now completes the Challenge handshake (`Game Connected - Name: Titanfall2, Offer ID: Origin.OFR.50.0001456, Multiplayer Id: 1039093, Version: 9.12.1.3` lands in the log). What we don't yet know is which subsequent LSX request triggers the corruption error — the LSX request/response logs were `debug!` so they're invisible at default INFO level.

Two distinct issues now:

**Issue A — embedded Origin login window appears.** TF2's Origin DRM stub doesn't accept our SSO env vars (`EAGenericAuthToken` / `EAAccessTokenJWS` / `EALaunchUserAuthToken`) and falls back to its built-in login dialog. Root cause: EA's `nucleus_auth_exchange` rejects our JWS→OPAQUE swap with a redirect to `signin.ea.com/p/juno/login?fid=…` (treated as `AuthError::InvalidRedirect`). We added a `match`-with-fallback in `launch::start_game::LaunchMode::Online` so we pass the JWS access token through as `EALaunchUserAuthToken` instead of bailing — that's the pre-PR-#34 upstream behavior and it lets the launch proceed. The cost is that TF2's Origin SDK doesn't trust the JWS as if it were OPAQUE and shows its own login. Manual login through that window works as a workaround.

Likely root cause of the OPAQUE rejection: EA's auth service wants a session cookie from a recent SSO flow (which EA Desktop carries from its embedded browser). Our reqwest client is cookie-less and stateless, so EA treats the exchange as untrusted. Fixing this properly would require either persisting EA cookies across `maxima-cli` runs or pre-fetching the OPAQUE token at login time and caching it.

**Issue B — "File corruption" after manual Origin login.** Same symptom as the prior session. Diagnostic this session: promoted the `Received/Queuing LSX Message` logs from `debug!` → `info!` in `lsx/connection.rs`, and changed `service.rs::"LSX connection closed"` to include the underlying error. The next test should produce a full LSX request/response trace + a real close reason, so we can see precisely which LSX request TF2 sends last before disconnecting.

Pending validation steps (next session):

1. **Capture the full LSX trace** — re-run with the latest binaries; the log should now show every request/response in `maxima-cli.log`. We expect to see the same sequence the prior session documented ending at `IsProgressiveInstallationAvailable`, or possibly stopping at an earlier request now that the EA env-var context is different.
2. **`MAXIMA_DENUVO_TOKEN` test** — set the env var on the `serve` process before invoking and re-launch TF2. If the symptom disappears, it's `.dlf` hash. If it persists, it's something else (Steam DRM IPC or local file integrity).
3. **Steam-running test** — open `steam.exe` inside the bottle (just the client UI) before clicking Play on TF2. If TF2 then works, Steam DRM IPC is the root cause.

Do not delete this section until the user confirms TF2 runs end-to-end.

**Next diagnostic steps if Path A doesn't fix it:**
1. Check that `…/EA Services/License/Origin.OFR.50.0001456_<...>.dlf` exists and is non-empty after a `serve` + Steam launch attempt.
2. Try the same launch with `MAXIMA_DENUVO_TOKEN=<anything>` set on the `serve` instance.
3. Diff the LSX trace from a working Windows session against the failing macOS session, especially `GetAllGameInfoResponse` fields.

### Update 2026-05-19 — `maxima-ui` install-then-launch bypasses the symptom

A meaningful data point: when TF2 is **installed via `maxima-ui`** (its own download path, custom install dir, not Steam's `steamapps/common/Titanfall2`) and then launched **directly by Maxima** (no Steam involvement at all), TF2 starts and runs without ever showing the "File corruption" error.

What's different about the install-via-UI path compared to Steam-installed-then-Maxima-auth:

| Aspect | Steam install + Steam launch + Maxima auth | Maxima-UI install + Maxima launch |
|--------|--------------------------------------------|-----------------------------------|
| Who runs `Titanfall2.exe` | `steam.exe -applaunch 1237970` → Steam wrapper → TF2 | `maxima` (via bootstrap) directly |
| `SteamAppId` env var | Set (1237970) | Unset |
| `SteamClientLaunch` env var | Set (1) | Unset |
| `EAExternalSource` / `EAEntitlementSource` | `Steam` | `EA` |
| Steam DRM IPC (`SteamAPI_Init`) | TF2 invokes it during boot | TF2 doesn't take the Steam DRM code path |
| Result on macOS/CrossOver | "Engine Error: File corruption detected" | TF2 runs |

The simplest reading: **the corruption symptom is rooted in TF2's Steam DRM IPC failing under Wine, not in anything Maxima does on the LSX/EA-auth side.** "File corruption" is the engine's misleading default error message when `SteamAPI_Init()` returns a failure (this is a well-known pattern in EA-on-Steam titles — the engine maps Steam init failures to a generic file-integrity error).

This is consistent with the earlier negative results: toggling `IsSubscriber`, `IsSteamSubscriber`, `EntitlementSource`, `EAExternalSource`, hardcoded vs echoed `ItemId`, etc. never moved the symptom, because all of those live on the LSX side, *after* TF2 has already failed its Steam init.

Implications:
- A user with TF2 only on Steam can side-step the bug by **re-downloading via `maxima-ui` to a non-Steam path** and launching that copy through Maxima. Lose Steam-cloud-saves and the launch-from-Steam-library UX, but get a working TF2 on macOS/CrossOver.
- A proper fix for the Steam-launched case would need to make `SteamAPI_Init` succeed under CrossOver Wine. That's a Wine/`steam.exe`/IPC problem, not a Maxima one.
- The catornot patch and the split-brain `serve` architecture were probably never the bottleneck — they just couldn't fix something happening *before* Maxima got a chance.

Caveats:
- Only "TF2 boots and shows the main menu" was confirmed. Multiplayer servers and Northstar interaction not validated through the maxima-UI install path.
- The `maxima-cli launch` path also sets Steam env vars when the slug looks numeric ([maxima-cli/src/main.rs](maxima-cli/src/main.rs)); a `--no-steam` flag (or unconditionally using the `Origin.OFR.…` offer-id path) would let users launch Steam-installed TF2 via CLI without invoking Steam IPC. Pending empirical test (the user uninstalled the Steam-TF2 bottle before we could confirm).

Do not delete this section until the user confirms TF2 launches reliably end-to-end.

### Update 2026-05-19 (later) — root cause: Steam CEG + Wine `ntdll-Junction_Points`

Final empirical test: `maxima-cli launch Origin.OFR.50.0001456 --game-path "C:\Program Files (x86)\Steam\steamapps\common\Titanfall2\Titanfall2.exe"` against the Steam install. LSX flow proceeds exactly through the same stop point documented in the original symptom — Challenge → ChallengeAccepted → GetConfig → GetProfile → GetSetting → GetGameInfo → GetAllGameInfo → IsProgressiveInstallationAvailable → connection closed → "File corruption detected".

Compared to the **maxima-ui-installed** copy (no Steam involvement) which reaches `RequestLicense` → `GetAuthCode` → `QueryEntitlements` → `SetPresence` → game runs. Same Maxima, same `serve`, same LSX, same account, same EA env vars. **Only the binary differs.**

The smoking gun: [NorthstarProton's `protonprep-valve-staging.sh`](https://github.com/R2NorthstarTools/NorthstarProton/blob/master/patches/protonprep-valve-staging.sh) explicitly disables the wine-staging patch `ntdll-Junction_Points` with the comment:

> `ntdll-Junction_Points - breaks CEG drm`

CEG (Custom Executable Generation) is Steam's per-user DRM. At install time Steam customizes `Titanfall2.exe` with a signature derived from the buyer's SteamID. At launch, the running exe validates that signature against the on-disk install via filesystem operations that the `ntdll-Junction_Points` wine patch breaks. When validation fails, the game emits a generic "File corruption" dialog and exits.

This explains every prior negative result:
- Toggling `IsSubscriber`, `IsSteamSubscriber`, `EntitlementSource`, `EAExternalSource`, hardcoded vs echoed `ItemId`, etc. never moved the symptom — those all live on the LSX side, **after** CEG fails internally.
- The catornot patch and the split-brain `serve` architecture didn't help — they couldn't fix something happening before any Maxima code runs.
- The `maxima-ui` install works because its binary is the EA original (no CEG signing) downloaded from Origin servers, not the Steam-custom-signed copy.

**What Maxima can't do.** The CEG validation runs inside `Titanfall2.exe` against Wine's `ntdll`, before any LSX call we control. There's no Maxima-layer hook that intercepts it.

**What Maxima now does (v0.8.0).** When `path_override` resolves to a path inside `steamapps\common\`, `launch::start_game` emits a `WARN` log explaining the CEG situation and recommending `maxima-ui` install. `--game-path` also now accepts a directory (resolves the exe via `STEAM_GAMES`), so users who hit the bug at least get a clean log line instead of a silent "Game stopped" exit.

**What would actually fix it.**
- CodeWeavers patching CrossOver to revert `ntdll-Junction_Points` for TF2 — needs upstream action.
- A Wine runtime without that patch (Proton works, but doesn't run on macOS).
- Stripping CEG from `Titanfall2.exe` — legally questionable and complex; not pursued.

The official Maxima-Draconis recommendation on macOS/CrossOver is therefore: **install via `maxima-ui` to a non-Steam path**, not via Steam.

### Update 2026-05-19 (CEG fix confirmed end-to-end)

**Steam install + targeted binary replace now works.** The hypothesis from the prior sub-section is empirically proven. Test from this session:

1. Steam install of TF2 at `C:\Program Files (x86)\Steam\steamapps\common\Titanfall2`. Northstar files alongside. Previously, every launch path produced the "File corruption" dialog after `IsProgressiveInstallationAvailable`.
2. Ran `maxima-cli install titanfall-2 --path "<above>" --replace-files "Titanfall2.exe,Titanfall2_trial.exe" --only-listed-files`. Took <60 seconds, downloaded ~3 MB from EA's CDN. Both Steam-signed CEG binaries replaced with the EA originals; **every other file in the Steam install left untouched** (Northstar's `wsock32.dll` proxy, `R2Northstar/`, `bin/`, `Core/`, etc. all preserved).
3. Ran `maxima-cli launch Origin.OFR.50.0001456 --game-path "<above>\Titanfall2.exe"`. **Game reached Main Menu.** Full LSX trace: Challenge → GetConfig → GetProfile → GetSetting → GetGameInfo → GetAllGameInfo → IsProgressiveInstallationAvailable → **`RequestLicense` → `GetAuthCode` → `QueryEntitlements` (9 entitlements returned) → `SetPresence` ("Main Menu", RTM updated) → `QueryFriends` (16 friends) → `GetInternetConnectedState` → second `GetAuthCode`** → user closed game cleanly → `Game stopped`.

This is the same LSX sequence the `maxima-ui`-installed copy produces. Same install dir as the broken Steam test, same env vars, same Maxima version — only those two binaries differed. CEG on the launcher exes is therefore both **necessary** (replacing them is sufficient to fix the symptom) and **sufficient** (no other file in the install needs touching) to cause/cure the corruption.

**Updated recommendation on macOS/CrossOver.** Both paths now work and Draconis will support both:

| Source | Maxima setup | Notes |
|---|---|---|
| Maxima-installed (any path) | install via `maxima-ui` or `maxima-cli install` | No CEG ever, simplest. |
| EA-Desktop-installed | `locate-game` + `launch` | No CEG. |
| Steam-installed | "Apply Maxima fix" → `maxima-cli install --replace-files "Titanfall2.exe,Titanfall2_trial.exe" --only-listed-files` | Surgical; keeps Steam install layout, Northstar files, save games. |
| Epic Games-installed | TBD (no test access yet) | Likely same pattern as Steam if Epic also CEG-signs. |

The Steam path is now first-class instead of "you should re-install via maxima-ui". Draconis's wizard CEG dialog will offer it as "Apply Maxima fix" and run the `maxima-cli install … --only-listed-files` invocation for the user.

---

## Pending code quality items

Tracked from PR #4 (Gemini review) and reaffirmed during the Session 2026-05-18 audit. Medium-priority. Address before publishing a release that other macOS users will rely on.

1. **Blocking I/O inside async** — `std::fs::read_to_string` on `libraryfolders.vdf` runs on a tokio worker without `spawn_blocking`. Low impact (VDF is small, ms-scale) but technically a yield-point hazard. Fix in `maxima-lib/src/steam.rs::resolve_steam_install_path`.
2. **Hardcoded Steam install fallback** — `C:\Program Files (x86)\Steam` and `C:\Program Files\Steam` are tried unconditionally after the registry. Should be removed once we trust the registry lookup is reliable inside Wine. `maxima-lib/src/steam.rs`.
3. **`attr_EntitlementSource` still hardcoded `"STEAM"`** — `GetAllGameInfoResponse` always returns `"STEAM"` regardless of launch path. Should reflect `LaunchOptions.steam_launch` (when known) or default to `"EA"` for non-Steam EA games. `maxima-lib/src/lsx/request/game.rs`.
4. **`SteamAppId` env var used as IPC** — `GetProfile` reads `std::env::var("SteamAppId")` to decide `IsSubscriber`. Cleaner: add a `steam_launch: bool` field to `ConnectionState`, populate it from `ActiveGameContext` at connection init (or from a per-request hint), and read it from `state`. `maxima-lib/src/lsx/connection.rs` + `request/profile.rs`.
5. **`Mode::Launch` and `Mode::Serve` coexistence** — when both `launch` and `serve` run simultaneously in the same bottle, `launch`'s `start_lsx` probe correctly defers to `serve`'s LSX, but it still spawns the game and sets `playing=Some(...)` on its own `maxima_arc`. The game's traffic still goes to `serve` (good), but `launch`'s state is then stale (`playing` set but no LSX traffic to update it). Cosmetic; not a correctness issue.
6. **No retry / health-check loop in bootstrap's forward path** — if `/authorize` returns a transient 5xx (e.g. EA license server hiccup), bootstrap surfaces the error directly. TF2 will keep polling LSX regardless, so a retry from the user side works, but a 1-retry in bootstrap would be friendlier.

---

## Known remaining gaps

- **`maxima-tui` / `maxima-ui`** — built and shipped in the installer; Draconis doesn't invoke them yet. **`maxima-ui` is functional on macOS/CrossOver as of 2026-05-19** (wgpu renderer + busy-loop fix; install + launch of TF2 validated end-to-end through the UI's own download path). Still missing: `/authorize` server wired up in the UI (would be a one-liner — call `start_auth_server()` alongside `start_lsx()` in `bridge_thread`); for now only `maxima-cli serve` provides the auth server. If we want a graphical "Maxima is running" indicator on macOS/CrossOver, that's the next step.
- **`origin2://` without an `offerIds` param** — the handler passes an empty string and the auth server returns 400. A better fallback (e.g. reading `productId`, or per-game hardcoded table) is a future improvement.
- **DLL injection on macOS / CrossOver** — `maxima-service`'s injector is Windows-only by design. Wine doesn't support `CreateRemoteThread`-style injection. The service is installed by NSIS but its injection path is never exercised in the Draconis flow.
- **Cloud saves, downloads, friends** — implemented upstream and present in the codebase, but untested in the Draconis / CrossOver configuration.
- **Offline mode after first launch** — `LaunchMode::Offline` path exists but Draconis doesn't expose it. License cache lives at `C:/ProgramData/Electronic Arts/EA Services/License/<content_id>.dlf` and is valid for approximately two weeks.
- **`STEAM_GAMES` table is TF2-only** — `lookup_steam_game(steam_app_id)` only has an entry for `1237970`. Other EA-on-Steam titles would not resolve via the fallback. Extend per title we validate.
- **No registry-driven UI-vs-CLI auth provider selector** — the user previously proposed `HKLM\Software\Maxima\AuthProvider = "UI"|"CLI"` that bootstrap would read when no auth server is running. Not implemented; the current fallback path simply spawns `maxima-cli launch` unconditionally. Becomes meaningful once we want bootstrap to auto-start `serve` if it can't find one running.
- **Auth-server endpoint not on UI yet** — `maxima.exe` doesn't bring up `/authorize`. If a user runs the UI without `serve`, bootstrap falls through to Path B (spawn). Easy fix; just hasn't been wired.
- **TF2's LSX-polling timeout, if any, is undocumented.** Path A relies on TF2 retrying indefinitely while bootstrap forwards. If TF2 has a finite timeout (we suspect it doesn't but haven't measured), `serve` cold-starts could miss the window.

---

## Operator recipes

### First-time setup in a fresh CrossOver bottle

```bash
# 1. Install MaximaSetup.exe inside the bottle (Draconis does this automatically;
#    if doing it by hand, copy the .exe from a release and run it).
#    The installer registers link2ea://, origin2://, qrc:// in Wine's registry
#    and drops the binaries in C:\Program Files\Maxima\.

# 2. On the macOS host, install / register MaximaHelper.app for qrc://. Draconis
#    does this with `codesign --force --deep --sign -` + `NSWorkspace`.

# 3. Inside the bottle, run maxima-cli once interactively to do OAuth login.
maxima-cli.exe
# → "Welcome to Maxima!" menu → Launch Game (any) → browser opens → log in →
#   redirect comes back via qrc:// → MaximaHelper forwards → :31033 captures the
#   auth code → token stored.
# (Or: paste a `remid` cookie value at the stdin prompt if the browser is stuck.)
```

After that the bottle has a persistent token. You never need to log in interactively again until it expires (months).

### Run `serve` and play

```bash
# Terminal 1 (inside the bottle):
maxima-cli.exe serve
# Expected console lines (and the same go to %LOCALAPPDATA%\Maxima\Logs\maxima-cli.log):
#   LSX server listening on port 3216
#   Authorize HTTP server listening on 127.0.0.1:13219
#   Subscribed to N friends for presence       (omit with --no-rtm)
#   Serving LSX. Launch your game externally; press Ctrl-C to stop.
```

Then launch TF2 any way you want:
- Draconis (vanilla or Northstar)
- Steam → Library → Titanfall 2 → Play
- `cxstart --bottle "Titanfall 2" -- "C:\\Program Files\\…\\Titanfall2.exe"`

When TF2 emits `link2ea://`, bootstrap forwards to the running `serve` and exits; TF2's LSX polling reaches `serve`'s listener; auth completes.

### Fallback: no `serve` running

`maxima-cli.exe launch Origin.OFR.50.0001456` (or any Steam App ID Maxima knows about, e.g. `1237970`) drops you into Path B. This is what bootstrap auto-spawns when `/authorize` doesn't answer.

---

## Changelog (most recent first)

History of significant changes since this fork was forked. Not a substitute for `git log` but useful for "when did X land" questions.

### 2026-05-19 (CEG fix shipped) — `install --replace-files` + `--only-listed-files`

Two new flags on `Mode::Install` that together implement the surgical Steam-CEG fix. Empirically validated 2026-05-19: a TF2 install from Steam (`steamapps\common\Titanfall2`) ran end-to-end through the LSX flow (`GetProfile` → `QueryEntitlements` → `SetPresence` → `QueryFriends` → game reaches Main Menu) after replacing **just `Titanfall2.exe` and `Titanfall2_trial.exe`** with the EA originals via this command. Same install dir, same env vars, same Maxima — only those two binaries differed. **CEG on the launcher exes is now confirmed as the sole root cause of the "File corruption detected" symptom on macOS/CrossOver**, and the fix is ~3-5 MB of download in <60 seconds, not the full ~30 GB re-install.

- **`--replace-files <p1,p2,...>`** on `Mode::Install` ([maxima-cli/src/main.rs](maxima-cli/src/main.rs)) — comma-separated list of file paths relative to `--path` that are deleted before the install runs. Validation: rejects entries with `..` segments, empty segments, or absolute paths. Skips entries that resolve to directories / symlinks (with a `warn!`). Missing entries are a no-op (`debug!` only).
- **`--only-listed-files`** — restricts the install to **only** the files in `--replace-files`, bypassing `install_now` entirely. Pulls each named entry from the build's zip manifest via `ZipDownloader::download_single_file` (the same primitive `Mode::DownloadSpecificFile` already uses) and leaves every other file on disk alone. Without this flag, applying a Steam-CEG fix would still re-download ~50% of the TF2 manifest (~15 GB) because the size-only `initial_state` check legitimately disagrees with Steam-packaged files on many entries; **with the flag, the fix is exactly two HTTP range requests against EA's CDN**.
- Logging: human mode logs `info!("Deleting <path> (replace-files)")` per delete, then `info!("Downloading <file> (i/n)")` per replace (strict mode); `--json` mode emits `{"event":"progress","current_file":"…","files_done":i,"total_files":n}` per file and a terminator `{"event":"done","files_replaced":[…]}` on success.

Working CEG-fix invocation against a Steam install:

```bat
maxima-cli.exe install titanfall-2 ^
  --path "C:\Program Files (x86)\Steam\steamapps\common\Titanfall2" ^
  --replace-files "Titanfall2.exe,Titanfall2_trial.exe" ^
  --only-listed-files
```

Why both flags shipped together: empirical evidence from a partial first-attempt test (without `--only-listed-files`) showed `install_now` re-downloading ~50% of the manifest against a Steam dir — Steam-vs-EA size mismatches across legitimate-but-differently-packaged files are common enough that "delete + re-install" alone isn't usable for the in-place CEG fix. The strict-mode flag isolates the actual remediation to just the CEG-touched binaries. The `--replace-files`-only mode is still useful for cases where the user genuinely wants a full re-install with specific files force-refreshed (e.g. corruption recovery on a Maxima-installed copy).

The downloader's CRC32 path remains commented out ([downloader.rs:316-329](maxima-lib/src/content/downloader.rs); `"We must be calculating the hash incorrectly or something"`). If/when that's fixed, the strict-mode flag remains useful as an "I know exactly which files to refresh, skip the verify entirely" optimization. Until then, it's the only way to do an in-place targeted replace without re-downloading half the game.

### 2026-05-19 (still going) — non-interactive `install` subcommand

Second step of the Draconis-side wizard rewrite (after the `list-games --json` work). Lets Draconis (or any script) trigger a Maxima install via CLI without the inquire-prompted interactive menu.

- **`maxima-cli install <slug> --path <abs_dir> [--build-id <id>] [--json]`** ([maxima-cli/src/main.rs](maxima-cli/src/main.rs)) — non-interactive install driver. Resolves slug → offer_id with the same chain `Mode::Launch` uses (`game_by_base_slug` → `game_by_base_offer` → exhaustive `games()` scan over slug/offer_id/content_id/product fields), minus the unlinked-Steam passthrough fallbacks (those only make sense for launching an already-installed copy). Picks the live build by default; `--build-id` overrides to a specific historical build. Queues via `QueuedGameBuilder` + `install_now`, then polls `content_manager().current()` every second until it returns None.
- In `--json` mode: emits one JSON document per line on stdout. Per-tick progress is `{"event":"progress","percent":N,"build_id":"…"}`; terminator on success is `{"event":"done","elapsed_secs":…,"offer_id":"…","build_id":"…","path":"…"}`; terminator on failure is `{"event":"error","message":"…"}` plus a non-zero process exit. Each line is flushed explicitly so a Draconis-style consumer with a piped stdout sees progress in real time. Logger stdout is auto-suppressed by `main()` when `--json` is set (via the `set_stdout_suppressed` toggle added in v0.9.0).
- In plain mode: emits the same `info!("Downloading: {}%/100%")` line per tick as the interactive flow, then `info!("Install complete in N.NNs — <path>")`. The interactive "Install Game" menu still uses its own inquire path — no behavior change there.

Limitation worth noting: the polling loop sees mid-install errors only via `install_now()`'s eventual error return — `consume_pending_events` is drained but not surfaced, because the upstream `ContentManager` API doesn't emit structured download-failure events we could forward as `{"event":"error",…}` lines. If a download fails halfway, the consumer gets an `event:error` line at the end (with the propagated error message) rather than mid-stream. Worth improving later when the content manager learns a `MaximaEvent::DownloadFailed` variant.

### 2026-05-19 (even later) — `list-games --json` for Draconis pre-flight

First step of the Draconis-side rewrite (full plan in chat history): give the SwiftUI launcher machine-readable detection without scraping log output.

- **`maxima-cli list-games --json`** ([maxima-cli/src/main.rs](maxima-cli/src/main.rs)) — emits a JSON array of every owned title with `slug`, `name`, `offer_id`, `content_id`, `installed`, `install_path`, `version`, `has_cloud_save`, and a nested `extra_offers` list (DLC/expansions). Used by Draconis to answer "does Maxima see TF2 in this user's EA library, and is it installed?". `--json` activates a logger-stdout suppression flag right after `Args::parse()` so the JSON document on stdout has no log-line noise; the file sink keeps capturing everything for debug.
- **`maxima-lib::util::log::set_stdout_suppressed(bool)`** — the runtime toggle that powers `--json` mode. Affects only the logger's stdout sink; file sink and `eprintln!` are unchanged. The ANSI-support fallback warning in `init_logger_named` moved from `println!` to `eprintln!` so it never corrupts a JSON stdout even before the suppression flag is set.

Per-title detection (Titanfall 2 binary names, Northstar markers, etc.) deliberately stays out of Maxima — `Mode::Inspect` was considered and rejected for the same reason the `-noOriginStartup -multiple` injection was removed in v0.7.0: Maxima needs to remain universal across EA titles. Draconis owns the TF2-specific detection logic using plain `FileManager` checks.

No behavior change to non-`--json` callers. `list-games` without the flag still prints the original `info!` table; bootstrap / serve / launch flows are untouched.

Three changes in `launch::start_game` and CLAUDE.md, in service of the same goal: make the macOS/CrossOver story honest about what Maxima can and can't fix.

- **`--game-path` accepts a directory.** [maxima-lib/src/core/launch.rs](maxima-lib/src/core/launch.rs) used to take `path_override` literally — if a user passed `…\steamapps\common\Titanfall2` (the install dir) instead of `…\steamapps\common\Titanfall2\Titanfall2.exe`, bootstrap silently failed to spawn (can't execute a directory) and the user saw a bare "Game stopped" with no error. Now the resolver detects a directory, looks up the exe name via `lookup_steam_game_by_offer` in the `STEAM_GAMES` table, and logs the resolved path. If the offer isn't in `STEAM_GAMES`, an explicit `error!` line tells the user to pass the full exe path.
- **Steam CEG warning.** When the resolved game path is inside `steamapps\common\`, `start_game` now emits a `WARN` log that names the root cause (Steam CEG + wine-staging's `ntdll-Junction_Points` patch) and points users to `maxima-ui` install. The warning fires regardless of host OS — on native Windows it's harmless (CEG works there); on macOS/CrossOver it's the heads-up that saves the next user from spending a week debugging LSX traces.
- **CLAUDE.md root-cause documentation.** The "Engine Error: File corruption detected" section now leads with the resolution (Steam CEG validation under Wine's `ntdll-Junction_Points` patch, intractable from Maxima's layer, workaround is `maxima-ui` install). A new "Update 2026-05-19 (later)" sub-section explains the evidence trail: matching LSX stop-point pattern between two installs, NorthstarProton's explicit `# ntdll-Junction_Points - breaks CEG drm` patch removal, and why all prior LSX-side hypotheses missed.

Validation: the path-override directory case now produces a clean `info!` line ("resolved exe to … via STEAM_GAMES") instead of the silent bootstrap failure. The warning fires correctly when path is in `steamapps\common\` (verified with both file and directory inputs).

### 2026-05-19 — drop TF2-specific launch-arg auto-injection from the universal path

`launch::start_game` no longer auto-inserts `-noOriginStartup -multiple` when `LaunchOptions.steam_app_id` is `Some(...)`. Those flags are TF2/Northstar/Source-engine-specific and were leaking into a path that's supposed to work for every EA-on-Steam title.

What's removed:
- The conditional block in [maxima-lib/src/core/launch.rs](maxima-lib/src/core/launch.rs) that prepended `-noOriginStartup` and `-multiple` to `game_args` whenever a Steam App ID was present.
- The matching point #4 in the `LaunchOptions.steam_app_id` doc comment.
- Stale references to the injection in [maxima-lib/src/auth_server.rs](maxima-lib/src/auth_server.rs) and [maxima-cli/src/main.rs](maxima-cli/src/main.rs).

What's kept (universally useful, not game-specific):
- `SteamAppId` / `SteamGameId` env vars on the spawned game — required by `SteamAPI_Init` for any EA-on-Steam title, without them the game exits with code 100010 "Steam not detected".
- `SteamClientLaunch=1` / `SteamPath` defaults.
- `EAEntitlementSource` / `EAExternalSource` / `EALaunchOwner` flipped to `"Steam"` when launched from a Steam context.

How callers supply the flags now:
- CLI: `maxima-cli launch <slug> --game-args -noOriginStartup --game-args -multiple`
- Env: `MAXIMA_LAUNCH_ARGS="-noOriginStartup -multiple"`
- Protocol: `link2ea://launchgame/<offer>?cmd_params=-noOriginStartup%20-multiple` (URL-decoded and split by [auth_server.rs::handle_authorize](maxima-lib/src/auth_server.rs))
- Draconis already passes both flags in its Northstar invocation (`steam.exe -applaunch 1237970 -novid -northstar -noOriginStartup -multiple`), so Draconis vanilla and Northstar flows are unaffected.

Why now: validated on 2026-05-19 that `maxima-cli launch Origin.OFR.50.0001456` against the `maxima-ui`-installed TF2 reaches the Main Menu cleanly (full LSX trace including `GetProfile`, `GetAuthCode`, `QueryEntitlements`, `SetPresence`, `QueryFriends`). The launch path doesn't need TF2-specific defaults; what TF2 needed for Northstar can come from the caller.

### 2026-05-19 — `maxima-ui` works on macOS/CrossOver: wgpu renderer + busy-loop fix

Three changes in `maxima-ui` to make the upstream graphical UI usable in a CrossOver bottle. All upstreambar; none require macOS-specific infrastructure.

- **Renderer: glow → wgpu** ([maxima-ui/Cargo.toml:12](maxima-ui/Cargo.toml:12), [maxima-ui/src/main.rs:112](maxima-ui/src/main.rs:112)). eframe 0.28's glow path asks Wine for an OpenGL 3.3 Core context, which `macdrv` rejects with `ERROR_INVALID_VERSION_ARB` ("OS X only supports forward-compatible 3.2+ contexts"). The GLES fallback also fails — Wine's CrossOver build doesn't expose `WGL_EXT_create_context_es_profile` and `EGL not compiled in!`. Added `"wgpu"` to eframe features and `renderer: eframe::Renderer::Wgpu` in `NativeOptions`. wgpu picks Vulkan via MoltenVK 1.2.10 on Apple Silicon. The custom `AppBgRenderer` / `GameViewBgRenderer` (glow-only) auto-disable via their existing `cc.gl.as_ref()?` early-return; all call sites are `if let Some(...)`, so background gradients silently disappear on macOS without UI errors.
- **Swapchain nudge for wgpu+MoltenVK** ([maxima-ui/src/main.rs](maxima-ui/src/main.rs)). MoltenVK 1.2.10 creates the initial swapchain in `VK_SUBOPTIMAL_KHR` and renders black until a swapchain recreate happens (user-initiated resize triggers one). Workaround: programmatic 1px `ViewportCommand::InnerSize` on the first `update()` call, tracked via `swapchain_nudged: bool` on `MaximaEguiApp`. UI shows content from frame 0 on. Harmless on non-Wine targets.
- **Busy-loop fixes** ([maxima-ui/src/bridge_thread.rs:412](maxima-ui/src/bridge_thread.rs:412), [maxima-ui/src/ui_image.rs:213](maxima-ui/src/ui_image.rs:213)) — addresses upstream issue [#41](https://github.com/ArmchairDevelopers/Maxima/issues/41). Both threads spun in tight `try_recv()` loops with no sleep on `Empty`, pegging two cores (~200% CPU at idle). Added `tokio::time::sleep` of 5ms (bridge) / 10ms (image) on the `Empty` branch, plus proper `break` on `Disconnected` (previously also looped forever post-shutdown). Idle CPU drops to single digits.
- **Central panel fill RED → TRANSPARENT** ([maxima-ui/src/main.rs:539](maxima-ui/src/main.rs:539)). Upstream relied on `AppBgRenderer` painting a gradient over the red placeholder; with wgpu that renderer is `None` and the red shows raw. Transparent fill lets the configured `window_fill` (black) show through.
- **Friend-presence event dedup** ([maxima-lib/src/rtm/client.rs:81](maxima-lib/src/rtm/client.rs:81), [maxima-ui/src/event_thread.rs](maxima-ui/src/event_thread.rs)). `EventThread::run` emitted a `FriendStatusResponse` and called `request_repaint()` for every friend in the moka cache every 500ms whether they'd changed or not — ~32 forced repaints/sec with 16 online friends. Derived `PartialEq, Eq` on `RichPresence`, cached previous presence per friend, only emit + repaint on diff. Idle event-thread repaints drop to 0.

Validation: installed TF2 end-to-end via `maxima-ui` on macOS/CrossOver (login → game list → install with custom path → wait for download → launch). TF2 ran. **First time `maxima-ui` has been verified to work on this fork's target.**

Diagnostics gotchas discovered during the debug session (now in the Diagnostics section):
- `cxstart` is not in `$PATH` — lives at `/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin/cxstart`.
- `WINEDEBUG` is overridden by CrossOver; **`CX_DEBUGMSG`** is the env var that actually wins.
- `CX_LOG` captures Wine traces even from detached cxstart-launched processes.

### 2026-05-18 — split-brain auth: bootstrap as router, `/authorize` as service (with launch)

The whole "Path A" infrastructure landed in this session, replacing the previous attempt where the bootstrap-spawned `maxima-cli launch` would try to coexist with `serve` and lose the LSX-port race under Wine.

- **New module `maxima-lib/src/auth_server.rs`** (~250 lines). Plain `tokio::net::TcpListener` + manual HTTP parse. `GET /` → liveness probe; `POST /authorize?offer_id=X[&cmd_params=…]` → call `launch::start_game` (license preflight + EA env vars + spawn game + set `maxima.playing=Some`). Default port 13219. Initially shipped as preflight-only (no spawn); reworked mid-session after empirical evidence showed TF2 exits after emitting `link2ea://` and needs to be re-launched, not just authenticated. Now aligned with upstream issue #27's design intent.
- **New module `maxima-lib/src/steam.rs`** (~180 lines). Lifted `STEAM_GAMES`, `lookup_steam_game`, `resolve_steam_install_path`, `EA_OFFER_ID_PATTERN`, `STEAM_APP_ID_PATTERN` out of `maxima-cli/src/main.rs`. Added `lookup_steam_game_by_offer` (reverse: `Origin.OFR.…` → entry) because `/authorize` receives offer IDs, not Steam App IDs.
- **`Maxima::start_auth_server`** in `maxima-lib/src/core/mod.rs`. Companion to `start_lsx`. Reads `MAXIMA_AUTHORIZE_PORT` for override.
- **`maxima-bootstrap/src/main.rs`** rewrite of `link2ea://` + `origin2://` handlers:
  - Deduplicated into a single `handle_protocol_authorize(offer_id, cmd_params, protocol_name)` helper.
  - Probes 127.0.0.1:13219 with `std::net::TcpStream::connect_timeout(200ms)`.
  - If alive: `reqwest::Client::post(http://…/authorize?offer_id=…&cmd_params=…)` with 60s timeout. 2xx → success, 4xx/5xx → surface as error (no fallthrough spawn — server made a deliberate decision).
  - If dead: spawn `maxima-cli.exe launch <offer_id>` (legacy Path B preserved).
  - New `log_event` helper writes structured lines to `%TEMP%/maxima_execution.log`.
- **`maxima-cli/src/main.rs::serve_lsx`** now calls `maxima.start_auth_server` after `start_lsx`. Best-effort: failure logs a warning and `serve` keeps going with LSX alone. The park loop ticks `maxima.update()` once per second so `update_playing_status` can detect game exit and run cloud-save sync.
- **`maxima-cli/Cargo.toml`** dropped `winreg` (only used by Steam helpers, now in `maxima-lib`).
- **`maxima-lib/Cargo.toml`** added `urlencoding`.
- **`maxima-bootstrap`** imports `AUTHORIZE_PORT` from `maxima-lib` instead of duplicating the constant.

### 2026-05-16 — `serve` mode + `start_lsx` probe + defensive license.rs

- **`Mode::Serve { no_rtm }`** subcommand added to `maxima-cli`. Long-running auth-only mode: logs in, starts LSX, optionally RTM, parks indefinitely.
- **`Maxima::start_lsx`** now probes `127.0.0.1:<port>` with a 200ms TCP timeout before binding. If a server is already listening it logs and returns without binding. Prevents the bootstrap-spawned `maxima-cli launch` from racing the existing `serve` for the LSX socket.
- **`maxima-lib/src/lsx/request/license.rs`** — `playing().as_ref().unwrap()` replaced with `let Some(playing) = … else { return empty-token }`. Mirrors the pattern `handle_set_presence_request` already had since the catornot patch. With this, externally-launched games (Steam direct, Northstar) that hit `RequestLicense` get a graceful empty-token response instead of crashing the spawned LSX task.
- Added `maxima-cli serve` operator recipe to CLAUDE.md.

### 2026-05-15 — Steam App ID launch support + LSX response fixes (PR #4)

- **Bootstrap** — accept Steam App IDs (pure numeric) in addition to `Origin.OFR.<digits>.<digits>`. Previously rejected, so Steam-launched titles silently no-op'd.
- **`maxima-cli`** — `STEAM_GAMES` table maps Steam App ID → Origin offer ID + install subdir; auto-discovers Steam install via registry + `libraryfolders.vdf`; sets `SteamAppId` / `SteamGameId` / `SteamClientLaunch` / `SteamPath` env vars; auto-injects `-noOriginStartup -multiple` launch args.
- **`launch::start_game`** — skip `offer.is_installed()` check when `path_override` is supplied. Adds conditional `"Steam"` vs `"EA"` for `EAEntitlementSource` / `EAExternalSource` / `EALaunchOwner`.
- **`GetAllGameInfoResponse`** — captures real `Version` and `Title` from the LSX Challenge handshake (was hardcoded `"0"` / `"1.0.1.3"` / `Titanfall® 2 Deluxe Edition`).
- **`GetProfileResponse`** — `attr_IsSubscriber` / `attr_IsSteamSubscriber` reflect `env::var("SteamAppId")` presence.
- **`IsProgressiveInstallationAvailableResponse`** — echoes the request's `attr_ItemId` instead of hardcoded TF2 offer.
- **`handle_set_presence_request`** — graceful no-op when `playing()=None` (catornot patch, applied here).
- **`Connection::new`** — accepts external LSX connections (catornot patch).
- **Bootstrap exit codes** — non-zero exits from `maxima-cli` now propagate as errors instead of being logged as "Success".

### 2026-05-14 — console visibility, NSIS registry view, full installer (PRs #1–#3)

- **`maxima-cli`** — `AllocConsole()` + `SetStdHandle("CONOUT$" / "CONIN$")` so the CLI is actually visible when bootstrap (GUI subsystem) spawns it. Panic hook to `%LOCALAPPDATA%\Maxima\Logs\maxima-cli.panic.log`. Plain `fn main()` + manual tokio runtime so the panic hook is installed before anything fallible. `init_logger_named` for per-binary log filenames.
- **`maxima-bootstrap`** — `link2ea://` URL parsing implemented (was `todo!()`); `origin2://` reads real `offerIds` from URL (was hardcoded BF2 offer); `qrc://` no longer panics on missing marker; offer-id shape validation defends against `--login=` flag injection.
- **`maxima-lib/src/util/log.rs`** — always-on file sink in addition to stdout; default `%LOCALAPPDATA%\Maxima\Logs\<binary>.log`.
- **NSIS installer** (`installer/maxima-setup.nsi`) — full rewrite. `SetRegView` properly reset before HKCR writes (avoids 32-vs-64-bit view collision). `BackupProtocol` guards against backing up Maxima's own values. Cross-compiled via `mingw-w64` + `nsis` from macOS.
- **CI** — `build-ci.yml` matrix expanded to Linux+Windows+macOS; Linux restricted to `-p maxima-cli -p maxima-bootstrap` (UI/TUI excluded due to rustix 0.37 incompatibility on nightly); Windows builds full workspace + NSIS. `release.yml` builds helper on macOS + installer on Windows + assembles GitHub release on Ubuntu.
- **`maxima-lib/src/util/dll_injector.rs`** — `GetModuleHandleW` / `LoadLibraryW` + UTF-16 paths (from upstream `fix/non-ascii-characters`).
- **`maxima-lib/src/unix/wine.rs`** — bare `HKLM\Software\Origin` registry entry, `regedit /S`, stderr captured.
- **`maxima-lib/src/core/launch.rs`** — `OnlineOffline` mode calls `needs_license_update` (from upstream `fix/license-update-online-offline`); `LaunchMode::Offline` implemented (was `todo!()`).
- **`maxima-lib/src/lsx/request/license.rs`** — `MAXIMA_DENUVO_TOKEN` env override (from upstream `feat/license-token-override`).
- **`maxima-cli launch`** — Steam-only owner passthrough (warn + try anyway when slug matches `Origin.OFR.<digits>.<digits>` but EA library doesn't know it); `GetGameBySlug` subcommand body restored (was a no-op stub upstream).

### Earlier — initial fork

Native Swift `MaximaHelper.app` replacing the upstream AppleScript helper; NSIS installer cross-compiled via mingw + nsis; release CI; PR template + upstream-PR guard; CLAUDE.md / README scope-narrowed to macOS/CrossOver + TF2.
