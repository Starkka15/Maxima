//! HTTP server that handles `link2ea://` / `origin2://` auth handoffs.
//!
//! ## Why this exists
//!
//! Upstream's bootstrap treats `link2ea://launchgame/<offer_id>` as
//! "launch this game" — it spawns a fresh `maxima-cli launch <offer_id>`
//! which in turn calls `launch::start_game`, spawning the game executable.
//! That works as a one-shot but doesn't compose: if Draconis already has
//! a long-running Maxima session in the bottle (cached login, RTM, etc.),
//! every protocol-handler invocation re-bootstraps from scratch.
//!
//! Upstream's own tracking issue
//! [`#27 — Support launching Maxima from Epic/Steam`](https://github.com/ArmchairDevelopers/Maxima/issues/27)
//! describes the intended end state: a long-running Maxima server, a
//! protocol handler that consults it over IPC, and the server preparing
//! the OOA license + computing launch params before the handler spawns
//! the game. This module is our HTTP-based realization of that design
//! (D-Bus on Linux per the issue, plain TCP HTTP for our cross-OS
//! Wine bottle).
//!
//! ## Endpoints
//!
//! - `GET /`  →  `200 OK` body `maxima-auth-server`. Used by bootstrap as
//!   a liveness probe before deciding whether to forward or fall back to
//!   spawning a fresh `maxima-cli launch`.
//! - `POST /authorize?offer_id=<id>`  →  Validate login, resolve the offer
//!   (EA library lookup with [`crate::steam`] fallback for the install
//!   path), then call [`crate::core::launch::start_game`] which:
//!     1. Refreshes the OOA license via `request_and_save_license`
//!        (writes `…/EA Services/License/<content_id>.dlf`).
//!     2. Sets the EA-* environment variables required by the game
//!        (`EALsxPort`, `EAGenericAuthToken`, `EAAccessTokenJWS`, …).
//!     3. Spawns the game via the upstream bootstrap → game chain.
//!     4. Records `maxima.playing = Some(ActiveGameContext)` so the
//!        LSX server takes the active-launch branch when the game
//!        connects.
//!   Returns `200 OK` `{"status":"ok"}` once the spawn is in flight, or
//!   `4xx`/`5xx` with `{"status":"error","message":...}` on failure.
//!
//! ### Why /authorize spawns the game (not just preflight)
//!
//! Empirically, Titanfall 2's Origin DRM stub emits `link2ea://` and
//! **exits**, expecting whoever handles the URL to re-launch it with
//! EA auth context (`EAGenericAuthToken` etc.) in the environment. A
//! preflight-only endpoint would refresh the `.dlf` but leave the game
//! closed. By calling `launch::start_game` we hand the spawned game a
//! full EA env — same as the upstream UI does via its Play button.
//! `maxima.playing` ends up `Some(...)` and the LSX flow goes down
//! the standard active-launch branch (not the catornot external-LSX
//! branch).

use std::sync::Arc;

use log::{debug, error, info, warn};
use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::Duration;

/// Per-request total deadline. A legitimate `/authorize` finishes in
/// well under 2s (license preflight + spawn); anything longer is either
/// EA's licensing service is having a bad day, or a slow / malicious
/// local client trying to pin our task indefinitely. 30s is generous
/// enough not to cut off the slow-but-real case while still keeping
/// the worker free for the next request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Cap total bytes we'll read from one connection at 8 KiB. This is the
/// request line plus all headers (we never read a body). 8 KiB is the
/// same number Nginx's `large_client_header_buffers` defaults to.
/// Without this cap, an attacker could send an arbitrarily long
/// request line on the loopback socket to exhaust memory.
const MAX_REQUEST_HEAD_BYTES: u64 = 8 * 1024;

use crate::core::{
    auth::storage::TokenError,
    launch::{self, LaunchError, LaunchMode, LaunchOptions},
    library::LibraryError,
    Maxima,
};
use crate::steam::{
    lookup_steam_game, lookup_steam_game_by_offer, resolve_steam_install_path,
    STEAM_APP_ID_PATTERN,
};

/// Default port for the authorize HTTP server. LSX is 3216; we pick
/// `lsx + 3` so the two stay together in `netstat` output but don't
/// collide. Override via `MAXIMA_AUTHORIZE_PORT` if anything ever clashes.
pub const AUTHORIZE_PORT: u16 = 13219;

#[derive(Error, Debug)]
pub enum AuthServerError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Bind the authorize HTTP listener and spawn the accept loop. Returns
/// once the listener is bound; errors inside the accept loop are logged
/// but don't propagate, so an LSX server already running stays up if
/// some transient socket error hits this listener.
pub async fn start_server(
    port: u16,
    maxima_arc: Arc<Mutex<Maxima>>,
) -> Result<(), AuthServerError> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    info!("Authorize HTTP server listening on {}", addr);

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((socket, peer)) => {
                    debug!("Authorize: new connection from {}", peer);
                    let maxima = maxima_arc.clone();
                    tokio::spawn(async move {
                        if let Err(err) = handle_connection(socket, maxima).await {
                            warn!("Authorize: request failed: {}", err);
                        }
                    });
                }
                Err(err) => {
                    error!("Authorize: accept failed: {}", err);
                    // Brief backoff so we don't hot-loop on a permanent
                    // listener error.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    });

    Ok(())
}

#[derive(Serialize)]
struct OkResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    status: &'static str,
    message: String,
}

/// Public entry point: wraps the real handler in a per-request
/// `tokio::time::timeout` so a stalled / hostile peer can't keep a
/// task pinned indefinitely. Slow-client mitigation for an
/// unauthenticated loopback HTTP listener.
async fn handle_connection(
    socket: TcpStream,
    maxima_arc: Arc<Mutex<Maxima>>,
) -> Result<(), std::io::Error> {
    match tokio::time::timeout(REQUEST_TIMEOUT, handle_connection_inner(socket, maxima_arc)).await {
        Ok(result) => result,
        Err(_) => {
            warn!(
                "Authorize: request exceeded {:?} timeout, dropping connection",
                REQUEST_TIMEOUT
            );
            Ok(())
        }
    }
}

async fn handle_connection_inner(
    mut socket: TcpStream,
    maxima_arc: Arc<Mutex<Maxima>>,
) -> Result<(), std::io::Error> {
    let (read_half, _) = socket.split();
    // `.take(N)` bounds the total bytes our BufReader will surface — once
    // the limit is reached, subsequent reads return 0 (EOF). A truncated
    // request line / header block then trips the HTTP parser below and
    // we respond 400 instead of hanging on the read.
    let mut reader = BufReader::new(read_half.take(MAX_REQUEST_HEAD_BYTES));

    // We only need the request line — the body is empty for our endpoints
    // and headers carry nothing we care about. Drain enough to keep the
    // peer's send buffer happy, then respond.
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_response(&mut socket, 400, "Bad Request", b"").await;
    }
    let method = parts[0].to_string();
    let path_and_query = parts[1].to_string();

    // Drain headers (until empty line). HTTP/1.1 requires this even if
    // we don't read further data — without it, some clients refuse to
    // read the response.
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header).await?;
        if n == 0 || header == "\r\n" || header == "\n" {
            break;
        }
    }

    // GET / — health probe used by bootstrap.
    if method == "GET" && (path_and_query == "/" || path_and_query.starts_with("/?")) {
        return write_response(&mut socket, 200, "OK", b"maxima-auth-server").await;
    }

    // POST /authorize?offer_id=...&cmd_params=...
    if method == "POST" && path_and_query.starts_with("/authorize") {
        let offer_id = extract_query_param(&path_and_query, "offer_id");
        let cmd_params = extract_query_param(&path_and_query, "cmd_params");
        return match handle_authorize(
            offer_id.as_deref(),
            cmd_params.as_deref(),
            maxima_arc,
        )
        .await
        {
            Ok(()) => {
                let body = serde_json::to_vec(&OkResponse { status: "ok" })
                    .unwrap_or_else(|_| b"{}".to_vec());
                write_json_response(&mut socket, 200, "OK", &body).await
            }
            Err(err) => {
                let (status, reason) = err.http_status();
                let body = serde_json::to_vec(&ErrorResponse {
                    status: "error",
                    message: err.to_string(),
                })
                .unwrap_or_else(|_| b"{}".to_vec());
                write_json_response(&mut socket, status, reason, &body).await
            }
        };
    }

    write_response(&mut socket, 404, "Not Found", b"").await
}

#[derive(Error, Debug)]
enum AuthorizeError {
    #[error("missing offer_id query parameter")]
    MissingOfferId,
    #[error("not logged in — open the Maxima UI / CLI once to authenticate first")]
    NotLoggedIn,
    #[error("no owned offer '{0}' in EA library — link your Steam account at https://www.ea.com")]
    OfferNotFound(String),
    #[error(transparent)]
    Token(#[from] TokenError),
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    Launch(#[from] LaunchError),
}

impl AuthorizeError {
    fn http_status(&self) -> (u16, &'static str) {
        match self {
            AuthorizeError::MissingOfferId => (400, "Bad Request"),
            AuthorizeError::NotLoggedIn | AuthorizeError::Token(_) => (401, "Unauthorized"),
            AuthorizeError::OfferNotFound(_) => (404, "Not Found"),
            // `LaunchError::NotInstalled` / `NoOfferFound` are also "not found"
            // shaped; map them precisely so curl users see a useful status.
            AuthorizeError::Launch(LaunchError::NotInstalled(_))
            | AuthorizeError::Launch(LaunchError::NoOfferFound(_))
            | AuthorizeError::Launch(LaunchError::GamePath) => (404, "Not Found"),
            AuthorizeError::Launch(LaunchError::BootstrapMissing) => (500, "Internal Server Error"),
            AuthorizeError::Library(_) | AuthorizeError::Launch(_) => (502, "Bad Gateway"),
        }
    }
}

/// Core authorize logic: validate login → resolve offer → call
/// `launch::start_game` which does the OOA license refresh, sets the
/// EA-* env vars, and spawns the game executable.
///
/// We accept an optional `cmd_params` query parameter — the URL-encoded
/// argument string from `link2ea://launchgame/<offer>?cmdParams=…`. It
/// is parsed and forwarded as additional launch args.
async fn handle_authorize(
    raw_offer_id: Option<&str>,
    cmd_params: Option<&str>,
    maxima_arc: Arc<Mutex<Maxima>>,
) -> Result<(), AuthorizeError> {
    let raw_offer_id = raw_offer_id.ok_or(AuthorizeError::MissingOfferId)?;
    info!("Authorize request for slug '{}'", raw_offer_id);

    // Steam emits `link2ea://launchgame/<numeric_steam_app_id>?platform=steam`
    // (e.g. `1237970` for TF2). EA Desktop's library is keyed by Origin
    // offer IDs like `Origin.OFR.50.0001456`, so we translate via the
    // STEAM_GAMES table before doing the library lookup. The original
    // slug is kept as `steam_app_id` to thread through to `launch.rs`
    // for SteamAppId/SteamGameId env-var setup on the spawned game.
    let (offer_id, steam_app_id): (String, Option<String>) =
        if STEAM_APP_ID_PATTERN.is_match(raw_offer_id) {
            match lookup_steam_game(raw_offer_id) {
                Some(entry) => {
                    info!(
                        "Steam App ID '{}' resolved to Origin offer ID '{}'",
                        raw_offer_id, entry.origin_offer_id
                    );
                    (
                        entry.origin_offer_id.to_owned(),
                        Some(raw_offer_id.to_owned()),
                    )
                }
                None => {
                    warn!(
                        "Steam App ID '{}' is not in the STEAM_GAMES table; \
                         passing through directly (will likely 404 in library lookup)",
                        raw_offer_id
                    );
                    (raw_offer_id.to_owned(), Some(raw_offer_id.to_owned()))
                }
            }
        } else {
            // Looks like an Origin offer ID already (TF2 itself emits
            // these mid-run; older EA-Desktop-style launches go this
            // path too).
            (raw_offer_id.to_owned(), None)
        };

    // Phase 1: cheap pre-checks. Drop the lock before
    // `launch::start_game` re-acquires it, so we don't deadlock.
    {
        let mut maxima = maxima_arc.lock().await;

        // `logged_in()` re-validates the cached token so an expired
        // account doesn't fall through to a confusing 502.
        let logged_in = {
            let mut auth_storage = maxima.auth_storage().lock().await;
            auth_storage.logged_in().await.unwrap_or(false)
        };
        if !logged_in {
            return Err(AuthorizeError::NotLoggedIn);
        }

        // Confirm the (translated) offer is in the user's EA library
        // here so we can give a clean 404 ("link your accounts at
        // ea.com") instead of bubbling a less-helpful
        // `LaunchError::NoOfferFound` later.
        if maxima
            .mut_library()
            .game_by_base_offer(&offer_id)
            .await?
            .is_none()
        {
            return Err(AuthorizeError::OfferNotFound(offer_id.clone()));
        }
    }

    // Phase 2: build LaunchOptions. The Steam-install path fallback is
    // crucial for Titanfall 2 from Steam — EA Desktop has no record of
    // the install, so `launch::start_game` would bail with
    // `LaunchError::NotInstalled` without an explicit override.
    let path_override = lookup_steam_game_by_offer(&offer_id)
        .and_then(resolve_steam_install_path)
        .and_then(|p| p.to_str().map(str::to_owned));
    if let Some(ref p) = path_override {
        info!("Resolved Steam install path for {}: {}", offer_id, p);
    }

    let arguments = cmd_params
        .map(|raw| {
            // URL-decode then split on whitespace, respecting basic
            // double-quote grouping (same shape `MAXIMA_LAUNCH_ARGS`
            // expects elsewhere in the codebase).
            let decoded = urlencoding::decode(raw)
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| raw.to_owned())
                .replace("\\\"", "\"");
            launch::parse_arguments(&decoded)
        })
        .unwrap_or_default();

    let launch_options = LaunchOptions {
        path_override,
        arguments,
        cloud_saves: true,
        // Threading the original Steam App ID (if any) through to
        // `launch.rs` makes it set SteamAppId/SteamGameId env vars on
        // the spawned game and auto-inject -noOriginStartup / -multiple
        // launch args — without this TF2 from Steam exits with code
        // 100010 "Steam not detected".
        steam_app_id,
    };

    // Phase 3: hand off to the upstream launch flow. This refreshes the
    // license, populates the EA-* env vars (including SteamAppId via
    // `LaunchOptions.steam_app_id`), spawns bootstrap → game, and sets
    // `maxima.playing = Some(ActiveGameContext)` so the LSX server
    // takes the active-launch branch when the game connects.
    launch::start_game(
        maxima_arc.clone(),
        LaunchMode::Online(offer_id.clone()),
        launch_options,
    )
    .await?;

    info!("Game launched for offer '{}'", offer_id);
    Ok(())
}

fn extract_query_param(path_and_query: &str, key: &str) -> Option<String> {
    let qs = path_and_query.split_once('?').map(|(_, q)| q)?;
    for pair in qs.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(
                    urlencoding::decode(v)
                        .map(|c| c.into_owned())
                        .unwrap_or_else(|_| v.to_owned()),
                );
            }
        }
    }
    None
}

async fn write_response(
    socket: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &[u8],
) -> Result<(), std::io::Error> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        body.len()
    );
    socket.write_all(head.as_bytes()).await?;
    if !body.is_empty() {
        socket.write_all(body).await?;
    }
    socket.flush().await?;
    Ok(())
}

async fn write_json_response(
    socket: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &[u8],
) -> Result<(), std::io::Error> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        body.len()
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(body).await?;
    socket.flush().await?;
    Ok(())
}
