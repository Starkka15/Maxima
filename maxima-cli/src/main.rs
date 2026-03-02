use clap::{Parser, Subcommand};

use anyhow::{bail, Result};
use inquire::{Select, Text};
use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use regex::Regex;

use std::{path::PathBuf, time::Instant};

#[cfg(windows)]
use is_elevated::is_elevated;

#[cfg(windows)]
use maxima::{
    core::background_service::request_registry_setup,
    util::service::{is_service_running, is_service_valid, register_service_user, start_service},
};

use maxima::{
    content::{downloader::ZipDownloader, manager::QueuedGameBuilder, ContentService},
    core::{
        auth::{
            context::AuthContext,
            login::{begin_oauth_login_flow, manual_login},
            nucleus_auth_exchange, nucleus_token_exchange, TokenResponse,
        },
        clients::JUNO_PC_CLIENT_ID,
        cloudsync::CloudSyncLockMode,
        launch::{self, LaunchMode, LaunchOptions},
        library::OwnedTitle,
        manifest::{self, MANIFEST_RELATIVE_PATH},
        service_layer::{
            ServiceGetBasicPlayerRequestBuilder, ServiceGetLegacyCatalogDefsRequestBuilder,
            ServiceLayerError, ServiceLegacyOffer, ServicePlayer, SERVICE_REQUEST_GETBASICPLAYER,
            SERVICE_REQUEST_GETLEGACYCATALOGDEFS,
        },
        LockedMaxima, Maxima, MaximaEvent, MaximaOptionsBuilder,
    },
    ooa::{self, needs_license_update, request_and_save_license, LicenseAuth},
    rtm::client::BasicPresence,
    steam::{
        lookup_steam_game, resolve_steam_install_path, EA_OFFER_ID_PATTERN, STEAM_APP_ID_PATTERN,
    },
    util::{
        log::{init_logger, init_logger_named},
        native::take_foreground_focus,
        registry::check_registry_validity,
        simple_crypto,
    },
};

lazy_static! {
    static ref MANUAL_LOGIN_PATTERN: Regex = Regex::new(r"^(.*):(.*)$").unwrap();
}

#[derive(Subcommand, Debug)]
enum Mode {
    Launch {
        slug: String,

        #[arg(long)]
        game_path: Option<String>,

        /// Extra arguments forwarded to the game executable. Repeated:
        /// `--game-args -noOriginStartup --game-args -vanilla`. Values
        /// starting with `-` are common (Northstar's `-noOriginStartup`,
        /// Source's `-novid`, etc.), so `allow_hyphen_values = true`
        /// stops clap from interpreting them as flags.
        ///
        /// For convenience, any args after a literal `--` are also
        /// forwarded to the game (see `trailing_args`), so both forms
        /// work:
        ///   `maxima-cli launch X --game-args -noOriginStartup --game-args -multiple`
        ///   `maxima-cli launch X -- -noOriginStartup -multiple`
        /// Both lists are concatenated (game_args first, then trailing).
        #[arg(long, allow_hyphen_values = true)]
        game_args: Vec<String>,

        /// When set, offer_id must be a content ID, and the only authenticated
        /// requests are made to the license server. A dummy name will be used
        /// in place of your real username, and any online LSX requests will fail
        #[arg(long)]
        login: Option<String>,

        /// Trailing positional args after `--`. clap's `last = true`
        /// collects everything past the `--` separator without flag
        /// interpretation, which makes it natural to pass a series of
        /// game flags without repeating `--game-args` for each one.
        /// Merged with `game_args` before being forwarded to the game.
        #[arg(last = true)]
        trailing_args: Vec<String>,
    },
    ListGames {
        /// Emit a JSON array on stdout (with log output suppressed) instead
        /// of the human-readable `info!` lines. Intended for Draconis and
        /// other automation that needs to inspect what Maxima has in the
        /// user's EA library — per-game `slug`, `name`, `offer_id`,
        /// `content_id`, `installed`, `install_path`, `version`, plus the
        /// list of extra-offer DLC.
        #[arg(long)]
        json: bool,
    },
    LocateGame {
        path: String,
    },
    /// Install a game from the user's EA library to a local path,
    /// non-interactively. Equivalent to the interactive "Install Game"
    /// option in `maxima-cli` (no args) or to the maxima-ui install flow,
    /// but driven entirely by CLI flags so Draconis / scripts can kick it
    /// off headless.
    Install {
        /// Slug, offer_id, or content_id of the game to install. Resolved
        /// against the EA library the same way `launch` resolves its
        /// `slug` argument.
        slug: String,

        /// Absolute path to install into. Will be created if missing. If
        /// it already contains a different game's files, behavior is
        /// `install_now`'s problem — Maxima doesn't try to dry-run.
        #[arg(long)]
        path: String,

        /// Specific build ID to install. Defaults to the live (latest)
        /// build advertised by the EA content service.
        #[arg(long)]
        build_id: Option<String>,

        /// Comma-separated list of file paths (relative to `--path`) to
        /// delete BEFORE the install runs, so the downloader sees them as
        /// missing and re-fetches them from EA's content servers. Designed
        /// for the Steam-CEG fix flow: a Steam-installed `Titanfall2.exe`
        /// is the same size as the EA original (CEG patches bytes in
        /// place) so the size-only entry-state check in the downloader
        /// would skip it. Listing it here forces a clean replace.
        /// Path-traversal segments (`..`) are rejected.
        #[arg(long, value_delimiter = ',')]
        replace_files: Vec<String>,

        /// Restrict the install to **only** the files listed in
        /// `--replace-files` — the downloader fetches just those entries
        /// from the build manifest and leaves every other file on disk
        /// alone. Without this flag, the full `install_now` flow runs
        /// after the delete step, which can re-download large chunks of
        /// the game when Steam-vs-EA file sizes legitimately differ
        /// (~50% of the manifest in the TF2 case). With this flag, an
        /// "Apply Maxima fix" against a Steam install touches only the
        /// exes you actually need to replace. Requires `--replace-files`
        /// to be non-empty.
        #[arg(long, requires = "replace_files")]
        only_listed_files: bool,

        /// Emit JSONL progress on stdout (one JSON document per line)
        /// with logger stdout suppressed. Each progress tick is
        /// `{"event":"progress","percent":<0-100>}`; the terminator is
        /// `{"event":"done","elapsed_secs":<float>}` on success or
        /// `{"event":"error","message":"..."}` on failure (also exits
        /// non-zero). Designed for Draconis to drive a real-time
        /// progress bar without scraping log lines.
        #[arg(long)]
        json: bool,
    },
    /// Walk the manifest for a game's live build and report which
    /// files on disk don't match the expected size. Designed to catch
    /// partial installs (the v0.12.1 decoder-poisoning bug + any
    /// future class of broken-content-with-right-size cases will need
    /// hash verification once we deduce EA's manifest CRC32 format).
    ///
    /// With `--repair`, broken files are re-downloaded via the same
    /// `--replace-files --only-listed-files` primitive as the
    /// Steam-CEG fix: surgical, only the broken entries, no whole-
    /// game re-download.
    Verify {
        /// Slug / offer_id / content_id, resolved against the EA
        /// library the same way `install` and `launch` resolve theirs.
        slug: String,
        /// Absolute path of the existing install dir.
        #[arg(long)]
        path: String,
        /// After listing broken files, immediately re-download them
        /// via `install --replace-files --only-listed-files`. No-op
        /// if the verify pass finds nothing wrong.
        #[arg(long)]
        repair: bool,
        /// Emit JSONL on stdout. Events:
        ///   `{"event":"progress","phase":"verify","files_checked":i,"total_files":n}`
        ///   `{"event":"verify_done","ok":N,"broken":[...]}`
        ///   `{"event":"progress","phase":"repair","current_file":"X","files_done":i,"total_files":m}`
        ///   `{"event":"done","verified":N,"broken":N,"repaired":M}`
        ///   `{"event":"error","message":"..."}` (also exits non-zero)
        #[arg(long)]
        json: bool,
    },
    CloudSync {
        game_slug: String,

        #[arg(long)]
        write: bool,
    },
    AccountInfo,
    CreateAuthCode {
        #[arg(long)]
        client_id: String,
    },
    JunoTokenRefresh,
    ReadLicenseFile {
        #[arg(long)]
        content_id: String,
    },
    GetUserById {
        #[arg(long)]
        user_id: String,
    },
    GetGameBySlug {
        #[arg(long)]
        slug: String,
    },
    TestRTMConnection,
    ListFriends,
    GetLegacyCatalogDef {
        #[arg(long)]
        offer_id: String,
    },
    DownloadSpecificFile {
        #[arg(long)]
        offer_id: String,

        #[arg(long)]
        build_id: String,

        #[arg(long)]
        file: String,
    },
    /// Run as a passive LSX server — log in, start the LSX listener, optionally
    /// log in to RTM, and wait indefinitely (Ctrl-C to stop). This is the CLI
    /// equivalent of "open the Maxima UI and leave it running": no game is
    /// launched by this process, so when an externally-started game (Steam
    /// `applaunch`, Northstar's `steam.exe -applaunch 1237970 -northstar`, or
    /// a direct double-click on `Titanfall2.exe`) connects to LSX, the
    /// connection's `playing()` is None — which exercises the
    /// catornot/patch-external-lsx code path that the user reports works on
    /// Windows. Use this when `maxima-cli launch` keeps tripping TF2's
    /// "File corruption detected" tamper check: kick `serve` first, then
    /// launch the game externally.
    Serve {
        /// Skip RTM (Real-Time Messaging) login — useful in low-connectivity
        /// environments or when you only care about LSX auth, not friends
        /// presence. Default is to log in to RTM so SetPresence requests from
        /// the game update your status normally.
        #[arg(long)]
        no_rtm: bool,
    },
    /// Get game info (offer_id, installed status) by slug
    GameInfo {
        /// Game slug (from list-games output)
        slug: String,
    },
    /// Authenticate and print EA environment variables for a game (no game launch)
    AuthEnv {
        /// Game slug (from list-games output)
        slug: String,

        /// Override the game path (used for license binding)
        #[arg(long)]
        game_path: Option<String>,
    },
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    mode: Option<Mode>,

    #[arg(long)]
    #[clap(global = true)]
    login: Option<String>,
}

/// Ensure a console window exists AND that Rust's stdio is wired up to it.
///
/// When `maxima-cli` is spawned by `maxima-bootstrap` (built as a Windows GUI
/// app via `#![windows_subsystem = "windows"]`), the child process inherits
/// the parent's stdio — which is null/invalid because bootstrap has no
/// console. Two things break:
///
/// 1. No console window appears at all (until we call `AllocConsole`).
/// 2. Even after `AllocConsole`, Rust's `println!` / `eprintln!` still write
///    to the invalid handles they inherited. `AllocConsole` does NOT
///    automatically redirect existing std handles — it only creates the
///    console window. We have to point `STD_OUTPUT_HANDLE` / `STD_ERROR_HANDLE`
///    / `STD_INPUT_HANDLE` at `CONOUT$` / `CONIN$` ourselves.
///
/// Without step 2 the v0.2.1 fix is decorative: the console window pops up
/// but stays blank because the logger writes go nowhere.
///
/// Idempotent: if a console is already attached (`cmd.exe` invocation),
/// `AllocConsole` fails harmlessly and we still rewire the std handles to
/// `CONOUT$` (which resolves to the existing console).
#[cfg(windows)]
fn ensure_console_attached() {
    use std::ptr::null_mut;
    use winapi::um::consoleapi::AllocConsole;
    use winapi::um::fileapi::{CreateFileA, OPEN_EXISTING};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::processenv::SetStdHandle;
    use winapi::um::winbase::{STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    use winapi::um::wincon::GetConsoleWindow;
    use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};

    unsafe {
        if GetConsoleWindow().is_null() {
            // Failure here means we already had a console (rare given the null
            // check) or the OS refused to give us one; either way, file
            // logging still works as a fallback.
            AllocConsole();
        }

        // Rewire std handles to the (possibly freshly allocated) console.
        // Each `CreateFileA` opens an independent handle; passing the same
        // handle to multiple `SetStdHandle` calls is technically allowed but
        // fragile (closing one closes them all).
        let open_console = |name: &[u8]| -> *mut winapi::ctypes::c_void {
            CreateFileA(
                name.as_ptr() as *const i8,
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null_mut(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };

        let stdout = open_console(b"CONOUT$\0");
        if stdout != INVALID_HANDLE_VALUE {
            SetStdHandle(STD_OUTPUT_HANDLE, stdout);
        }

        let stderr = open_console(b"CONOUT$\0");
        if stderr != INVALID_HANDLE_VALUE {
            SetStdHandle(STD_ERROR_HANDLE, stderr);
        }

        let stdin = open_console(b"CONIN$\0");
        if stdin != INVALID_HANDLE_VALUE {
            SetStdHandle(STD_INPUT_HANDLE, stdin);
        }
    }
}

#[cfg(not(windows))]
fn ensure_console_attached() {}

/// Install a panic hook that writes the panic message to a dedicated file
/// before unwinding. Without this, panics that happen *before* the regular
/// logger is initialized (or that hit `eprintln!` when stderr is unattached)
/// disappear silently — exactly the failure mode that made the v0.2.1
/// "nothing shows" bug so hard to diagnose.
///
/// File location matches the rest of the file logging:
///   - Windows: %LOCALAPPDATA%\Maxima\Logs\maxima-cli.panic.log
///   - Unix:    $XDG_DATA_HOME/maxima/logs/maxima-cli.panic.log (or ~/.local/share/...)
fn install_panic_hook() {
    let log_path: Option<std::path::PathBuf> = {
        #[cfg(windows)]
        {
            std::env::var_os("LOCALAPPDATA")
                .or_else(|| std::env::var_os("APPDATA"))
                .map(std::path::PathBuf::from)
                .map(|p| p.join("Maxima").join("Logs").join("maxima-cli.panic.log"))
        }
        #[cfg(unix)]
        {
            std::env::var_os("XDG_DATA_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
                })
                .map(|p| p.join("maxima").join("logs").join("maxima-cli.panic.log"))
        }
    };

    std::panic::set_hook(Box::new(move |info| {
        // Best-effort: never let the panic hook itself panic.
        if let Some(ref path) = log_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                use std::io::Write;
                let _ = writeln!(
                    file,
                    "\n===== PANIC at {:?} (pid={}) =====\n{}",
                    std::time::SystemTime::now(),
                    std::process::id(),
                    info
                );
                let _ = file.flush();
            }
        }
        // Also try stderr (works once stdio is reattached to the console).
        eprintln!("FATAL: {}", info);
    }));
}

/// Returns true if the parsed mode requests JSON output. Used to enable
/// stdout suppression on the global logger before anything has a chance to
/// log — keeps `--json` subcommand output cleanly parseable.
fn json_mode(args: &Args) -> bool {
    matches!(
        args.mode,
        Some(Mode::ListGames { json: true })
            | Some(Mode::Install { json: true, .. })
            | Some(Mode::Verify { json: true, .. })
    )
}

/// Plain (non-tokio) `main`. The order is load-bearing:
///
/// 1. Panic hook BEFORE anything fallible so a panic in any subsequent step
///    is captured on disk.
/// 2. Console + stdio reattach BEFORE any println / clap output so error
///    messages reach the user.
/// 3. Logger init BEFORE `Args::parse()` so clap's exit-on-error path can
///    hit the file sink.
/// 4. Argument parsing.
/// 5. Tokio runtime constructed manually so a panic in runtime setup (e.g.
///    IOCP init under Wine) is caught by the panic hook above — `#[tokio::main]`
///    would construct the runtime *before* user code, defeating step 1.
fn main() {
    install_panic_hook();
    ensure_console_attached();
    init_logger_named("maxima-cli");

    let args = Args::parse();

    // For `--json` subcommands, mute stdout logging so callers (Draconis,
    // scripts) can parse stdout as a single JSON document. The file sink
    // keeps receiving everything for debugging.
    if json_mode(&args) {
        maxima::util::log::set_stdout_suppressed(true);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!("Failed to build tokio runtime: {}", e);
            std::process::exit(1);
        }
    };

    let result = runtime.block_on(startup(args));

    if let Some(e) = result.err() {
        match std::env::var("RUST_BACKTRACE") {
            Ok(_) => error!("{}:\n{}", e, e.backtrace().to_string()),
            Err(_) => error!("{}: {}", e, e.root_cause()),
        }
    }
}

#[cfg(windows)]
async fn native_setup() -> Result<()> {
    if !is_elevated() {
        if !is_service_valid()? {
            info!("Installing service...");
            register_service_user()?;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        if !is_service_running()? {
            info!("Starting service...");
            start_service().await?;
        }
    }

    if let Err(err) = check_registry_validity() {
        warn!("{}, fixing...", err);
        request_registry_setup().await?;
    }

    Ok(())
}

#[cfg(not(windows))]
async fn native_setup() -> Result<()> {
    use maxima::util::registry::set_up_registry;

    if let Err(err) = check_registry_validity() {
        warn!("{}, fixing...", err);
        set_up_registry()?;
    }

    Ok(())
}

pub async fn login_flow(login_override: Option<String>) -> Result<TokenResponse> {
    let mut auth_context = AuthContext::new()?;

    if let Some(access_token) = &login_override {
        let access_token = if let Some(captures) = MANUAL_LOGIN_PATTERN.captures(&access_token) {
            let persona = &captures[1];
            let password = &captures[2];

            let login_result = manual_login(persona, password).await;
            if login_result.is_err() {
                bail!("Login failed: {}", login_result.err().unwrap().to_string());
            }

            login_result.unwrap()
        } else {
            access_token.to_owned()
        };

        auth_context.set_access_token(&access_token);
        let code = nucleus_auth_exchange(&auth_context, JUNO_PC_CLIENT_ID, "code").await?;
        auth_context.set_code(&code);
    } else {
        begin_oauth_login_flow(&mut auth_context).await?
    };

    if auth_context.code().is_none() {
        bail!("Login failed!");
    }

    if login_override.is_none() {
        info!("Received login...");
    }

    let token_res = nucleus_token_exchange(&auth_context).await;
    if token_res.is_err() {
        bail!("Login failed: {}", token_res.err().unwrap().to_string());
    }

    Ok(token_res?)
}

async fn startup(args: Args) -> Result<()> {
    // Args parsing and logger initialization happen in `main()` so a clap
    // exit hits the file sink and the panic hook is already installed by
    // the time the runtime is built.

    info!("Starting Maxima...");

    native_setup().await?;

    // `--login <content_id>` short-circuits the OAuth flow: only the
    // license server needs auth, so we skip loading persistent auth
    // storage and run with a dummy local user. `matches!` lets us
    // ignore the rest of the Launch fields cleanly — they don't
    // affect this decision.
    let skip_login = matches!(args.mode, Some(Mode::Launch { login: Some(_), .. }));

    let options = MaximaOptionsBuilder::default()
        .load_auth_storage(!skip_login)
        .dummy_local_user(skip_login)
        .build()?;

    let maxima_arc = Maxima::new_with_options(options).await?;

    if !skip_login {
        let maxima = maxima_arc.lock().await;

        {
            let mut auth_storage = maxima.auth_storage().lock().await;
            let logged_in = auth_storage.logged_in().await?;
            if !logged_in || args.login.is_some() {
                info!("Logging in...");
                let token_res = login_flow(args.login).await?;
                auth_storage.add_account(&token_res).await?;
            }
        }

        let user = maxima.local_user().await?;

        info!(
            "Logged in as {}!",
            user.player().as_ref().unwrap().display_name()
        );
    }

    // Take back the focus since the browser and bootstrap will take it
    take_foreground_focus()?;

    if args.mode.is_none() {
        run_interactive(maxima_arc.clone()).await?;
        return Ok(());
    }

    let mode = args.mode.unwrap();
    match mode {
        Mode::Launch {
            slug,
            game_path,
            game_args,
            login,
            trailing_args,
        } => {
            // Merge the explicit `--game-args` repetitions with the
            // post-`--` trailing args. `--game-args` first so order is
            // predictable for callers that mix both styles.
            let mut game_args = game_args;
            game_args.extend(trailing_args);
            let offer_id = if login.is_none() {
                let mut maxima = maxima_arc.lock().await;

                // First try standard slug
                let mut found_offer_id = None;
                if let Ok(Some(offer)) = maxima.mut_library().game_by_base_slug(&slug).await {
                    found_offer_id = Some(offer.offer_id().clone());
                }

                // Then try base offer
                if found_offer_id.is_none() {
                    if let Ok(Some(offer)) = maxima.mut_library().game_by_base_offer(&slug).await {
                        found_offer_id = Some(offer.offer_id().clone());
                    }
                }

                // If still not found, do an exhaustive search across all properties
                // (useful for Steam App IDs or content IDs)
                if found_offer_id.is_none() {
                    if let Ok(games) = maxima.mut_library().games().await {
                        for game in games {
                            let base = game.base_offer();
                            if base.slug() == &slug
                                || base.offer_id() == &slug
                                || base.product().id() == &slug
                                || base.product().origin_offer_id() == &slug
                                || base.offer().content_id() == &slug
                                || base.product().product().id() == &slug
                            {
                                found_offer_id = Some(base.offer_id().clone());
                                break;
                            }
                        }
                    }
                }

                if let Some(id) = found_offer_id {
                    id
                } else if EA_OFFER_ID_PATTERN.is_match(&slug) {
                    // The EA library lookup failed (e.g. Steam-only owner whose TF2 is not
                    // linked to their EA account), but the slug is already a well-formed EA
                    // offer ID — pass it through and let EA's license server decide.
                    warn!(
                        "Offer '{}' not found in EA library; passing through directly. \
                         If this fails, link your Steam account at https://www.ea.com",
                        slug
                    );
                    slug.clone()
                } else if STEAM_APP_ID_PATTERN.is_match(&slug) {
                    // Slug is a Steam App ID. The exhaustive library lookup above
                    // should have matched it via product.id / offer.content_id for
                    // any user whose Steam and EA accounts are linked. If we got
                    // here the accounts are not linked, so fall back to the static
                    // STEAM_GAMES table — the EA license server only accepts
                    // Origin offer IDs, not Steam IDs, so a passthrough would
                    // just fail with a less helpful error.
                    if let Some(game) = lookup_steam_game(&slug) {
                        warn!(
                            "Steam App ID '{}' not in EA library (Steam/EA accounts not linked?); \
                             using hardcoded fallback offer ID '{}'. Link your accounts at \
                             https://www.ea.com to remove this warning.",
                            slug, game.origin_offer_id
                        );
                        game.origin_offer_id.to_string()
                    } else {
                        bail!(
                            "Steam App ID '{}' is not in this user's EA library and has no \
                             hardcoded fallback. Link your Steam and EA accounts at https://www.ea.com, \
                             or open an issue if this is an EA-published game on Steam that should be supported.",
                            slug
                        );
                    }
                } else {
                    bail!("No owned offer found for '{}'. If this is an EA offer ID, make sure your EA and Steam accounts are linked at https://www.ea.com", slug);
                }
            } else {
                slug.clone()
            };

            // If the slug was a Steam App ID, the game is installed under
            // Steam's library, not EA Desktop's. `launch::start_game` would
            // bail with `NotInstalled` because EA's metadata doesn't know
            // about the Steam install. Discover the actual location from
            // Steam's registry + libraryfolders.vdf and pass it as an
            // explicit game_path override.
            let resolved_game_path = if game_path.is_none() && STEAM_APP_ID_PATTERN.is_match(&slug)
            {
                lookup_steam_game(&slug)
                    .and_then(resolve_steam_install_path)
                    .and_then(|p| p.to_str().map(|s| s.to_owned()))
                    .map(|p| {
                        info!("Discovered Steam install for app {}: {}", slug, p);
                        p
                    })
                    .or_else(|| {
                        warn!(
                            "Could not auto-discover Steam install path for app {}. \
                             If this game is installed in a non-standard location, \
                             pass --game-path manually.",
                            slug
                        );
                        None
                    })
            } else {
                game_path
            };

            // Steam-Play detection: if the original slug was a numeric
            // Steam App ID, surface it via `LaunchOptions.steam_app_id`.
            // `launch::start_game` uses that to set SteamAppId/SteamGameId
            // env vars on the spawned game — see the LaunchOptions doc
            // comment. Per-game launch args (-noOriginStartup, -multiple,
            // etc.) are the caller's responsibility: pass via --game-args
            // or MAXIMA_LAUNCH_ARGS.
            let steam_app_id = STEAM_APP_ID_PATTERN.is_match(&slug).then(|| slug.clone());

            start_game(
                &offer_id,
                resolved_game_path,
                game_args,
                login,
                steam_app_id,
                maxima_arc.clone(),
            )
            .await
        }
        Mode::ListGames { json } => list_games(maxima_arc.clone(), json).await,
        Mode::LocateGame { path } => locate_game(maxima_arc.clone(), &path).await,
        Mode::Install {
            slug,
            path,
            build_id,
            replace_files,
            only_listed_files,
            json,
        } => {
            install_game(
                maxima_arc.clone(),
                &slug,
                &path,
                build_id.as_deref(),
                &replace_files,
                only_listed_files,
                json,
            )
            .await
        }
        Mode::Verify {
            slug,
            path,
            repair,
            json,
        } => verify_game(maxima_arc.clone(), &slug, &path, repair, json).await,
        Mode::CloudSync { game_slug, write } => {
            do_cloud_sync(maxima_arc.clone(), &game_slug, write).await
        }
        Mode::AccountInfo => print_account_info(maxima_arc.clone()).await,
        Mode::CreateAuthCode { client_id } => {
            create_auth_code(maxima_arc.clone(), &client_id).await
        }
        Mode::JunoTokenRefresh => juno_token_refresh(maxima_arc.clone()).await,
        Mode::ReadLicenseFile { content_id } => read_license_file(&content_id).await,
        Mode::ListFriends => list_friends(maxima_arc.clone()).await,
        Mode::GetUserById { user_id } => get_user_by_id(maxima_arc.clone(), &user_id).await,
        Mode::GetGameBySlug { slug } => get_game_by_slug(maxima_arc.clone(), &slug).await,
        Mode::TestRTMConnection => test_rtm_connection(maxima_arc.clone()).await,
        Mode::GetLegacyCatalogDef { offer_id } => {
            get_legacy_catalog_def(maxima_arc.clone(), &offer_id).await
        }
        Mode::DownloadSpecificFile {
            offer_id,
            build_id,
            file,
        } => download_specific_file(maxima_arc.clone(), &offer_id, &build_id, &file).await,
        Mode::Serve { no_rtm } => serve_lsx(maxima_arc.clone(), no_rtm).await,
        Mode::GameInfo { slug } => {
            game_info(maxima_arc.clone(), &slug).await
        }
        Mode::AuthEnv { slug, game_path } => {
            auth_env(maxima_arc.clone(), &slug, game_path).await
        }
    }?;

    Ok(())
}

async fn run_interactive(maxima_arc: LockedMaxima) -> Result<()> {
    let launch_options = vec![
        "Launch Game",
        "Install Game",
        "List Builds",
        "List Games",
        "Account Info",
    ];
    let name = Select::new(
        "Welcome to Maxima! What would you like to do?",
        launch_options,
    )
    .prompt()?;

    match name {
        "Launch Game" => interactive_start_game(maxima_arc.clone()).await?,
        "Install Game" => interactive_install_game(maxima_arc.clone()).await?,
        "List Builds" => generate_download_links(maxima_arc.clone()).await?,
        "List Games" => list_games(maxima_arc.clone(), false).await?,
        "Account Info" => print_account_info(maxima_arc.clone()).await?,
        _ => bail!("Something went wrong."),
    }

    Ok(())
}

async fn interactive_start_game(maxima_arc: LockedMaxima) -> Result<()> {
    let offer_id = {
        let mut maxima = maxima_arc.lock().await;

        let mut owned_games = Vec::new();
        for game in maxima.mut_library().games().await? {
            if !game.base_offer().is_installed().await {
                continue;
            }

            owned_games.push(game);
        }

        let owned_games_strs = owned_games
            .iter()
            .map(|g| g.name())
            .collect::<Vec<String>>();

        let name = Select::new("What game would you like to play?", owned_games_strs).prompt()?;
        let game: &OwnedTitle = owned_games.iter().find(|g| g.name() == name).unwrap();
        game.base_offer().offer_id().to_owned()
    };

    start_game(&offer_id, None, Vec::new(), None, None, maxima_arc.clone()).await?;

    Ok(())
}

async fn interactive_install_game(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    let offer_id = {
        let mut owned_games = Vec::new();
        for game in maxima.mut_library().games().await? {
            if game.base_offer().is_installed().await {
                continue;
            }

            owned_games.push(game);
        }

        let owned_games_strs = owned_games
            .iter()
            .map(|g| g.name())
            .collect::<Vec<String>>();

        let name =
            Select::new("What game would you like to install?", owned_games_strs).prompt()?;
        let game = owned_games.iter().find(|g| g.name() == name).unwrap();
        game.base_offer().offer_id().to_owned()
    };

    let builds = maxima
        .content_manager()
        .service()
        .available_builds(&offer_id)
        .await?;
    let build = builds.live_build();
    if build.is_none() {
        bail!("Couldn't find a suitable game build");
    }

    let build = build.unwrap();
    info!("Installing game build {}", build.to_string());

    let path = PathBuf::from(
        Text::new("Where would you like to install the game? (must be an absolute path)")
            .prompt()?,
    );
    if !path.is_absolute() {
        error!("Path {:?} is not absolute.", path);
        return Ok(());
    }

    let game = QueuedGameBuilder::default()
        .offer_id(offer_id)
        .build_id(build.build_id().to_owned())
        .path(path.clone())
        .build()?;

    let start_time = Instant::now();
    maxima.content_manager().install_now(game).await?;

    drop(maxima);

    loop {
        let mut maxima = maxima_arc.lock().await;

        for event in maxima.consume_pending_events() {
            match event {
                MaximaEvent::ReceivedLSXRequest(_pid, _request) => (),
                _ => {}
            }
        }

        maxima.update().await;

        if let Some(downloader) = maxima.content_manager().current() {
            info!("Downloading: {}%/100%", downloader.percentage_done());
        } else {
            break;
        }

        drop(maxima);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    let end_time = Instant::now();
    let elapsed_time = end_time - start_time;

    info!(
        "Download took {}.{}",
        elapsed_time.as_secs(),
        elapsed_time.subsec_millis()
    );

    Ok(())
}

async fn game_info(maxima_arc: LockedMaxima, slug: &str) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    let titles = maxima.mut_library().games().await?;
    for title in titles {
        if title.base_offer().slug() == slug {
            let installed = title.base_offer().is_installed().await;
            println!(
                "{{\"slug\":\"{}\",\"name\":\"{}\",\"offer_id\":\"{}\",\"installed\":{}}}",
                title.base_offer().slug(),
                title.name().replace('"', "\\\""),
                title.base_offer().offer_id(),
                installed
            );
            return Ok(());
        }
    }

    bail!("No game found with slug '{}'", slug);
}

async fn auth_env(
    maxima_arc: LockedMaxima,
    slug: &str,
    game_path_override: Option<String>,
) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    // Resolve slug to offer — extract owned data before releasing the library borrow
    let (offer_id, content_id, exe_path) = {
        let offer = maxima.mut_library().game_by_base_slug(slug).await?;
        if offer.is_none() {
            bail!("No owned game found for slug '{}'", slug);
        }
        let offer = offer.unwrap();
        let oid = offer.offer_id().to_owned();
        let cid = offer.offer().content_id().to_owned();
        let epath = if game_path_override.is_none() {
            Some(offer.execute_path(false).await?.clone())
        } else {
            None
        };
        (oid, cid, epath)
    };

    let access_token = maxima.access_token().await?;

    // Determine game path for license binding
    let path = if let Some(ref p) = game_path_override {
        PathBuf::from(p)
    } else {
        exe_path.unwrap()
    };

    // Handle licensing
    let auth = LicenseAuth::AccessToken(access_token.clone());
    if needs_license_update(&content_id).await? {
        info!("Requesting game license for {}...", offer.offer().display_name());
        request_and_save_license(&auth, &content_id, path.clone()).await?;
    } else {
        info!("Existing game license is still valid");
    }

    // Request opaque OOA token for online mode
    let mut auth_context = AuthContext::new()?;
    auth_context.set_access_token(&access_token);
    auth_context.set_token_format("OPAQUE");
    auth_context.set_expires_in(550);
    auth_context.add_scope("basic.commerce.cartv2");
    auth_context.add_scope("service.atom");
    auth_context.add_scope("dp.client.default");
    auth_context.add_scope("signin");
    auth_context.add_scope("social_recommendation_user");
    auth_context.add_scope("basic.optin.write");
    auth_context.add_scope("basic.commerce.cartv2.write");
    auth_context.add_scope("basic.billing");
    auth_context.add_scope("external.social_information_ups_admin");
    let short_token = nucleus_auth_exchange(&auth_context, JUNO_PC_CLIENT_ID, "token").await?;

    let user = maxima.local_user().await?;
    let launch_id = uuid::Uuid::new_v4().to_string();
    let display_name = user
        .player()
        .as_ref()
        .ok_or(ServiceLayerError::MissingField)?
        .display_name()
        .to_owned();

    // Print EA environment variables as shell export statements to stdout
    // The launcher script will eval this output
    println!("export MXLaunchId=\"{}\"", launch_id);
    println!("export EAAuthCode=\"unavailable\"");
    println!("export EAEgsProxyIpcPort=\"0\"");
    println!("export EAEntitlementSource=\"EA\"");
    println!("export EAExternalSource=\"EA\"");
    println!("export EAFreeTrialGame=\"false\"");
    println!("export EAGameLocale=\"{}\"", maxima.locale().full_str());
    println!("export EAGenericAuthToken=\"{}\"", access_token);
    println!("export EALaunchCode=\"unavailable\"");
    println!("export EALaunchOwner=\"EA\"");
    println!("export EALaunchEAID=\"{}\"", display_name);
    println!("export EALaunchEnv=\"production\"");
    println!("export EALaunchOfflineMode=\"false\"");
    println!("export EALsxPort=\"{}\"", maxima.lsx_port());
    println!("export EARtPLaunchCode=\"{}\"", simple_crypto::rtp_handshake());
    println!("export EASecureLaunchTokenTemp=\"{}\"", user.id());
    println!("export EASteamProxyIpcPort=\"0\"");
    println!("export OriginSessionKey=\"{}\"", launch_id);
    println!("export ContentId=\"{}\"", content_id);
    println!("export EAOnErrorExitRetCode=\"1\"");
    println!("export EAConnectionId=\"{}\"", offer_id);
    println!("export EALicenseToken=\"{}\"", offer_id);
    println!("export EALaunchUserAuthToken=\"{}\"", short_token);
    println!("export EAAccessTokenJWS=\"{}\"", access_token);

    info!("EA environment variables exported for {}", slug);

    Ok(())
}

async fn download_specific_file(
    maxima_arc: LockedMaxima,
    offer: &str,
    build_id: &str,
    file: &str,
) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    let content_service = ContentService::new(maxima.auth_storage().clone());
    let builds = content_service.available_builds(offer).await?;
    let build = builds.build(build_id);
    if build.is_none() {
        bail!("Couldn't find the game build {}", build_id);
    }

    let build = build.unwrap();
    info!("Downloading file from game build {}", build.to_string());

    let url = content_service
        .download_url(offer, Some(&build.build_id()))
        .await?;

    debug!("URL: {}", url.url());

    let downloader = ZipDownloader::new("test-game", &url.url(), "C:/DownloadTest").await?;
    let num_of_entries = downloader.manifest().entries().len();
    info!("Entries: {}", num_of_entries);

    let entry = downloader
        .manifest()
        .entries()
        .iter()
        .find(|x| x.name() == file);
    if entry.is_none() {
        bail!("Couldn't find the file {}", file);
    }

    let ele = entry.unwrap();
    downloader.download_single_file(ele, None).await.unwrap();

    info!(
        "Downloaded file {} from game build {}",
        file,
        build.to_string()
    );
    Ok(())
}

async fn generate_download_links(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    let content_service = ContentService::new(maxima.auth_storage().clone());

    let owned_games = maxima.mut_library().games().await?;
    let owned_games_strs = owned_games
        .iter()
        .map(|g| g.name())
        .collect::<Vec<String>>();

    let name = Select::new(
        "What game would you like to list builds for?",
        owned_games_strs,
    )
    .prompt()?;
    let game = owned_games.iter().find(|g| g.name() == name).unwrap();

    info!("Working...");

    let builds = content_service
        .available_builds(&game.base_offer().offer_id())
        .await?;

    let mut strs = String::new();
    for build in builds.builds {
        let url = content_service
            .download_url(&game.base_offer().offer_id(), Some(&build.build_id()))
            .await;
        if url.is_err() {
            continue;
        }

        let url = url.unwrap();

        strs += &build.to_string();
        strs += ": ";
        strs += url.url();
        strs += "\n";
    }

    println!("{}", strs);
    Ok(())
}

async fn print_account_info(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;
    let user = maxima.local_user().await?;

    info!("Access Token: {}", maxima.access_token().await?);
    info!("PC Sign: {}", AuthContext::new()?.generate_pc_sign()?);

    let player = user.player().as_ref().unwrap();
    info!("Username: {}", player.unique_name());
    info!("User ID: {}", user.id());
    info!("Persona ID: {}", player.psd());
    Ok(())
}

async fn create_auth_code(maxima_arc: LockedMaxima, client_id: &str) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    let mut context = AuthContext::new()?;
    context.set_access_token(&maxima.access_token().await?);

    let auth_code = nucleus_auth_exchange(&context, client_id, "code").await?;
    info!("Auth Code for {}: {}", client_id, auth_code);
    info!("Code verifier: {}", context.code_verifier());
    Ok(())
}

async fn juno_token_refresh(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    let mut context = AuthContext::new()?;
    context.set_access_token(&maxima.access_token().await?);

    context.add_scope("basic.identity");
    context.add_scope("basic.persona");
    context.add_scope("basic.entitlement");

    let code = nucleus_auth_exchange(&context, JUNO_PC_CLIENT_ID, "code").await?;
    context.set_code(&code);

    if context.code().is_none() {
        bail!("Login failed!");
    }

    let token_res = nucleus_token_exchange(&context).await;
    if token_res.is_err() {
        bail!("Login failed: {}", token_res.err().unwrap().to_string());
    }

    let token_res = token_res.unwrap();
    info!("Access Token: {}", token_res.access_token());
    info!("Refresh Token: {:?}", token_res.refresh_token());
    info!("Token Type: {}", token_res.token_type());
    Ok(())
}

async fn read_license_file(content_id: &str) -> Result<()> {
    let path = ooa::get_license_dir()?.join(format!("{}.dlf", content_id));
    let mut data = tokio::fs::read(path).await?;
    data.drain(0..65); // Signature

    let license = ooa::decrypt_license(data.as_slice())?;
    info!("License: {:?}", license);

    Ok(())
}

async fn list_friends(maxima_arc: LockedMaxima) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    for ele in maxima.friends(0).await? {
        info!(
            "{} [ID: {}, Persona ID: {}]",
            ele.display_name(),
            ele.pd(),
            ele.psd()
        );
    }

    Ok(())
}

async fn get_user_by_id(maxima_arc: LockedMaxima, user_id: &str) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    let player: ServicePlayer = maxima
        .service_layer()
        .request(
            SERVICE_REQUEST_GETBASICPLAYER,
            ServiceGetBasicPlayerRequestBuilder::default()
                .pd(user_id.to_string())
                .build()?,
        )
        .await?;

    info!("Name: {}", player.display_name());

    dbg!(player);
    Ok(())
}

async fn get_game_by_slug(maxima_arc: LockedMaxima, slug: &str) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    match maxima.mut_library().game_by_base_slug(slug).await? {
        Some(game) => {
            info!("Slug:       {}", game.slug());
            info!("Offer ID:   {}", game.offer_id());
            info!("Content ID: {}", game.offer().content_id());
            info!("Display:    {}", game.offer().display_name());
            info!("Installed:  {}", game.is_installed().await);
        }
        None => {
            bail!("No game found for slug '{}'", slug);
        }
    }

    Ok(())
}

async fn test_rtm_connection(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;
    let friends = maxima.friends(0).await?;

    let rtm = maxima.rtm();
    rtm.login().await?;
    rtm.set_presence(BasicPresence::Online, "Test", "Origin.OFR.50.0002148")
        .await?;

    let players: Vec<String> = friends.iter().map(|f| f.id().to_owned()).collect();
    info!("Subscribed to {} players", players.len());

    rtm.subscribe(&players).await?;
    drop(maxima);

    loop {
        let mut maxima = maxima_arc.lock().await;
        maxima.rtm().heartbeat().await?;

        {
            let store = maxima.rtm().presence_store().lock().await;
            for entry in store.iter() {
                info!(
                    "{}/{} is {:?}: In {}",
                    friends
                        .iter()
                        .find(|x| x.id().to_owned() == *entry.0)
                        .unwrap()
                        .display_name(),
                    entry.0,
                    entry.1.basic(),
                    entry.1.status()
                );
            }
        }

        drop(maxima);

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn get_legacy_catalog_def(maxima_arc: LockedMaxima, offer_id: &str) -> Result<()> {
    let maxima = maxima_arc.lock().await;
    let defs: Vec<ServiceLegacyOffer> = maxima
        .service_layer()
        .request(
            SERVICE_REQUEST_GETLEGACYCATALOGDEFS,
            ServiceGetLegacyCatalogDefsRequestBuilder::default()
                .offer_ids(vec![offer_id.to_owned()])
                .locale(maxima.locale().clone())
                .build()?,
        )
        .await?;

    info!("Content ID: {}", defs[0].content_id());
    Ok(())
}

#[derive(serde::Serialize)]
struct GameJson {
    slug: String,
    name: String,
    offer_id: String,
    content_id: String,
    display_name: String,
    installed: bool,
    install_path: Option<String>,
    version: Option<String>,
    has_cloud_save: bool,
    extra_offers: Vec<ExtraOfferJson>,
}

#[derive(serde::Serialize)]
struct ExtraOfferJson {
    offer_id: String,
    display_name: String,
}

async fn list_games(maxima_arc: LockedMaxima, json: bool) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;
    let titles = maxima.mut_library().games().await?;

    if json {
        // Machine-readable mode: one JSON array, no log noise. main() already
        // muted stdout logging via `set_stdout_suppressed(true)` for this
        // subcommand; everything that follows goes directly to stdout via
        // `println!`.
        let mut out: Vec<GameJson> = Vec::with_capacity(titles.len());
        for title in titles {
            let base = title.base_offer();
            let installed = base.is_installed().await;
            // `execute_path` and `installed_version` both read the local
            // manifest, which can be absent for externally-installed copies
            // (Steam, manually-placed). Swallow those errors — Draconis
            // can still see `installed: true` even when version is unknown,
            // and `installed: false` is enough to drive the install flow.
            // Errors are logged at `debug!` so the file sink still has them
            // for diagnosing why a path/version came back null, without
            // breaking the JSON document on stdout.
            let install_path = if installed {
                match base.execute_path(false).await {
                    Ok(p) => Some(p.display().to_string()),
                    Err(e) => {
                        debug!(
                            "execute_path for {} failed: {}",
                            base.offer_id(),
                            e
                        );
                        None
                    }
                }
            } else {
                None
            };
            let version = if installed {
                match base.installed_version().await {
                    Ok(v) => Some(v),
                    Err(e) => {
                        debug!(
                            "installed_version for {} failed: {}",
                            base.offer_id(),
                            e
                        );
                        None
                    }
                }
            } else {
                None
            };

            let extras = title
                .extra_offers()
                .iter()
                .map(|g| ExtraOfferJson {
                    offer_id: g.offer_id().clone(),
                    display_name: g.offer().display_name().to_string(),
                })
                .collect();

            out.push(GameJson {
                slug: base.slug().clone(),
                name: title.name().to_string(),
                offer_id: base.offer_id().clone(),
                content_id: base.offer().content_id().to_string(),
                display_name: base.offer().display_name().to_string(),
                installed,
                install_path,
                version,
                has_cloud_save: base.offer().has_cloud_save(),
                extra_offers: extras,
            });
        }

        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    info!("Owned games:");
    for title in titles {
        info!(
            "{:<width$} - {:<width2$} - {:<width3$} - Installed: {}",
            title.base_offer().slug(),
            title.name(),
            title.base_offer().offer_id(),
            title.base_offer().is_installed().await,
            width = 35,
            width2 = 35,
            width3 = 25,
        );

        for game in title.extra_offers() {
            info!(
                "  {:<width$} - {:<width2$}",
                game.offer().display_name(),
                game.offer_id(),
                width = 55,
                width2 = 25
            );
        }
    }

    Ok(())
}

/// Non-interactive install driver. Mirrors `interactive_install_game` but
/// takes all parameters as CLI flags so Draconis / shell scripts can drive
/// downloads without an inquire-prompted TTY. In JSON mode emits one
/// JSON document per line on stdout — `{"event":"progress","percent":N}`
/// once per second, then `{"event":"done",…}` on success or
/// `{"event":"error",…}` on failure. Logger stdout is already suppressed
/// by `main()` when `--json` is set.
async fn install_game(
    maxima_arc: LockedMaxima,
    slug: &str,
    path: &str,
    build_id_override: Option<&str>,
    replace_files: &[String],
    only_listed_files: bool,
    json: bool,
) -> Result<()> {
    use std::io::Write;

    let emit_error = |msg: &str| {
        if json {
            println!(
                "{}",
                serde_json::json!({"event": "error", "message": msg})
            );
            let _ = std::io::stdout().flush();
        }
    };

    let install_path = PathBuf::from(path);
    if !install_path.is_absolute() {
        let msg = format!("--path must be absolute, got '{}'", path);
        emit_error(&msg);
        bail!(msg);
    }

    // Pre-install replace step. The downloader's `initial_state` check
    // marks entries `Fresh` when missing (re-downloads) and `Complete`
    // when their on-disk size matches the manifest (skips). For files
    // that are content-different-but-same-size (the Steam CEG case),
    // we need to delete them up front so the downloader's size check
    // sees them as missing. Path-traversal segments are rejected so a
    // bad `--replace-files` argument can't escape the install dir.
    for relative in replace_files {
        if relative.is_empty() {
            continue;
        }
        if relative
            .split(['/', '\\'])
            .any(|segment| segment == ".." || segment.is_empty())
            || PathBuf::from(relative).is_absolute()
        {
            let msg = format!(
                "--replace-files entries must be relative paths without '..' segments, got '{}'",
                relative
            );
            emit_error(&msg);
            bail!(msg);
        }

        let target = install_path.join(relative);
        match std::fs::metadata(&target) {
            Ok(meta) if meta.is_file() => {
                if !json {
                    info!("Deleting {} (replace-files)", target.display());
                }
                if let Err(e) = std::fs::remove_file(&target) {
                    let msg = format!("failed to delete {}: {}", target.display(), e);
                    emit_error(&msg);
                    bail!(msg);
                }
            }
            Ok(_) => {
                // Directory or symlink — skip, not what this flag is for.
                if !json {
                    warn!(
                        "Skipping {} from --replace-files (not a regular file)",
                        target.display()
                    );
                }
            }
            Err(_) => {
                // Already missing — that's fine, the downloader will fetch it.
                if !json {
                    debug!("--replace-files: {} already absent", target.display());
                }
            }
        }
    }

    // Resolve slug → offer_id. Same chain as `Mode::Launch` minus the
    // unlinked-Steam passthrough fallbacks (those only let you launch an
    // already-installed copy; for install we genuinely need the offer in
    // the user's EA library).
    let offer_id = {
        let mut maxima = maxima_arc.lock().await;
        let mut found: Option<String> = None;
        if let Ok(Some(offer)) = maxima.mut_library().game_by_base_slug(slug).await {
            found = Some(offer.offer_id().clone());
        }
        if found.is_none() {
            if let Ok(Some(offer)) = maxima.mut_library().game_by_base_offer(slug).await {
                found = Some(offer.offer_id().clone());
            }
        }
        if found.is_none() {
            if let Ok(games) = maxima.mut_library().games().await {
                for game in games {
                    let base = game.base_offer();
                    if base.slug() == slug
                        || base.offer_id() == slug
                        || base.product().id() == slug
                        || base.product().origin_offer_id() == slug
                        || base.offer().content_id() == slug
                        || base.product().product().id() == slug
                    {
                        found = Some(base.offer_id().clone());
                        break;
                    }
                }
            }
        }
        match found {
            Some(id) => id,
            None => {
                let msg = format!(
                    "No owned offer found for '{}'. Link your Steam/EA accounts at https://www.ea.com if you bought the game from a third-party store.",
                    slug
                );
                emit_error(&msg);
                bail!(msg);
            }
        }
    };

    // Pick the build to install: explicit override, else the live build.
    let build_id = {
        let mut maxima = maxima_arc.lock().await;
        let builds = maxima
            .content_manager()
            .service()
            .available_builds(&offer_id)
            .await?;
        let chosen = match build_id_override {
            Some(bid) => builds.build(bid).ok_or_else(|| {
                anyhow::anyhow!("build_id '{}' not found for offer '{}'", bid, offer_id)
            })?,
            None => builds.live_build().ok_or_else(|| {
                anyhow::anyhow!("No live build available for offer '{}'", offer_id)
            })?,
        };
        let id = chosen.build_id().to_owned();
        if !json {
            info!(
                "Installing build {} of {} into {}",
                chosen.to_string(),
                offer_id,
                install_path.display()
            );
        }
        id
    };

    // Strict replace path: download only the entries listed in
    // `--replace-files`, never run `install_now`. Empirically the full
    // install path re-downloads ~50% of the TF2 manifest against a Steam
    // install because legitimate size differences across distribution
    // channels make many files fail the size-only entry-state check.
    // For the CEG-fix use case we only need to refresh a handful of
    // CEG-touched binaries, so we bypass install_now entirely and pull
    // each listed file directly from the build's zip manifest via
    // `ZipDownloader::download_single_file` — the same primitive
    // `Mode::DownloadSpecificFile` already uses.
    if only_listed_files {
        // Release the maxima_arc lock BEFORE the async `download_url` HTTP
        // call so concurrent LSX / RTM / serve operations on the same
        // Maxima instance aren't blocked while EA's CDN responds.
        let auth_storage = {
            let maxima = maxima_arc.lock().await;
            maxima.auth_storage().clone()
        };
        let content_service = ContentService::new(auth_storage);
        let url = content_service
            .download_url(&offer_id, Some(&build_id))
            .await?;

        // Use the offer_id as the downloader's id so concurrent strict
        // installs of different games don't collide in any temp/state
        // that ZipDownloader keys by id.
        let downloader =
            ZipDownloader::new(&offer_id, url.url(), install_path.clone()).await?;
        let entries = downloader.manifest().entries();

        // Empty entries in `replace_files` are skipped by the delete loop;
        // mirror that here so an empty string doesn't bail the whole flow
        // on a manifest lookup miss, and so per-file progress counts stay
        // accurate.
        let total = replace_files.iter().filter(|s| !s.is_empty()).count();
        let start_time = Instant::now();

        for (idx, relative) in replace_files
            .iter()
            .filter(|s| !s.is_empty())
            .enumerate()
        {
            let normalized = relative.replace('\\', "/");
            // Common case: zip entries use forward slashes, so the
            // case-insensitive compare against the unmodified name
            // succeeds without allocating. Fall back to a normalized
            // compare only when the entry path actually contains
            // backslashes — saves an allocation per entry per file on
            // manifests with tens of thousands of entries.
            let entry = entries.iter().find(|e| {
                let n = e.name();
                if n.eq_ignore_ascii_case(&normalized) {
                    return true;
                }
                if n.contains('\\') {
                    return n.replace('\\', "/").eq_ignore_ascii_case(&normalized);
                }
                false
            });
            let entry = match entry {
                Some(e) => e,
                None => {
                    let msg = format!(
                        "file '{}' not found in build {} manifest",
                        relative, build_id
                    );
                    emit_error(&msg);
                    bail!(msg);
                }
            };

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "event": "progress",
                        "current_file": relative,
                        "files_done": idx,
                        "total_files": total,
                    })
                );
                let _ = std::io::stdout().flush();
            } else {
                info!("Downloading {} ({}/{})", relative, idx + 1, total);
            }

            if let Err(e) = downloader.download_single_file(entry, None).await {
                let msg = format!("download of '{}' failed: {}", relative, e);
                emit_error(&msg);
                return Err(e.into());
            }
        }

        let elapsed = start_time.elapsed();
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "event": "done",
                    "elapsed_secs": elapsed.as_secs_f64(),
                    "offer_id": offer_id,
                    "build_id": build_id,
                    "path": install_path.display().to_string(),
                    "files_replaced": replace_files,
                })
            );
            let _ = std::io::stdout().flush();
        } else {
            info!(
                "Replaced {} file(s) in {}.{}s — {}",
                total,
                elapsed.as_secs(),
                elapsed.subsec_millis(),
                install_path.display()
            );
        }

        return Ok(());
    }

    let game = QueuedGameBuilder::default()
        .offer_id(offer_id.clone())
        .build_id(build_id.clone())
        .path(install_path.clone())
        .build()?;

    let start_time = Instant::now();
    {
        let mut maxima = maxima_arc.lock().await;
        if let Err(e) = maxima.content_manager().install_now(game).await {
            let msg = format!("install_now failed: {}", e);
            emit_error(&msg);
            return Err(e.into());
        }
    }

    // Progress loop: poll every second, tick the content manager, emit a
    // progress line. When `current()` goes None the download is done.
    loop {
        let (percent, still_downloading) = {
            let mut maxima = maxima_arc.lock().await;
            for _event in maxima.consume_pending_events() {
                // Drained but not surfaced — the current ContentManager API
                // doesn't emit structured download-error events we could
                // forward as `event: error` lines. Mid-install failures fall
                // through to `install_now`'s error return (caught above).
            }
            maxima.update().await;
            match maxima.content_manager().current() {
                Some(d) => (d.percentage_done(), true),
                None => (100.0, false),
            }
        };

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "event": "progress",
                    "percent": percent,
                    "build_id": build_id,
                })
            );
            let _ = std::io::stdout().flush();
        } else {
            info!("Downloading: {}%/100%", percent);
        }

        if !still_downloading {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    let elapsed = start_time.elapsed();
    if json {
        println!(
            "{}",
            serde_json::json!({
                "event": "done",
                "elapsed_secs": elapsed.as_secs_f64(),
                "offer_id": offer_id,
                "build_id": build_id,
                "path": install_path.display().to_string(),
            })
        );
        let _ = std::io::stdout().flush();
    } else {
        info!(
            "Install complete in {}.{}s — {}",
            elapsed.as_secs(),
            elapsed.subsec_millis(),
            install_path.display()
        );
    }

    Ok(())
}

/// Walk the build manifest for `slug` and report which on-disk files
/// don't match the expected `uncompressed_size`. With `repair = true`,
/// feed the broken list into `install_game(--replace-files
/// --only-listed-files)` for a surgical re-fetch.
///
/// Why size-only (for now):
///   * EA's manifest stores a `crc32` field but the value upstream
///     Maxima computes from on-disk bytes never matches what's in the
///     manifest. The hash-check code was commented out by upstream
///     with the note "We must be calculating the hash incorrectly or
///     something." Until that's deduced, hash verification would
///     false-positive on every entry. Size-only catches:
///       - missing files (file doesn't exist)
///       - truncated files (size < expected)
///       - over-sized files (size > expected)
///     It misses:
///       - same-size content corruption (e.g. the v0.12.1 decoder-
///         poisoning bug, now fixed in v0.12.2). Users in that
///         pre-fix bottle state can target the known-bad file via
///         `install --replace-files <name> --only-listed-files`
///         directly.
async fn verify_game(
    maxima_arc: LockedMaxima,
    slug: &str,
    path: &str,
    repair: bool,
    json: bool,
) -> Result<()> {
    use std::io::Write;
    use tokio::fs;

    let start_ts = std::time::Instant::now();
    let emit_error = |msg: &str| {
        if json {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"event": "error", "message": msg})
            );
            let _ = std::io::stdout().flush();
        }
    };

    let install_path = PathBuf::from(path);
    if !install_path.is_absolute() {
        let msg = format!("--path must be absolute, got '{}'", path);
        emit_error(&msg);
        bail!(msg);
    }
    if !fs::try_exists(&install_path).await.unwrap_or(false) {
        let msg = format!("install path '{}' doesn't exist", install_path.display());
        emit_error(&msg);
        bail!(msg);
    }

    // Slug → offer_id resolution. Same chain as `install_game` minus
    // the unlinked-Steam passthrough fallbacks: for verify we genuinely
    // need the manifest, so we must hit the EA library.
    let offer_id = {
        let mut maxima = maxima_arc.lock().await;
        let mut found: Option<String> = None;
        if let Ok(Some(offer)) = maxima.mut_library().game_by_base_slug(slug).await {
            found = Some(offer.offer_id().clone());
        }
        if found.is_none() {
            if let Ok(Some(offer)) = maxima.mut_library().game_by_base_offer(slug).await {
                found = Some(offer.offer_id().clone());
            }
        }
        if found.is_none() {
            if let Ok(games) = maxima.mut_library().games().await {
                for game in games {
                    let base = game.base_offer();
                    if base.slug() == slug
                        || base.offer_id() == slug
                        || base.product().id() == slug
                        || base.product().origin_offer_id() == slug
                        || base.offer().content_id() == slug
                        || base.product().product().id() == slug
                    {
                        found = Some(base.offer_id().clone());
                        break;
                    }
                }
            }
        }
        match found {
            Some(id) => id,
            None => {
                let msg = format!("No owned offer found for '{}'", slug);
                emit_error(&msg);
                bail!(msg);
            }
        }
    };

    // Resolve the live build + fetch the manifest. We don't need a
    // `QueuedGame` or `add_install` — just the metadata, which the
    // `ZipDownloader::new` constructor pulls down as a side effect.
    let (manifest_url, build_id) = {
        let maxima = maxima_arc.lock().await;
        let content_service = ContentService::new(maxima.auth_storage().clone());
        let builds = content_service.available_builds(&offer_id).await?;
        let build = match builds.live_build() {
            Some(b) => b,
            None => {
                let msg = format!("No live build for offer '{}'", offer_id);
                emit_error(&msg);
                bail!(msg);
            }
        };
        let build_id = build.build_id().to_owned();
        let url = content_service
            .download_url(&offer_id, Some(&build_id))
            .await?;
        (url.url().to_owned(), build_id)
    };

    let downloader = ZipDownloader::new(&offer_id, &manifest_url, &install_path).await?;
    let entries = downloader.manifest().entries();
    let total = entries.len();

    if !json {
        info!(
            "Verifying {} files for '{}' (build {})",
            total, offer_id, build_id
        );
    }

    // Walk entries, compare sizes. Progress emitted every ~5% of total
    // (or every 100 files for smaller manifests) so JSON consumers
    // get a steady cadence without flooding.
    let mut broken: Vec<String> = Vec::new();
    let progress_every = std::cmp::max(total / 20, 100);
    for (i, entry) in entries.iter().enumerate() {
        // Skip directory entries (manifest sometimes lists trailing-
        // slash names) and zero-byte entries (uncompressed_size == 0).
        let name = entry.name();
        if name.ends_with('/') || *entry.uncompressed_size() == 0 {
            continue;
        }
        let file_path = install_path.join(name);
        let expected = *entry.uncompressed_size();
        let actual = match fs::metadata(&file_path).await {
            Ok(meta) => meta.len() as i64,
            Err(_) => -1, // missing
        };
        if actual != expected {
            if !json {
                if actual < 0 {
                    warn!("Missing: {}", name);
                } else {
                    warn!("Size mismatch: {} (got {}, expected {})", name, actual, expected);
                }
            }
            broken.push(name.clone());
        }
        if json && (i + 1) % progress_every == 0 {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({
                    "event": "progress",
                    "phase": "verify",
                    "files_checked": i + 1,
                    "total_files": total,
                })
            );
            let _ = std::io::stdout().flush();
        }
    }

    if json {
        let _ = writeln!(
            std::io::stdout(),
            "{}",
            serde_json::json!({
                "event": "verify_done",
                "ok": total - broken.len(),
                "broken": &broken,
                "total": total,
            })
        );
        let _ = std::io::stdout().flush();
    } else {
        info!(
            "Verify done — {}/{} ok, {} broken",
            total - broken.len(),
            total,
            broken.len()
        );
    }

    // If nothing's broken, we're done (success either way).
    if broken.is_empty() {
        if json {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({
                    "event": "done",
                    "verified": total,
                    "broken": 0,
                    "repaired": 0,
                    "elapsed_secs": start_ts.elapsed().as_secs_f64(),
                })
            );
            let _ = std::io::stdout().flush();
        }
        return Ok(());
    }

    // --repair: feed broken into the existing install --replace-files
    // --only-listed-files pipeline. `install_game` emits its own
    // JSONL events when invoked with `json = true`, so the consumer
    // sees a continuous stream across the verify → repair handoff.
    if repair {
        if !json {
            info!("Repairing {} broken file(s)…", broken.len());
        }
        // Re-emit a transition event so JSON consumers can switch
        // their UI phase without parsing install_game's events
        // ambiguously (install_game uses `"phase":"download"` /
        // `"current_file"` of its own).
        if json {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({
                    "event": "progress",
                    "phase": "repair",
                    "files_done": 0,
                    "total_files": broken.len(),
                })
            );
            let _ = std::io::stdout().flush();
        }

        install_game(
            maxima_arc.clone(),
            slug,
            path,
            None,           // live build — same one we just verified against
            &broken,        // replace_files = list of broken
            true,           // only_listed_files
            json,
        )
        .await?;

        if json {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({
                    "event": "done",
                    "verified": total,
                    "broken": broken.len(),
                    "repaired": broken.len(),
                    "elapsed_secs": start_ts.elapsed().as_secs_f64(),
                })
            );
            let _ = std::io::stdout().flush();
        }
    } else if json {
        // No --repair: end with a `done` event but flagging broken
        // count so the consumer can decide whether to follow up with
        // an explicit repair invocation.
        let _ = writeln!(
            std::io::stdout(),
            "{}",
            serde_json::json!({
                "event": "done",
                "verified": total,
                "broken": broken.len(),
                "repaired": 0,
                "elapsed_secs": start_ts.elapsed().as_secs_f64(),
            })
        );
        let _ = std::io::stdout().flush();
    }

    Ok(())
}

async fn locate_game(maxima_arc: LockedMaxima, path: &str) -> Result<()> {
    let path = PathBuf::from(path);
    let manifest = manifest::read(path.join(MANIFEST_RELATIVE_PATH)).await?;
    manifest.run_touchup(&path).await?;
    info!("Installed!");
    Ok(())
}

async fn do_cloud_sync(maxima_arc: LockedMaxima, game_slug: &str, write: bool) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;
    let offer = maxima
        .mut_library()
        .game_by_base_slug(game_slug)
        .await?
        .unwrap()
        .clone();

    info!("Got offer");

    let lock = maxima
        .cloud_sync()
        .obtain_lock(
            &offer,
            if write {
                CloudSyncLockMode::Write
            } else {
                CloudSyncLockMode::Read
            },
        )
        .await?;
    let res = lock.sync_files().await;
    lock.release().await?;
    res?;

    info!("Done");

    Ok(())
}

async fn start_game(
    offer_id: &str,
    game_path_override: Option<String>,
    game_args: Vec<String>,
    login: Option<String>,
    steam_app_id: Option<String>,
    maxima_arc: LockedMaxima,
) -> Result<()> {
    {
        let mut maxima = maxima_arc.lock().await;
        maxima.start_lsx(maxima_arc.clone()).await?;

        if login.is_none() {
            maxima.rtm().login().await?;

            let friends = maxima.friends(0).await?;
            let players: Vec<String> = friends.iter().map(|f| f.id().to_owned()).collect();
            info!("Subscribed to {} players", players.len());

            maxima.rtm().subscribe(&players).await?;
        }
    }

    let launch_options = LaunchOptions {
        path_override: game_path_override,
        arguments: game_args,
        cloud_saves: true,
        steam_app_id,
    };

    if login.is_none() {
        launch::start_game(
            maxima_arc.clone(),
            LaunchMode::Online(offer_id.to_owned()),
            launch_options,
        )
        .await?;
    } else if let Some(captures) = MANUAL_LOGIN_PATTERN.captures(&login.unwrap()) {
        let persona = &captures[1];
        let password = &captures[2];

        launch::start_game(
            maxima_arc.clone(),
            LaunchMode::OnlineOffline(offer_id.to_owned(), persona.to_owned(), password.to_owned()),
            launch_options,
        )
        .await?;
    }

    loop {
        let mut maxima = maxima_arc.lock().await;

        for event in maxima.consume_pending_events() {
            match event {
                MaximaEvent::ReceivedLSXRequest(_pid, _request) => (),
                _ => {}
            }
        }

        maxima.update().await;
        if maxima.playing().is_none() {
            break;
        }

        drop(maxima);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(())
}

/// Long-running "passive LSX server" mode — the CLI equivalent of leaving the
/// Maxima UI open.
///
/// Why this exists: the catornot/patch-external-lsx scenario only works
/// reliably when the LSX server's `maxima.playing()` is None at the moment
/// the game establishes its socket. `maxima-cli launch` always sets
/// `playing = Some(...)` immediately before spawning bootstrap, so when the
/// game connects a few seconds later the LSX handlers go down the
/// "Some(context)" branch in `Connection::new` (Kyber PID lookup, RTM
/// presence updates, real OOA license requests, etc.). On Windows that's
/// fine; on macOS/CrossOver the user reports it triggers TF2's
/// "Engine Error: File corruption detected" tamper dialog.
///
/// `maxima-cli serve` decouples the two halves of the launch:
///
///   1. Terminal 1: `maxima-cli.exe serve` — logs in, opens the LSX listener
///      on the configured port (`MAXIMA_LSX_PORT` or 3216), optionally logs
///      in to RTM, and parks.
///   2. Terminal/Steam/Northstar: launch the game by any means that gets
///      `EALsxPort=<that port>` into the process environment (Steam's
///      `applaunch`, Draconis's vanilla / Northstar launch, or a `cxstart`
///      against `Titanfall2.exe` after manually setting the env var).
///
/// When the game connects, the server sees `playing=None`, takes the
/// catornot external-LSX path (now correctly defended in
/// `license.rs` / `profile.rs::set_presence`), and the auth flow proceeds.
///
/// This loop deliberately does NOT call `maxima.update()` — `update_playing_status`
/// is a no-op when `playing` is None and we don't want the content manager
/// poking at downloads from a serve session. Ctrl-C is the exit path.
async fn serve_lsx(maxima_arc: LockedMaxima, no_rtm: bool) -> Result<()> {
    {
        let mut maxima = maxima_arc.lock().await;
        maxima.start_lsx(maxima_arc.clone()).await?;
        info!("LSX server listening on port {}", maxima.lsx_port());

        // Bring up the HTTP `/authorize` endpoint too. Bootstrap probes
        // this when handling `link2ea://` / `origin2://` and forwards the
        // offer here instead of spawning a duplicate `maxima-cli launch`.
        // Failure to bind isn't fatal — LSX is what TF2 strictly needs,
        // and bootstrap falls back to the legacy spawn path if the probe
        // can't reach us.
        if let Err(err) = maxima.start_auth_server(maxima_arc.clone()).await {
            warn!(
                "Authorize HTTP server failed to start ({}); bootstrap will fall back \
                 to spawning maxima-cli launch on link2ea://.",
                err
            );
        }

        if !no_rtm {
            // Best-effort RTM login: it's only needed for friends presence /
            // SetPresence handlers. A failure here shouldn't bring down the
            // LSX server.
            if let Err(err) = maxima.rtm().login().await {
                warn!("RTM login failed (continuing without presence): {}", err);
            } else {
                match maxima.friends(0).await {
                    Ok(friends) => {
                        let players: Vec<String> =
                            friends.iter().map(|f| f.id().to_owned()).collect();
                        if let Err(err) = maxima.rtm().subscribe(&players).await {
                            warn!("Failed to subscribe to friends presence: {}", err);
                        } else {
                            info!("Subscribed to {} friends for presence", players.len());
                        }
                    }
                    Err(err) => warn!("Failed to fetch friends list: {}", err),
                }
            }
        }
    }

    info!(
        "Serving LSX. Launch your game externally (Steam / Draconis / etc.); press Ctrl-C to stop."
    );

    // Park indefinitely. Tick `maxima.update()` so when a game launched
    // via `/authorize` exits, `update_playing_status` notices the
    // bootstrap child has finished, runs cloud-save sync, and clears
    // `maxima.playing` — leaving the server ready to handle the next
    // launch with a clean state.
    loop {
        {
            let mut maxima = maxima_arc.lock().await;
            for event in maxima.consume_pending_events() {
                if let MaximaEvent::ReceivedLSXRequest(pid, request) = event {
                    debug!("LSX request from pid={}: {:?}", pid, request);
                }
            }
            maxima.update().await;
            // Heartbeat RTM so presence stays fresh (no-op if RTM wasn't started).
            if !no_rtm {
                if let Err(err) = maxima.rtm().heartbeat().await {
                    warn!("RTM heartbeat failed: {}", err);
                }
            }
        }
        // 1s tick — enough to detect game exit promptly without burning
        // CPU. Lock contention with LSX handlers is negligible at this
        // cadence.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
