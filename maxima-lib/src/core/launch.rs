use base64::{engine::general_purpose, Engine};
use derive_getters::Getters;
use log::{error, info, warn};
use std::{env, fmt::Display, path::PathBuf, sync::Arc};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};
use uuid::Uuid;

use crate::{
    core::{
        auth::{
            context::AuthContext,
            nucleus_auth_exchange,
            storage::{AuthError, TokenError},
        },
        clients::JUNO_PC_CLIENT_ID,
        cloudsync::{CloudSyncError, CloudSyncLockMode},
        library::{LibraryError, OwnedOffer},
        service_layer::ServiceLayerError,
        Maxima,
    },
    ooa::{needs_license_update, request_and_save_license, LicenseAuth, LicenseError},
    util::{
        native::{NativeError, SafeParent, SafeStr},
        registry::bootstrap_path,
        simple_crypto,
    },
};
use thiserror::Error;

#[cfg(unix)]
use crate::unix::fs::case_insensitive_path;

use serde::{Deserialize, Serialize};

#[derive(Error, Debug)]
pub enum LaunchError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    CloudSync(#[from] CloudSyncError),
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    License(#[from] LicenseError),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    ServiceLayer(#[from] ServiceLayerError),
    #[error(transparent)]
    Token(#[from] TokenError),

    #[error("no offer was found for id `{0}`")]
    NoOfferFound(String),
    #[error("offline mode is not yet supported")]
    Offline,
    #[error("game path must be specified when launching in OnlineOffline mode")]
    GamePathOffline,
    #[error("game path not found")]
    GamePath,
    #[error("`{0}` is not installed")]
    NotInstalled(String),
    #[error("bootstrap was not found! Please re-install maxima")]
    BootstrapMissing,
    #[error(
        "content ID (`{0}`) was specified as an offer ID when launching in OnlineOffline mode"
    )]
    ContentIdAsOfferId(String),
}

pub enum StartupStage {
    Launch,
    ConnectionEstablished,
}

pub struct LibraryInjection {
    pub path: PathBuf,
    pub stage: StartupStage,
}

pub struct LaunchOptions {
    pub path_override: Option<String>,
    pub arguments: Vec<String>,
    pub cloud_saves: bool,
    /// When set, the game is being launched from Steam context. Steam
    /// emits `link2ea://launchgame/<numeric_steam_app_id>?platform=steam`
    /// expecting the link2ea handler to take over the launch entirely
    /// (Steam does NOT spawn the exe itself for older EA-on-Steam titles
    /// like TF2 — it delegates to whatever owns the link2ea protocol).
    ///
    /// Passing `Some(steam_app_id)` causes `start_game` to:
    ///   1. Set `EAEntitlementSource` / `EAExternalSource` / `EALaunchOwner`
    ///      to `"Steam"` instead of `"EA"` so the DRM stub sees a launch
    ///      context consistent with where it's being run from.
    ///   2. Set `SteamAppId` / `SteamGameId` env vars on the spawned game
    ///      (required by the Steam DRM stub — without these the game exits
    ///      immediately with code 100010 "Steam not detected").
    ///   3. Default `SteamClientLaunch=1` and `SteamPath=...` if the
    ///      parent env doesn't already provide them.
    ///
    /// `None` (the default) is the EA-Desktop-style launch path — env
    /// vars stay `"EA"` and no Steam-specific setup happens.
    ///
    /// Note: per-game launch args (e.g. `-noOriginStartup` for Northstar,
    /// `-multiple` for Source-engine titles) are NOT auto-injected. Callers
    /// who need them pass them via `arguments`, `MAXIMA_LAUNCH_ARGS`, or
    /// `cmd_params` on the `link2ea://` URL.
    pub steam_app_id: Option<String>,
}

pub enum LaunchMode {
    /// Completely offline, relies on cached license files and user IDs
    Offline(String), // Offer ID
    /// Online, makes requests about the user and licensing
    Online(String), // Offer ID
    /// Online, but only for license requests; everything else uses dummy offer and user IDs
    /// Content ID, Game executable path, and username/password must be specified
    OnlineOffline(String, String, String), // Content ID, Persona, Password
}

impl LaunchMode {
    // What an awful name
    pub fn is_online_offline(&self) -> bool {
        match self {
            LaunchMode::OnlineOffline(_, _, _) => true,
            _ => false,
        }
    }
}

#[derive(Getters)]
pub struct ActiveGameContext {
    launch_id: String,
    game_path: String,
    content_id: String,
    offer: Option<OwnedOffer>,
    mode: LaunchMode,
    injections: Vec<LibraryInjection>,
    cloud_saves: bool,
    /// The Steam App ID this launch came from, if any. Threaded through
    /// from `LaunchOptions.steam_app_id` so the LSX request handlers
    /// (specifically `GetProfile` and `GetAllGameInfo`) can return
    /// consistent values for `IsSteamSubscriber` / `EntitlementSource`
    /// without resorting to reading `env::var("SteamAppId")` from the
    /// serve process (which doesn't have it — those env vars are set
    /// directly on the spawned game's `Command`, not on the parent).
    ///
    /// `None` means this is an EA-Desktop-style launch (TF2 emitting
    /// `link2ea://launchgame/Origin.OFR.…` mid-run, or maxima-cli launch
    /// with an Origin offer ID slug).
    steam_app_id: Option<String>,
    process: Child,
    started: bool,
}

impl ActiveGameContext {
    pub fn new(
        launch_id: &str,
        game_path: &str,
        cloud_saves: bool,
        content_id: &str,
        offer: Option<OwnedOffer>,
        mode: LaunchMode,
        steam_app_id: Option<String>,
        process: Child,
    ) -> Self {
        Self {
            launch_id: launch_id.to_owned(),
            game_path: game_path.to_owned(),
            content_id: content_id.to_owned(),
            offer,
            mode,
            injections: Vec::new(),
            cloud_saves,
            steam_app_id,
            process,
            started: false,
        }
    }

    pub fn set_started(&mut self) {
        self.started = true;
    }

    pub fn process_mut(&mut self) -> &mut Child {
        &mut self.process
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct BootstrapLaunchArgs {
    pub path: String,
    pub args: Vec<String>,
}

impl Display for LaunchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchMode::Offline(offer_id) => write!(f, "{}", offer_id),
            LaunchMode::Online(offer_id) => write!(f, "{}", offer_id),
            LaunchMode::OnlineOffline(content_id, _, _) => write!(f, "{}", content_id),
        }
    }
}

pub async fn start_game(
    maxima_arc: Arc<Mutex<Maxima>>,
    mode: LaunchMode,
    options: LaunchOptions,
) -> Result<(), LaunchError> {
    let mut maxima = maxima_arc.lock().await;
    info!("Initiating game launch with {}...", mode);

    if let LaunchMode::OnlineOffline(ref content_id, _, _) = mode {
        if options.path_override.is_none() {
            return Err(LaunchError::GamePathOffline);
        }

        if content_id.starts_with("Origin.OFR") {
            return Err(LaunchError::ContentIdAsOfferId(content_id.clone()));
        }
    }

    let (content_id, online_offline, offer, access_token) =
        if let LaunchMode::Online(ref offer_id) = mode {
            let access_token = &maxima.access_token().await?;
            let offer = match maxima.mut_library().game_by_base_offer(offer_id).await? {
                Some(offer) => offer,
                None => return Err(LaunchError::NoOfferFound(offer_id.clone())),
            };

            // Skip the EA-side install check when the caller supplied an
            // explicit path_override. This covers the Steam-launched case
            // where the game lives in Steam's library and EA Desktop has no
            // record of it — `offer.is_installed()` would return false even
            // though the binary is right there on disk.
            if options.path_override.is_none() && !offer.is_installed().await {
                return Err(LaunchError::NotInstalled(offer.offer_id().clone()));
            }

            let content_id = offer.offer().content_id().to_owned();

            (
                content_id,
                false,
                Some(offer.clone()),
                access_token.to_owned(),
            )
        } else if let LaunchMode::OnlineOffline(ref content_id, _, _) = mode {
            (content_id.to_owned(), true, None, String::new())
        } else if let LaunchMode::Offline(ref offer_id) = mode {
            // Offline: look up game from library but skip auth token
            let offer = match maxima.mut_library().game_by_base_offer(offer_id).await? {
                Some(offer) => offer,
                None => return Err(LaunchError::NoOfferFound(offer_id.clone())),
            };

            if !offer.is_installed().await {
                return Err(LaunchError::NotInstalled(offer.offer_id().clone()));
            }

            let content_id = offer.offer().content_id().to_owned();
            (content_id, false, Some(offer.clone()), String::new())
        } else {
            return Err(LaunchError::Offline);
        };

    // Need to move this into Maxima and have a "current game" system
    let path = if let Some(game_path_override) = options.path_override {
        PathBuf::from(&game_path_override)
    } else if !online_offline {
        match offer {
            Some(ref offer) => offer.execute_path(false).await?.clone(),
            None => return Err(LaunchError::NoOfferFound("Unknown".to_string())),
        }
    } else {
        return Err(LaunchError::GamePath);
    };

    let dir = path.safe_parent()?.safe_str()?;
    #[cfg(unix)]
    let path = case_insensitive_path(path.clone());
    let path = path.safe_str()?;
    info!("Game path: {}", path);

    #[cfg(unix)]
    mx_linux_setup().await?;

    match mode {
        LaunchMode::Offline(_) => {}
        LaunchMode::Online(_) => {
            let auth = LicenseAuth::AccessToken(maxima.access_token().await?);

            let offer = offer.as_ref().unwrap();

            // Diagnostic override: setting `MAXIMA_SKIP_LICENSE_WRITE=1` in the
            // environment makes us NOT fetch + write the `.dlf` license file
            // to `…/EA Services/License/<content_id>.dlf`. Used to test whether
            // TF2's "Engine Error: File corruption detected" symptom is driven
            // by the on-disk `.dlf` (hardware-hash mismatch hypothesis from
            // CLAUDE.md) — if TF2 still corrupts when we DON'T write a `.dlf`,
            // the issue is somewhere else (Steam DRM, local file integrity,
            // some other check). Remove the .dlf manually before testing so
            // there's no stale file lying around.
            if env::var("MAXIMA_SKIP_LICENSE_WRITE").is_ok() {
                warn!(
                    "MAXIMA_SKIP_LICENSE_WRITE is set — skipping OOA license \
                     fetch + .dlf write entirely. Game will only have whatever \
                     .dlf was already on disk (or none)."
                );
            } else if needs_license_update(&content_id).await? {
                info!(
                    "Requesting new game license for {}...",
                    offer.offer().display_name()
                );

                request_and_save_license(&auth, &content_id, path.to_owned().into()).await?;
            } else {
                info!("Existing game license is still valid, not updating");
            }

            if options.cloud_saves && offer.offer().has_cloud_save() {
                info!("Syncing with cloud save...");

                let result = maxima
                    .cloud_sync()
                    .obtain_lock(offer, CloudSyncLockMode::Read)
                    .await;
                if let Err(err) = result {
                    error!("Failed to obtain CloudSync read lock: {}", err);
                } else {
                    let lock = result?;

                    let result = lock.sync_files().await;
                    if let Err(err) = result {
                        error!("Failed to sync cloud save: {}", err);
                    } else {
                        info!("Cloud save synced");
                    }

                    lock.release().await?;
                }
            }
        }
        LaunchMode::OnlineOffline(_, ref persona, ref password) => {
            let auth = LicenseAuth::Direct(persona.to_owned(), password.to_owned());

            if needs_license_update(&content_id).await? {
                request_and_save_license(&auth, &content_id, path.to_owned().into()).await?;
            } else {
                info!("Existing game license is still valid, not updating");
            }
        }
    }

    let mut game_args = options.arguments.clone();

    // Append args from env
    if let Ok(args) = env::var("MAXIMA_LAUNCH_ARGS") {
        game_args.append(&mut parse_arguments(args.as_str()));
    }

    let is_steam_launch = options.steam_app_id.is_some();

    if !bootstrap_path()?.exists() {
        return Err(LaunchError::BootstrapMissing);
    }

    let mut child = Command::new(bootstrap_path()?);
    child.arg("launch");

    let bootstrap_args = BootstrapLaunchArgs {
        path: path.to_string(),
        args: game_args,
    };

    let b64 = general_purpose::STANDARD.encode(serde_json::to_string(&bootstrap_args)?);
    child.arg(b64);

    let user = maxima.local_user().await?;
    let launch_id = Uuid::new_v4().to_string();

    // Source / owner / entitlement env vars: "EA" for EA-Desktop-launched
    // games, "Steam" for games launched via Steam (the user clicked Play in
    // Steam). When mismatched, TF2 (and likely other EA-on-Steam titles)
    // throws a "corrupted game files" error because its DRM stub expects
    // the ownership tag to match its install context.
    let source_tag = if is_steam_launch { "Steam" } else { "EA" };

    child
        .current_dir(PathBuf::from(path).safe_parent()?)
        .env("MXLaunchId", launch_id.to_owned())
        .env("EAAuthCode", "unavailable")
        .env("EAEgsProxyIpcPort", "0")
        .env("EAEntitlementSource", source_tag)
        .env("EAExternalSource", source_tag)
        .env("EAFreeTrialGame", "false")
        .env("EAGameLocale", maxima.locale.full_str())
        .env("EAGenericAuthToken", access_token.to_owned())
        .env("EALaunchCode", "unavailable")
        .env("EALaunchOwner", source_tag)
        .env(
            "EALaunchEAID",
            user.player()
                .as_ref()
                .ok_or(ServiceLayerError::MissingField)?
                .display_name(),
        )
        .env("EALaunchEnv", "production")
        .env("EALaunchOfflineMode", "false")
        .env("EALsxPort", maxima.lsx_port.to_string())
        .env(
            "EARtPLaunchCode",
            simple_crypto::rtp_handshake().to_string(),
        )
        .env("EASecureLaunchTokenTemp", user.id())
        .env("EASteamProxyIpcPort", "0")
        .env("OriginSessionKey", launch_id.clone())
        .env("ContentId", content_id.clone())
        .env("EAOnErrorExitRetCode", "1");

    // Steam-Play env vars on the spawned child specifically (NOT via
    // `std::env::set_var` on the parent — that would persist for every
    // future spawn in the same process, which matters for the long-
    // running `maxima-cli serve` host where /authorize spawns multiple
    // games over its lifetime).
    //
    // The Steam DRM stub in EA-on-Steam titles reads `SteamAppId` /
    // `SteamGameId` during `SteamAPI_Init()`. If either is absent the
    // game exits immediately with code 100010 ("Steam not detected").
    // `SteamClientLaunch` and `SteamPath` are normally set by Steam's
    // own runtime; we default-fill them from the parent env (if Steam
    // really did launch us) or to safe constants otherwise.
    if let Some(ref app_id) = options.steam_app_id {
        child.env("SteamAppId", app_id).env("SteamGameId", app_id);
        let inherited_client_launch = env::var("SteamClientLaunch").ok();
        child.env(
            "SteamClientLaunch",
            inherited_client_launch.as_deref().unwrap_or("1"),
        );
        let inherited_steam_path = env::var("SteamPath").ok();
        child.env(
            "SteamPath",
            inherited_steam_path
                .as_deref()
                .unwrap_or("C:\\Program Files (x86)\\Steam"),
        );
    }

    match mode {
        LaunchMode::Offline(ref _offer_id) => {
            // Offline mode: use cached license, skip cloud sync
            // The license should already exist from a prior online session
            child.env("EALaunchOfflineMode", "true");
        }
        LaunchMode::Online(ref offer_id) => {
            // Best-effort: fetch an OPAQUE short-token for `EALaunchUserAuthToken`
            // (introduced by upstream PR #34 so the OOA license API works even
            // with a hardware-hash mismatch). Under Wine / CrossOver EA's auth
            // service routinely rejects this exchange with a redirect to
            // `signin.ea.com` (treated as `AuthError::InvalidRedirect`). When
            // that happens we fall back to the JWS access token — that's the
            // pre-PR-#34 upstream behavior and it still satisfies the env-var
            // contract the game expects. Without this fallback every launch
            // would fail end-to-end on bottles where the OOA exchange isn't
            // happy with our pc_sign / token, even though the rest of the
            // flow is fine.
            let short_token = match request_opaque_ooa_token(&access_token).await {
                Ok(token) => token,
                Err(err) => {
                    warn!(
                        "OPAQUE OOA token exchange failed ({}); falling back to \
                         JWS access_token for EALaunchUserAuthToken. The game's \
                         OOA-side calls may still work via EAAccessTokenJWS.",
                        err
                    );
                    access_token.clone()
                }
            };

            child
                .env("EAConnectionId", offer_id.clone())
                .env("EALicenseToken", offer_id.clone())
                .env("EALaunchUserAuthToken", short_token)
                .env("EAAccessTokenJWS", access_token);
        }
        LaunchMode::OnlineOffline(_, ref persona, ref password) => {
            child
                .env("EALaunchOOAUserEmail", persona)
                .env("EALaunchOOAUserPass", password)
                // Given this is probably running headlessly, don't show a UI on error
                .env("EAOnErrorExitRetCode", "1");
        }
    };

    let child = child.spawn().expect("Failed to start child");

    maxima.playing = Some(ActiveGameContext::new(
        &launch_id,
        dir,
        options.cloud_saves,
        &content_id,
        offer,
        mode,
        options.steam_app_id.clone(),
        child,
    ));

    Ok(())
}

async fn request_opaque_ooa_token(access_token: &str) -> Result<String, AuthError> {
    let mut context = AuthContext::new()?;
    context.set_access_token(&access_token);
    context.set_token_format("OPAQUE");
    context.set_expires_in(550);

    // These scopes match the token EA Desktop requests for this
    context.add_scope("basic.commerce.cartv2");
    context.add_scope("service.atom");
    context.add_scope("dp.client.default");
    context.add_scope("signin");
    context.add_scope("social_recommendation_user");
    context.add_scope("basic.optin.write");
    context.add_scope("basic.commerce.cartv2.write");
    context.add_scope("basic.billing");
    context.add_scope("external.social_information_ups_admin");

    nucleus_auth_exchange(&context, JUNO_PC_CLIENT_ID, "token").await
}

#[cfg(unix)]
pub async fn mx_linux_setup() -> Result<(), NativeError> {
    use crate::unix::wine::{
        check_runtime_validity, check_wine_validity, get_lutris_runtimes, install_runtime,
        install_wine, setup_wine_registry,
    };

    info!("Verifying wine dependencies...");

    let skip = std::env::var("MAXIMA_DISABLE_WINE_VERIFICATION").is_ok();
    if !skip {
        if !check_wine_validity().await? {
            install_wine().await?;
        }
        let runtimes = get_lutris_runtimes().await?;
        if !check_runtime_validity("eac_runtime", &runtimes).await? {
            install_runtime("eac_runtime", &runtimes).await?;
        }
        if !check_runtime_validity("umu", &runtimes).await? {
            install_runtime("umu", &runtimes).await?;
        }
    }

    setup_wine_registry().await?;

    Ok(())
}

pub fn parse_arguments(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut in_quotes = false;

    for c in input.chars() {
        match c {
            ' ' if !in_quotes => {
                if !current_arg.is_empty() {
                    args.push(current_arg.clone());
                    current_arg.clear();
                }
            }
            '"' => {
                in_quotes = !in_quotes;
            }
            _ => {
                current_arg.push(c);
            }
        }
    }

    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    args
}
