# Maxima-Draconis — Changes from Upstream

This file tracks modifications made on top of the upstream
[ArmchairDevelopers/Maxima](https://github.com/ArmchairDevelopers/Maxima)
project to support **macOS/CrossOver** environments and Steam/Epic game launches.

---

## 1. Authentication Fallback (macOS / CrossOver)

**Problem:** macOS browsers do not recognise the `qrc://` protocol EA uses for
OAuth redirection, so the login flow hangs.

**Fix:**
- Added a native macOS helper (`MaximaHelper/`) — a minimal Swift background app
  that registers the `qrc://` URL scheme on macOS and silently forwards it to
  Maxima's TCP listener on port 31033. Build it once with `MaximaHelper/build.sh`.
- Added a `remid`-cookie fallback in `maxima-lib/src/core/auth/login.rs`: when
  the browser gets stuck, users can paste the `remid` cookie value directly into
  the CLI, which then performs the OAuth exchange server-side.
- The CLI now uses `tokio::select!` so both the TCP socket and stdin are monitored
  concurrently — the first successful signal wins.

---

## 2. Steam App ID Resolution

**Problem:** Steam launches games via its own App IDs (e.g. `1237970` for
Titanfall 2). Maxima's library lookup only matched EA Offer IDs / slugs.

**Fix:** `maxima-cli/src/main.rs` now performs an exhaustive cascade:
1. Slug match
2. Base offer ID match
3. Full library scan across `product.id`, `origin_offer_id`, `content_id`, and
   `product.product.id`

---

## 3. `link2ea://` Protocol Handler in Bootstrap

**Problem:** `maxima-bootstrap` had a `todo!()` stub for `link2ea://` URIs
(emitted by Steam when launching EA titles).

**Fix:** Implemented the handler in `maxima-bootstrap/src/main.rs`:
- Parses the URL, extracts the offer ID, forwards `cmdParams` via the
  `MAXIMA_LAUNCH_ARGS` environment variable, and spawns `maxima-cli launch`.

---

## 4. Offline Mode

**Problem:** `LaunchMode::Offline` was an unimplemented `todo!()` stub in both
`launch.rs` and `registry.rs`.

**Fix:**
- `maxima-lib/src/core/launch.rs`: resolves the game from the local library,
  verifies installation, and sets `EALaunchOfflineMode=true` for the child
  process.
- `maxima-lib/src/util/registry.rs`: enabled `link2ea://` and `origin2://`
  protocol registrations (they were commented out pending bootstrap implementation).

---

## 5. Diagnostics & Headless Error Reporting

**Problem:** `maxima-bootstrap.exe` runs without a console window; failures
were silent.

**Fix:** Added append-only log output to `%TEMP%\maxima_execution.log` and
`%TEMP%\maxima_bootstrap_error.log` at each stage of execution.

---

## 6. Build System & Installer

- `.cargo/config.toml`: pins MinGW linker for `x86_64-pc-windows-gnu` cross-compilation.
- `rust-toolchain.toml`: pins the nightly toolchain required by the workspace.
- `installer/build.sh`: cross-compiles all binaries and runs NSIS to produce
  `MaximaSetup.exe` — run from macOS with MinGW and NSIS installed.
- `installer/maxima-setup.nsi`: registers `qrc://`, `link2ea://`, and `origin2://`
  protocol handlers, installs the Windows service, and creates Add/Remove Programs
  entries.
- `.github/workflows/build-ci.yml`: CI now installs NSIS on Windows runners and
  uploads `MaximaSetup.exe` as a build artefact.
