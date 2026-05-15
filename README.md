<p align="center">
  <img src="images/1500x500.jpg" alt="Maxima-Draconis banner" />
</p>

<h1 align="center">Maxima-Draconis</h1>

<p align="center">
  EA authentication and launch backend for <a href="https://github.com/AA-EION/Draconis">Draconis</a>.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/macOS-via%20CrossOver%20%2F%20Wine-lightgrey?logo=apple" alt="macOS via CrossOver/Wine" />
  <img src="https://img.shields.io/badge/Rust-nightly-F74C00?logo=rust&logoColor=white" alt="Rust nightly" />
  <img src="https://img.shields.io/github/license/ArmchairDevelopers/Maxima?color=blue" alt="GPL-3.0" />
</p>

---

> [!WARNING]
> This is a **fork** of [ArmchairDevelopers/Maxima](https://github.com/ArmchairDevelopers/Maxima) primarily maintained for [Draconis](https://github.com/AA-EION/Draconis) on macOS/CrossOver. The code is still portable to the other OSes upstream supports (native Windows + Linux), but only the macOS/CrossOver path is actively tested. If you want a vanilla build on Linux or native Windows, the upstream repo may be a better fit.

**Maxima-Draconis is an open-source (Mostly Vibecoded, Cringe, I know) replacement for the EA Desktop Launcher.** It handles the EA authentication handshake and license resolution that EA-published games require at startup. On macOS, it runs entirely **inside a CrossOver or Wine bottle** — it is a Windows application, not a native Mac app. The only Mac-native piece is `MaximaHelper.app`, a lightweight background agent that bridges EA's `qrc://` OAuth redirect from your browser into the bottle.

---

## How it fits into Draconis

```
macOS host
├── Draconis.app          ← native SwiftUI launcher
│   └── Resources/
│       └── MaximaHelper.app  ← bridges qrc:// OAuth from browser → Wine
│
└── CrossOver bottle
    ├── maxima-bootstrap.exe  ← catches link2ea:// and origin2:// URIs
    ├── maxima-cli.exe        ← authenticates with EA, resolves the license
    └── Titanfall2.exe
```

**Launch sequence:**

1. Draconis starts `Titanfall2.exe` (or `steam.exe -applaunch 1237970 -northstar` for Northstar).
2. The game emits `link2ea://launchgame/Origin.OFR.50.0002694?...` to request EA auth.
3. Wine routes that URI to `maxima-bootstrap.exe` (registered by the installer).
4. `maxima-bootstrap` calls `maxima-cli launch Origin.OFR.50.0002694`.
5. `maxima-cli` logs into EA (OAuth via browser if needed — `MaximaHelper.app` handles the `qrc://` redirect back into Wine), fetches the Denuvo license token, and feeds it to the game via LSX.
6. Titanfall 2 launches.

For Northstar mode the same auth chain fires after Steam starts the game.

---

## What this fork adds over upstream

| Change | Detail |
|--------|--------|
| **`MaximaHelper.app`** | Native Swift background agent for macOS — replaces the old AppleScript helper. Properly bundle-signed so LaunchServices registers `qrc://`. |
| **NSIS installer** | Cross-compiled from macOS via `mingw-w64` + `nsis`. Registers all three protocol handlers (`link2ea://`, `origin2://`, `qrc://`) inside Wine. |
| **Release CI** | GitHub Actions workflow that builds and publishes `MaximaHelper.zip` + `MaximaSetup.exe` — Draconis fetches these automatically at build time. |
| **Steam-only owner passthrough** | When the EA library lookup fails but the slug is already a valid offer ID (`Origin.OFR.X.Y`), Maxima now passes it directly to EA's license server instead of bailing. Useful when TF2 is owned through Steam only and the accounts aren't linked. |
| **`origin2://` fix** | The upstream handler hardcoded the Star Wars Battlefront 2 offer ID. Fixed to read the actual `offerIds` from the URL — any EA title can now use `origin2://`. |
| **Wine registry** | Added the bare `HKEY_LOCAL_MACHINE\Software\Origin` key (without `Electronic Arts\` prefix) that some games require. `regedit` now runs silently (`/S`) with stderr captured, so Wine errors appear in logs instead of hanging silently. |
| **DLL injector wide strings** | Fixed `GetModuleHandleA` / `LoadLibraryA` → `GetModuleHandleW` / `LoadLibraryW`. DLL injection no longer breaks on non-ASCII installation paths. |

---

## Setup (manual — Draconis automates this)

> If you are using Draconis v0.4.0+, you don't need to do any of this manually. Draconis downloads `MaximaSetup.exe` from the latest release of this repo and installs it into your bottle automatically.

**Prerequisites:** Xcode Command Line Tools (`xcode-select --install`), `brew install mingw-w64 nsis`.

```bash
# 1. Build MaximaHelper.app (runs on macOS host, bridges qrc:// OAuth)
bash MaximaHelper/build.sh

# 2. Cross-compile the Windows installer
bash installer/build.sh
# → produces installer/MaximaSetup.exe

# 3. Run MaximaSetup.exe inside your CrossOver bottle
#    It installs maxima-cli, maxima-bootstrap, maxima-service,
#    and registers link2ea://, origin2://, and qrc:// in Wine's registry.
```

---

## Building from source

```bash
# Windows binaries (cross-compiled on macOS)
cargo build --release --target x86_64-pc-windows-gnu -p maxima-cli
cargo build --release --target x86_64-pc-windows-gnu -p maxima-bootstrap
cargo build --release --target x86_64-pc-windows-gnu -p maxima-service

# macOS helper
bash MaximaHelper/build.sh

# Full installer (bundles all .exe files)
bash installer/build.sh
```

---

## Northstar online play

Northstar works with Maxima, but requires two things:

**1. Launch via Steam, not via `NorthstarLauncher.exe`.**  
`NorthstarLauncher.exe` hard-codes a call to `Origin.exe` which doesn't exist in Wine. Pass the `-northstar` flag to Steam instead so it invokes `Titanfall2.exe` directly with the Northstar hooks loaded:

```
steam.exe -applaunch 1237970 -northstar
```

Draconis already does this automatically.

**2. Add `-noOriginStartup` to your Northstar launch arguments.**  
Without it, Northstar tries to start Origin at launch, which hangs forever in Wine since there is no Origin install. The correct set of arguments is:

```
-noOriginStartup -multiple -northstar
```

Thanks to [catornot](https://github.com/catornot) for identifying this requirement and for contributing the external LSX connection patch that makes Northstar online play work in this fork. See [catornot/flightcore-ng](https://github.com/catornot/flightcore-ng/blob/221e4444b6f1813c2401deed9f21d95494bad1ed/flightcore-ng-core/src/dev/wine/wine_run.rs#L23-L31) for reference.

---

## Known limitations

- **Steam-only TF2 owners**: If your TF2 EA license isn't linked to your EA account (it's Steam-only), Maxima will warn and attempt a passthrough. For the cleanest experience, link your accounts at [ea.com](https://www.ea.com). Linking takes about 30 seconds and resolves the warning permanently.
- **Offline mode**: Implemented in the code and works after a successful first online launch. Draconis does not yet expose it in the UI. License files live at `C:/ProgramData/Maxima/Licenses/` and are valid for roughly two weeks.
- **NorthstarLauncher.exe**: Incompatible with this setup — it hard-codes a call to `Origin.exe` which doesn't exist in Wine. Northstar mode works fine via `steam.exe -applaunch 1237970 -northstar`, which is how Draconis does it.

---

## Project layout

```
maxima-lib/          Core library — auth, launch, license, library lookup
maxima-cli/          CLI frontend — authenticates and launches games
maxima-bootstrap/    Windows bootstrap — handles link2ea:// / origin2:// / qrc://
maxima-service/      Background Windows service — registry setup, DLL injection
maxima-tui/          Terminal UI (upstream, not used by Draconis)
maxima-ui/           Graphical UI (upstream, not used by Draconis)
maxima-resources/    Shared assets (icons, translations)
MaximaHelper/        macOS Swift app — bridges qrc:// from host into Wine
installer/           NSIS script + cross-build script (macOS → Windows .exe)
```

---

## Upstream

This fork tracks [ArmchairDevelopers/Maxima](https://github.com/ArmchairDevelopers/Maxima) closely. Changes specific to Draconis / macOS / CrossOver are kept in this fork; generic fixes are submitted upstream when appropriate.

**Original creators:**
- [Sean Kahler](https://github.com/battledash) — creator of Maxima
- [Nick Whelan](https://github.com/headassbtw) — UI maintainer
- [Paweł Lidwin](https://github.com/imLinguin) — core maintainer

**This fork used by:** [AA-EION/Draconis](https://github.com/AA-EION/Draconis)

**Contributors to this fork:**
- [catornot](https://github.com/catornot) — external LSX connection patch enabling Northstar online play, and identifying the `-noOriginStartup` launch argument required for Wine

---

## License

GPL-3.0-or-later — same as upstream. See [LICENSE](./LICENSE).
