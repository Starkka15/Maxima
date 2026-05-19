//! Steam install discovery helpers, shared between `maxima-cli` (which uses
//! them when `link2ea://launchgame/<numeric>?platform=steam` arrives with no
//! linked EA library) and `auth_server` (which needs the on-disk path to
//! tell `request_and_save_license` which OOA version to probe).
//!
//! These were originally in `maxima-cli/src/main.rs`; they were moved up
//! into `maxima-lib` once the same lookup was needed from the HTTP
//! `/authorize` handler, so we don't end up with two copies that can drift.

use std::path::PathBuf;

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    /// Matches a well-formed EA offer ID like "Origin.OFR.50.0002694".
    pub static ref EA_OFFER_ID_PATTERN: Regex = Regex::new(r"^Origin\.OFR\.\d+\.\d+$").unwrap();
    /// Matches a Steam App ID emitted by `link2ea://launchgame/<id>?platform=steam`.
    /// Current Steam App IDs fit in 1..=10 ASCII digits (max issued is ~3M).
    pub static ref STEAM_APP_ID_PATTERN: Regex = Regex::new(r"^\d{1,10}$").unwrap();
}

/// Hardcoded fallback table for EA-published games available on Steam.
///
/// When a Steam-only owner whose EA account isn't linked launches an
/// EA-on-Steam title, the EA library lookup fails — both for the
/// offer-id translation (Steam App ID → Origin offer ID) AND for the
/// install location (EA Desktop doesn't know where Steam put the game).
/// This table provides both:
///   - the EA Origin offer ID to use for license/auth
///   - the relative path under `steamapps/common/` to find the exe
///
/// Discovery (`resolve_steam_install_path`):
///   1. Read `HKLM\SOFTWARE\(Wow6432Node\)Valve\Steam\InstallPath` for Steam root
///   2. Parse `<steam>\steamapps\libraryfolders.vdf` for additional libraries
///   3. Verify `<library>\steamapps\common\<install_subdir>\<exe>` exists
///
/// Extend as more EA-on-Steam titles get validated.
pub struct SteamGameEntry {
    pub steam_app_id: &'static str,
    pub origin_offer_id: &'static str,
    /// Directory name under `steamapps/common/`, e.g. "Titanfall2".
    pub install_subdir: &'static str,
    /// Game executable filename within the install dir, e.g. "Titanfall2.exe".
    pub exe_name: &'static str,
}

pub const STEAM_GAMES: &[SteamGameEntry] = &[
    SteamGameEntry {
        steam_app_id: "1237970",
        // Note: NOT Origin.OFR.50.0002694 — that's Apex Legends. TF2's real
        // offer ID is Origin.OFR.50.0001456, confirmed against a real EA
        // library dump ("titanfall-2 - Titanfall 2 - Origin.OFR.50.0001456").
        origin_offer_id: "Origin.OFR.50.0001456",
        install_subdir: "Titanfall2",
        exe_name: "Titanfall2.exe",
    },
];

pub fn lookup_steam_game(steam_app_id: &str) -> Option<&'static SteamGameEntry> {
    STEAM_GAMES.iter().find(|g| g.steam_app_id == steam_app_id)
}

/// Reverse of `lookup_steam_game`: find the entry for a given Origin offer ID.
/// Used by the HTTP `/authorize` handler — it receives an offer ID (because
/// that's what `link2ea://launchgame/Origin.OFR.…` carries when TF2 emits the
/// URL mid-run) and needs to find the on-disk path even if the EA library
/// lookup fails.
pub fn lookup_steam_game_by_offer(origin_offer_id: &str) -> Option<&'static SteamGameEntry> {
    STEAM_GAMES.iter().find(|g| g.origin_offer_id == origin_offer_id)
}

/// Resolve where a Steam-installed EA game lives on disk. Returns the full
/// path to the executable, or None if not installed in any known Steam
/// library.
///
/// Lookup order:
///   1. Steam install path from registry (both 32-bit and 64-bit views)
///   2. Common defaults (covers fresh Wine bottles where the registry key
///      may not yet exist)
///   3. Parse `libraryfolders.vdf` to discover extra Steam library folders
///   4. Verify `<library>\steamapps\common\<subdir>\<exe>` exists
#[cfg(windows)]
pub fn resolve_steam_install_path(game: &SteamGameEntry) -> Option<PathBuf> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let mut steam_roots: Vec<PathBuf> = Vec::new();

    // 1. Registry — try both views since Steam installs as 32-bit on most systems
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for key in &[
        "SOFTWARE\\WOW6432Node\\Valve\\Steam",
        "SOFTWARE\\Valve\\Steam",
    ] {
        if let Ok(subkey) = hklm.open_subkey(key) {
            if let Ok(path) = subkey.get_value::<String, _>("InstallPath") {
                steam_roots.push(PathBuf::from(path));
            }
        }
    }

    // 2. Common defaults (covers fresh Wine bottles where the registry key
    //    may not have been written yet, or when running outside Wine)
    for default in &[
        "C:\\Program Files (x86)\\Steam",
        "C:\\Program Files\\Steam",
    ] {
        let p = PathBuf::from(default);
        if p.exists() && !steam_roots.contains(&p) {
            steam_roots.push(p);
        }
    }

    // 3. For each Steam root, gather library folders from libraryfolders.vdf
    //    and search for the game.
    for root in &steam_roots {
        let mut libraries: Vec<PathBuf> = vec![root.clone()];

        // VDF is a simple key-value format; we don't need a full parser
        // — just grep "path" lines.
        let vdf_paths = [
            root.join("steamapps").join("libraryfolders.vdf"),
            root.join("config").join("libraryfolders.vdf"),
        ];
        for vdf in &vdf_paths {
            if let Ok(content) = std::fs::read_to_string(vdf) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    // Lines look like:   "path"   "C:\\SteamLibrary"
                    if let Some(rest) = trimmed.strip_prefix("\"path\"") {
                        if let Some(start) = rest.find('"') {
                            let after = &rest[start + 1..];
                            if let Some(end) = after.find('"') {
                                let extra = PathBuf::from(after[..end].replace("\\\\", "\\"));
                                if !libraries.contains(&extra) {
                                    libraries.push(extra);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. Verify the executable exists in each library
        for lib in &libraries {
            let exe = lib
                .join("steamapps")
                .join("common")
                .join(game.install_subdir)
                .join(game.exe_name);
            if exe.exists() {
                return Some(exe);
            }
        }
    }

    None
}

#[cfg(not(windows))]
pub fn resolve_steam_install_path(_game: &SteamGameEntry) -> Option<PathBuf> {
    // Non-Windows builds (Linux CI, native macOS) can't read the Windows
    // registry. Maxima only runs through Wine on those targets and the
    // win32 binaries take the cfg(windows) branch.
    None
}
