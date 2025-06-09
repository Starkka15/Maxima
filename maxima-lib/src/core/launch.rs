use base64::{engine::general_purpose, Engine};
use derive_getters::Getters;
use log::{error, info};
use std::{env, fmt::Display, path::PathBuf, sync::Arc};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};
use uuid::Uuid;

use crate::{
    core::{
        auth::storage::TokenError, cloudsync::CloudSyncError, cloudsync::CloudSyncLockMode,
        library::LibraryError, library::OwnedOffer, service_layer::ServiceLayerError, Maxima,
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
    CloudSync(#[from] CloudSyncError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
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
    process: Child,
    started: bool,
}

impl ActiveGameContext {
    pub fn new(
        launch_id: &str,
        game_path: &str,
        content_id: &str,
        offer: Option<OwnedOffer>,
        mode: LaunchMode,
        process: Child,
    ) -> Self {
        Self {
            launch_id: launch_id.to_owned(),
            game_path: game_path.to_owned(),
            content_id: content_id.to_owned(),
            offer,
            mode,
            injections: Vec::new(),
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
    game_path_override: Option<String>,
    mut game_args: Vec<String>,
) -> Result<(), LaunchError> {
    let mut maxima = maxima_arc.lock().await;
    info!("Initiating game launch with {}...", mode);

    if let LaunchMode::OnlineOffline(ref content_id, _, _) = mode {
        if game_path_override.is_none() {
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

            if !offer.is_installed().await {
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
        } else {
            return Err(LaunchError::Offline);
        };

    // Need to move this into Maxima and have a "current game" system
    let path = if let Some(game_path_override) = game_path_override {
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
            if needs_license_update(&content_id).await? {
                info!(
                    "Requesting new game license for {}...",
                    offer.offer().display_name()
                );

                request_and_save_license(&auth, &content_id, path.to_owned().into()).await?;
            } else {
                info!("Existing game license is still valid, not updating");
            }

            if offer.offer().has_cloud_save() {
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
            request_and_save_license(&auth, &content_id, path.to_owned().into()).await?;
        }
    }

    // Append args from env
    if let Ok(args) = env::var("MAXIMA_LAUNCH_ARGS") {
        game_args.append(&mut parse_arguments(args.as_str()));
    }

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

    child
        .current_dir(PathBuf::from(path).safe_parent()?)
        .env("MXLaunchId", launch_id.to_owned())
        .env("EAAuthCode", "unavailable")
        .env("EAEgsProxyIpcPort", "0")
        .env("EAEntitlementSource", "EA")
        .env("EAExternalSource", "EA")
        .env("EAFreeTrialGame", "false")
        .env("EAGameLocale", maxima.locale.full_str())
        .env("EAGenericAuthToken", access_token.to_owned())
        .env("EALaunchCode", "")
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
        .env("OriginSessionKey", launch_id.to_owned())
        .env("ContentId", content_id.to_owned())
        .env("EAOnErrorExitRetCode", "1");

    match mode {
        LaunchMode::Offline(_) => todo!(),
        LaunchMode::Online(ref offer_id) => {
            child
                .env("EAConnectionId", offer_id.to_owned())
                .env("EALicenseToken", offer_id.to_owned())
                .env("EALaunchUserAuthToken", access_token);
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
        &content_id,
        offer,
        mode,
        child,
    ));

    Ok(())
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
