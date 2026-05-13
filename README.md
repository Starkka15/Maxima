<p align="center">
  <img src="maxima-resources/assets/logo.png" width="120" alt="Maxima logo" />
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

**This is the Maxima-Draconis fork** — the EA authentication and launch backend for [Draconis](https://github.com/AA-EION/Draconis), a native macOS launcher for Titanfall 2. This fork is tested and maintained exclusively for **Titanfall 2 on macOS via CrossOver / Wine**. It may work in other configurations, but none are tested or supported here. For a general-purpose build, use [ArmchairDevelopers/Maxima](https://github.com/ArmchairDevelopers/Maxima).

> [!IMPORTANT]
> **Maxima does not run natively on macOS.** It is a Windows application that runs inside a CrossOver or Wine bottle. The Mac host only needs `MaximaHelper.app`, a lightweight background agent that bridges EA's `qrc://` login redirect from the macOS side into the bottle. Without it, the EA OAuth flow stalls because macOS browsers cannot forward `qrc://` links into Wine.

---

## What this fork supports

**Tested and maintained:**
- EA authentication — OAuth login flow with a `remid`-cookie fallback for when the browser gets stuck on the `qrc://` redirect
- `link2ea://` and `origin2://` protocol handlers — Steam uses these to launch Titanfall 2 through Maxima inside Wine
- Offline mode — launch Titanfall 2 using a cached local license without re-authenticating
- Titanfall 2 launch via Steam on macOS / CrossOver

**In the codebase but not tested in this fork:**
- Other EA titles
- Downloading / updating games
- Cloud save sync
- Friends and social features
- Linux / SteamDeck
- Direct Windows install
- Epic Games Store

---

## How it works

```
macOS host
├── Draconis.app          — native launcher UI (SwiftUI)
├── MaximaHelper.app      — catches qrc:// from the browser, forwards to Wine
│
└── CrossOver bottle
    ├── maxima-bootstrap  — handles link2ea:// URIs from Steam
    ├── maxima-cli        — authenticates with EA, resolves the offer ID
    └── Titanfall2.exe
```

When Steam launches Titanfall 2, it sends a `link2ea://` URI → `maxima-bootstrap` catches it inside Wine → `maxima-cli` authenticates with EA and resolves the license → `Titanfall2.exe` launches.

---

## Setup on macOS

**Step 1 — Build `MaximaHelper.app` on your Mac** (outside CrossOver, run once):

```bash
# Requires Xcode Command Line Tools: xcode-select --install
bash MaximaHelper/build.sh
```

This compiles and registers a native Swift background agent on your Mac that intercepts `qrc://` URLs from the browser and tunnels them into the Wine bottle during EA login.

**Step 2 — Build the Windows installer on your Mac** (cross-compile, outside CrossOver):

```bash
# Requires: brew install mingw-w64 nsis
bash installer/build.sh
# → produces installer/MaximaSetup.exe
```

**Step 3 — Install Maxima inside your CrossOver bottle:**

Run `MaximaSetup.exe` inside the bottle. It registers the `link2ea://`, `origin2://`, and `qrc://` protocol handlers within Wine, installs the background service, and adds start menu shortcuts.

> In a future [Draconis](https://github.com/AA-EION/Draconis) release, these steps will be handled automatically.

---

## Project layout

```
maxima-lib/               Core library (auth, launch, library)
maxima-cli/               CLI frontend — authenticates and launches games
maxima-bootstrap/         Windows bootstrap — handles link2ea:// / origin2:// URIs
maxima-service/           Background Windows service
maxima-resources/         Shared assets (icons, etc.)
MaximaHelper/             Native macOS Swift app — bridges qrc:// from host to Wine
installer/                NSIS script + cross-build script (macOS → Windows .exe)
```

---

## Credits

Maxima was created and is maintained by [ArmchairDevelopers](https://github.com/ArmchairDevelopers). This fork exists solely to support [Draconis](https://github.com/AA-EION/Draconis) and tracks upstream closely.

**Original creators:**
- [Sean Kahler](https://github.com/battledash) — creator of Maxima
- [Nick Whelan](https://github.com/headassbtw) — UI maintainer
- [Paweł Lidwin](https://github.com/imLinguin) — core maintainer

**Upstream:** [ArmchairDevelopers/Maxima](https://github.com/ArmchairDevelopers/Maxima)  
**Used by:** [AA-EION/Draconis](https://github.com/AA-EION/Draconis)

---

## License

GPL-3.0-or-later — same as upstream. See [LICENSE](./LICENSE).
