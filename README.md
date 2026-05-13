<p align="center">
  <img src="images/logo.png" width="120" alt="Maxima logo" />
</p>

<h1 align="center">Maxima</h1>

<p align="center">
  A free, open-source replacement for the EA Desktop Launcher.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/macOS-via%20CrossOver%20%2F%20Wine-lightgrey?logo=apple" alt="macOS via CrossOver/Wine" />
  <img src="https://img.shields.io/badge/Rust-nightly-F74C00?logo=rust&logoColor=white" alt="Rust nightly" />
  <img src="https://img.shields.io/github/license/ArmchairDevelopers/Maxima?color=blue" alt="GPL-3.0" />
</p>

---

> [!WARNING]
> Maxima is pre-pre-pre-alpha software, released early to support [KYBER](https://github.com/ArmchairDevelopers/Kyber). Expect rough edges.

**This is the Maxima-Draconis fork** — the EA authentication and launch backend for [Draconis](https://github.com/AA-EION/Draconis) on macOS. It is tested exclusively with **Titanfall 2 on macOS via CrossOver / Wine**. It may work on Windows directly, but that has not been tested or validated in this fork. For a general-purpose build, use the canonical [ArmchairDevelopers/Maxima](https://github.com/ArmchairDevelopers/Maxima).

> [!IMPORTANT]
> **Maxima does not run natively on macOS.** It is a Windows application that runs inside a CrossOver or Wine bottle. The Mac host only needs `MaximaHelper.app`, a lightweight background agent that bridges EA's `qrc://` login redirect from the macOS side into the bottle. Without it, the EA OAuth flow stalls because macOS browsers cannot forward `qrc://` links into Wine.

---

## Features

- **EA Authentication** — OAuth login flow with a `remid`-cookie fallback for macOS browsers that cannot handle `qrc://` redirects
- **Download & update games** — any build, with DRM and licensing support
- **Launch EA games from Steam** — Steam App IDs resolve automatically to EA offer IDs
- **`link2ea://` and `origin2://` protocol handlers** — so Steam can invoke Maxima directly from inside Wine
- **Offline mode** — launch single-player titles using a cached local license
- **EA cloud save sync**
- **Game importing** (locate existing installations)

**Not tested in this fork:**
- Linux / SteamDeck
- Direct Windows install (may work — untested)
- Epic Games Store launches
- Friends / social features
- Battlefield 3 / 4 (Battlelog launch flow)
- Pre-Download-In-Place era games (Dead Space 2, BFBC2)

---

## Project layout

```
maxima-lib/         Core library (auth, launch, library, cloud save)
maxima-cli/         Interactive CLI and subcommand frontend
maxima-bootstrap/   Windows bootstrap — handles link2ea:// / origin2:// URIs
maxima-service/     Background Windows service
maxima-resources/   Shared assets (icons, etc.)
MaximaHelper/       Native macOS Swift app — registers qrc:// on the host Mac
installer/          NSIS installer script + cross-build script (macOS → Windows)
```

---

## macOS / CrossOver setup

Maxima is a **Windows application**. On macOS it runs entirely inside a CrossOver or Wine bottle. The setup has two independent parts:

| Part | Where it runs | What it does |
|---|---|---|
| `MaximaSetup.exe` | Inside the Wine bottle | Installs Maxima and registers `link2ea://`, `origin2://`, `qrc://` handlers within Wine |
| `MaximaHelper.app` | On the Mac host (outside Wine) | Catches `qrc://` redirects from the macOS browser and forwards them into the bottle on port 31033 |

**One-time host setup (run on your Mac, outside CrossOver):**

```bash
# Requires Xcode Command Line Tools — install with: xcode-select --install
bash MaximaHelper/build.sh
```

**Build and install Maxima inside your CrossOver bottle:**

```bash
# Requires mingw-w64 and nsis: brew install mingw-w64 nsis
bash installer/build.sh
# → produces installer/MaximaSetup.exe
```

Then run `MaximaSetup.exe` inside your CrossOver bottle. It registers the protocol handlers, installs the background service, and creates start menu shortcuts.

> In a future Draconis release, both steps will be handled automatically from within the app.

---

## Building from source

Requires Rust nightly.

```bash
# Cross-compile for Windows from macOS (produces the .exe binaries)
bash installer/build.sh
```

See [`changes.md`](./changes.md) for all patches applied on top of upstream and [`todo.md`](./todo.md) for remaining work.

---

## Why "Maxima"?

It's the farthest you can get from the Origin.

---

## Credits

Maxima was created and is maintained by [ArmchairDevelopers](https://github.com/ArmchairDevelopers). This fork exists solely to support [Draconis](https://github.com/AA-EION/Draconis) and tracks upstream closely.

**Original creators:**

- [Sean Kahler](https://github.com/battledash) — creator of Maxima
- [Nick Whelan](https://github.com/headassbtw) — UI maintainer
- [Paweł Lidwin](https://github.com/imLinguin) — core maintainer

**Upstream:** [ArmchairDevelopers/Maxima](https://github.com/ArmchairDevelopers/Maxima)  
**Sister project:** [KYBER](https://uplink.kyber.gg/news/features-overview)

---

## License

GPL-3.0-or-later — same as upstream. See [LICENSE](./LICENSE).
