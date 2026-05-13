# Maxima-Draconis — Remaining Work

## High Priority

- [ ] **Diagnose game launch failure**: Collect `%TEMP%\maxima_execution.log`
  from a CrossOver session and verify whether `maxima-cli launch` is reached
  and whether `Titanfall2.exe` is spawned.
- [ ] **Registry path for Steam-managed installs**: Verify that
  `HKLM\SOFTWARE\Respawn\Titanfall2\Install Dir` is populated inside the
  CrossOver bottle, or add a fallback path scan.
- [ ] **MaximaHelper deployment in Draconis**: Draconis should bundle
  `MaximaHelper.app` in its resources and register it on first launch, removing
  the need for a manual `build.sh` run.

## Medium Priority

- [ ] **Draconis installer wizard**: Guide the user through:
  1. Running `installer/build.sh` (or downloading a pre-built `MaximaSetup.exe`)
  2. Opening CrossOver / creating a bottle
  3. Running `MaximaSetup.exe` inside the bottle
  4. Triggering the first login so `MaximaHelper` handles the `qrc://` redirect
- [ ] **Offline mode validation**: End-to-end test of `LaunchMode::Offline` for
  single-player campaigns after an initial online session caches the license.
- [ ] **Steam/Epic auto-detection**: If registry keys are absent, scan common
  Steam library paths (`~/.steam/steam/steamapps/`, etc.) for game installs.

## Low Priority

- [ ] **Improved login UI**: Surface a "paste remid cookie" input in `maxima-ui`
  so GUI users get the same fallback as CLI users.
- [ ] **Installer hardening**: Handle existing Maxima installations and running
  services gracefully before overwriting files.
