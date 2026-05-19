#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//extern crate windows_service;

use std::env::current_exe;
use std::path::{Path, PathBuf};
use std::error::Error;
use std::string::FromUtf8Error;
use thiserror::Error;
use tokio::process::Command;

use base64::{engine::general_purpose, Engine};
use maxima::auth_server::AUTHORIZE_PORT;
use maxima::core::launch::BootstrapLaunchArgs;
use maxima::util::native::NativeError;
#[cfg(windows)]
use maxima::util::service::{is_service_valid, register_service};
use maxima::util::BackgroundServiceControlError;
use url::Url;

#[cfg(target_os = "macos")]
mod macos;

/// Validates that an offer_id is one of the safe identifier shapes we'll
/// forward to `maxima-cli launch`.
///
/// Two forms are accepted:
///
/// 1. **EA Origin offer id** — `Origin.OFR.<digits>.<digits>` (e.g.
///    `Origin.OFR.50.0002694`). Emitted by EA Desktop and by games launched
///    directly outside Steam.
/// 2. **Pure-numeric Steam App ID** — e.g. `1237970` (Titanfall 2 on Steam).
///    Emitted by EA-published games when launched from inside Steam, where
///    the URL looks like `link2ea://launchgame/1237970?platform=steam&theme=tf2`.
///    `maxima-cli`'s exhaustive library lookup resolves these against the
///    user's owned games (matching against `product.id`, `offer.content_id`,
///    etc., not just the slug).
///
/// This is a defense against command-line injection: protocol handler URLs
/// (`link2ea://`, `origin2://`) are attacker-controlled. Without validation,
/// an attacker could craft a URL like `link2ea://launchgame/--login=stolen_token`
/// and `maxima-cli` would interpret `--login` as a flag, bypassing OAuth.
/// Both accepted shapes start with either an ASCII letter or digit, so flag
/// injection is structurally impossible.
fn is_valid_ea_offer_id(s: &str) -> bool {
    is_valid_origin_offer_id(s) || is_valid_steam_app_id(s)
}

fn is_valid_origin_offer_id(s: &str) -> bool {
    let mut parts = s.split('.');
    if parts.next() != Some("Origin") {
        return false;
    }
    if parts.next() != Some("OFR") {
        return false;
    }
    let Some(major) = parts.next() else { return false };
    let Some(minor) = parts.next() else { return false };
    if parts.next().is_some() {
        return false;
    }
    !major.is_empty()
        && !minor.is_empty()
        && major.chars().all(|c| c.is_ascii_digit())
        && minor.chars().all(|c| c.is_ascii_digit())
}

fn is_valid_steam_app_id(s: &str) -> bool {
    // 1..=10 digits covers every Steam App ID issued (current max is ~3M).
    // Reject empty (covers `link2ea://launchgame/` with no segment) and
    // anything that includes a non-digit (defends against `12--login=x`).
    !s.is_empty() && s.len() <= 10 && s.chars().all(|c| c.is_ascii_digit())
}

/// Append a one-liner to `%TEMP%/maxima_execution.log`. Bootstrap is a
/// GUI subsystem binary so it has no console of its own — this file is
/// the only feedback channel for what happened during a protocol-handler
/// invocation. Best-effort; failures are silently ignored.
fn log_event(line: &str) {
    let path = std::env::temp_dir().join("maxima_execution.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(
            file,
            "[{:?}] {}",
            std::time::SystemTime::now(),
            line
        );
    }
}

/// Quick TCP probe — does the `/authorize` HTTP server look reachable?
/// Used before paying for a full reqwest round-trip.
///
/// Uses tokio's async `TcpStream::connect` wrapped in `timeout` so it
/// doesn't block the executor thread. (`std::net::TcpStream::connect_timeout`
/// inside an async fn parks a worker for up to the timeout duration,
/// which we don't want.)
async fn auth_server_alive(port: u16) -> bool {
    let addr = format!("127.0.0.1:{}", port);
    matches!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            tokio::net::TcpStream::connect(&addr),
        )
        .await,
        Ok(Ok(_))
    )
}

/// Hand a `link2ea://` or `origin2://` URL off to whichever Maxima
/// already speaks `/authorize`, or fall back to the legacy
/// `maxima-cli launch` spawn if nothing's listening.
///
/// The fall-back path preserves the upstream behavior (and the `link2ea`
/// flow Draconis used before `serve`-mode existed), so this rewrite
/// doesn't regress users who never type `maxima-cli serve` — they just
/// don't get the benefit of the always-on auth server.
///
/// See [`maxima::auth_server`] in `maxima-lib` for the server side.
async fn handle_protocol_authorize(
    offer_id: &str,
    cmd_params: Option<String>,
    protocol_name: &'static str,
) -> Result<bool, RunError> {
    // SECURITY: refuse anything that doesn't match the EA offer ID
    // shape. URLs like `link2ea://launchgame/--login=stolen_token`
    // would otherwise inject a flag into the maxima-cli invocation
    // below (or, worse, into the HTTP forward).
    if !is_valid_ea_offer_id(offer_id) {
        log_event(&format!(
            "REJECTED malformed {} offer_id: {:?}",
            protocol_name, offer_id
        ));
        return Ok(false);
    }

    let port = std::env::var("MAXIMA_AUTHORIZE_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(AUTHORIZE_PORT);

    if auth_server_alive(port).await {
        // Forward to the running Maxima. The server will refresh the
        // `.dlf`, set the EA-* env vars, and spawn the game executable
        // via `launch::start_game` — that's the chain TF2's Origin
        // DRM stub expects when it emits `link2ea://` and exits.
        let mut url = format!(
            "http://127.0.0.1:{}/authorize?offer_id={}",
            port,
            urlencoding::encode(offer_id)
        );
        if let Some(ref params) = cmd_params {
            // Re-encode the param value (URL we got it from might have
            // used `+` for space or other quirks). The server URL-decodes
            // on its end.
            url.push_str("&cmd_params=");
            url.push_str(&urlencoding::encode(params));
        }
        log_event(&format!(
            "Forwarding {} offer={} to auth server at {}",
            protocol_name, offer_id, url
        ));

        // Long timeout: the very first call after `serve` boots may
        // hit `request_and_save_license` which makes an EA license-
        // server round-trip (typically <2s, but Wine + spotty network
        // can push it higher).
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        let resp = client.post(&url).send().await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.is_success() {
            log_event(&format!(
                "Auth server accepted {} authorize for {} (body: {})",
                protocol_name, offer_id, body
            ));
            return Ok(true);
        }
        // Server is alive but rejected the request. Don't fall back to
        // spawning `maxima-cli launch` — that would just re-attempt the
        // same operation through a different code path and produce a
        // duplicate side-effect (a second TF2 process) without resolving
        // the underlying problem (not logged in, offer not in library).
        log_event(&format!(
            "Auth server rejected {} authorize for {} ({}, body: {})",
            protocol_name, offer_id, status, body
        ));
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Maxima authorize for {} failed with HTTP {}: {}",
                offer_id, status, body
            ),
        )
        .into());
    }

    // No `/authorize` server reachable. Fall back to the legacy path:
    // spawn `maxima-cli launch <offer_id>` which will start LSX, log in
    // if needed, do its own license preflight, and spawn the game via
    // `launch::start_game`. This is what bootstrap did before the
    // auth-server existed; it stays here so users who never run
    // `maxima-cli serve` (or whose `serve` hasn't started yet) still get
    // a working launch path.
    log_event(&format!(
        "No auth server on 127.0.0.1:{}; falling back to maxima-cli launch for {} offer={}",
        port, protocol_name, offer_id
    ));

    let mut child = Command::new(current_exe()?.with_file_name("maxima-cli.exe"));

    if let Ok(port) = std::env::var("KYBER_INTERFACE_PORT") {
        child.env("KYBER_INTERFACE_PORT", port);
    }
    if let Some(params) = cmd_params {
        let decoded = urlencoding::decode(&params)
            .map(|c| c.into_owned())
            .unwrap_or(params);
        child.env("MAXIMA_LAUNCH_ARGS", decoded.replace("\\\"", "\""));
    }

    child.args(["launch", offer_id]);
    let status = child.spawn()?.wait().await?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "maxima-cli ({}) exited non-zero: code={:?}",
                protocol_name,
                status.code()
            ),
        )
        .into());
    }
    Ok(true)
}

#[derive(Error, Debug)]
pub(crate) enum RunError {
    #[error(transparent)]
    BackgroundService(#[from] BackgroundServiceControlError),
    #[error(transparent)]
    Base64(#[from] base64::DecodeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    ParseUrl(#[from] url::ParseError),
    #[error(transparent)]
    ParseUtf8(#[from] FromUtf8Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

#[cfg(not(target_os = "macos"))]
#[tokio::main]
async fn main() -> Result<(), RunError> {
    // Immediate entry log
    if let Ok(temp_dir) = std::env::var("TEMP").map(PathBuf::from).or_else(|_| Ok::<PathBuf, RunError>(std::env::temp_dir())) {
        let debug_log = temp_dir.join("maxima_execution.log");
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&debug_log) {
            use std::io::Write;
            let _ = writeln!(file, "BOOTSTRAP MAIN START at {:?} | Raw Args: {:?}", std::time::SystemTime::now(), std::env::args().collect::<Vec<_>>());
        }
    }

    let _ = handle_launch_args().await?;

    Ok(())
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<()> {
    use cacao::appkit::App;

    use crate::macos::MaximaBootstrapApp;

    let handle = tokio::runtime::Handle::current();
    App::new(
        "dev.armchairdevelopers.MaximaBootstrap",
        MaximaBootstrapApp::new(handle),
    )
    .run();

    Ok(())
}

async fn handle_launch_args() -> Result<bool, RunError> {
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0);

    let result = run(&args).await;
    let str_result = result
        .as_ref()
        .map_err(|e| {
            let source = e.source();
            let error_str = if source.is_some() {
                source.unwrap().to_string()
            } else {
                e.to_string()
            };

            error_str
        })
        .err()
        .unwrap_or("Success".to_string());
        
    // Unconditional debug log to verify execution (APPEND)
    let temp_dir = std::env::temp_dir();
    let debug_log = temp_dir.join("maxima_execution.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&debug_log) {
        use std::io::Write;
        let _ = writeln!(file, "Maxima Bootstrap executed at {:?}\nArgs: {:?}\nResult: {}\n---", std::time::SystemTime::now(), args, str_result);
    }

    if str_result != "Success" {
        let log_path = temp_dir.join("maxima_bootstrap_error.log");
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write;
            let _ = writeln!(file, "Maxima Bootstrap Error at {:?}: {}", std::time::SystemTime::now(), str_result);
        }
        
        // Try a very simple path as well
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("C:\\maxima_debug_error.log") {
            use std::io::Write;
            let _ = writeln!(file, "Maxima Bootstrap Error at {:?}: {}", std::time::SystemTime::now(), str_result);
        }
    }

    if cfg!(debug_assertions) || std::env::var("MAXIMA_DEBUG").is_ok() {
        println!("Args: {:?}", &args);
        println!("Result: {}", str_result);

        // Pause terminal
        //std::io::Read::read(&mut std::io::stdin(), &mut [0]).unwrap();
    }

    result
}

#[cfg(windows)]
fn service_setup() -> Result<(), BackgroundServiceControlError> {
    if is_service_valid()? {
        return Ok(());
    }

    register_service()?;

    Ok(())
}

#[cfg(not(windows))]
fn service_setup() -> Result<(), BackgroundServiceControlError> {
    Ok(())
}

#[cfg(windows)]
async fn platform_launch(args: BootstrapLaunchArgs) -> Result<(), NativeError> {
    let mut binding = Command::new(&args.path);
    let child = binding.args(&args.args);

    let temp_dir = std::env::temp_dir();
    let debug_log = temp_dir.join("maxima_execution.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&debug_log) {
        use std::io::Write;
        let _ = writeln!(file, "PLATFORM_LAUNCH: Executing {:?} with args {:?}", args.path, args.args);
    }

    let status = child.spawn()?.wait().await?;
    if !status.success() {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Game exited with code: {:?}", status.code())).into());
    }
    Ok(())
}

#[cfg(unix)]
async fn platform_launch(args: BootstrapLaunchArgs) -> Result<(), NativeError> {
    use maxima::unix::wine::run_wine_command;
    use maxima::unix::wine::CommandType;

    run_wine_command(
        args.path,
        Some(args.args),
        None,
        false,
        CommandType::WaitForExitAndRun,
    )
    .await?;

    Ok(())
}

async fn run(args: &[String]) -> Result<bool, RunError> {
    let len = args.len();
    if len == 1 {
        let arg = &args[0];

        if arg == "--noop" {
            return Ok(true);
        }

        if arg.starts_with("link2ea") {
            // link2ea://launchgame/<offer-id>?platform=<p>&theme=<t>
            // link2ea://resume/<offer-id>?...
            //
            // The offer id is the first path segment after the action.
            let url = Url::parse(arg)?;
            let segments: Vec<&str> = url
                .path_segments()
                .map(|c| c.collect())
                .unwrap_or_default();
            if segments.is_empty() {
                return Ok(false);
            }
            let offer_id = segments[0];
            let cmd_params = url.query().and_then(|q| {
                querystring::querify(q)
                    .into_iter()
                    .find(|(k, _)| *k == "cmdParams")
                    .map(|(_, v)| v.to_string())
            });
            return handle_protocol_authorize(offer_id, cmd_params, "link2ea").await;
        }

        if arg.starts_with("origin2") {
            // origin2://game/launch?offerIds=<offer_id>&cmdParams=<encoded_args>&...
            let url = Url::parse(arg)?;
            let query = querystring::querify(url.query().unwrap_or_default());
            let offer_id: String = query
                .iter()
                .find(|(k, _)| *k == "offerIds")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            let cmd_params = query
                .iter()
                .find(|(k, _)| *k == "cmdParams")
                .map(|(_, v)| v.to_string());
            return handle_protocol_authorize(&offer_id, cmd_params, "origin2").await;
        }

        if arg.starts_with("qrc") {
            // Guard against malformed qrc:// URLs — splitn(2, ...) gives at most two
            // segments, so we won't panic on inputs that lack the marker.
            let parts: Vec<&str> = arg.splitn(2, "login_successful.html?").collect();
            let Some(query) = parts.get(1) else {
                return Ok(false);
            };
            reqwest::get(format!("http://127.0.0.1:31033/auth?{}", query)).await?;

            return Ok(true);
        }

        return Ok(false);
    }

    if len > 1 {
        let command = &args[0];
        let handled = match command.as_str() {
            "launch" => {
                let decoded = general_purpose::STANDARD.decode(&args[1])?;
                let launch_args: BootstrapLaunchArgs = serde_json::from_slice(&decoded)?;
                platform_launch(launch_args).await?;

                true
            }
            _ => false,
        };
        return Ok(handled);
    }

    service_setup()?;

    Ok(false)
}
