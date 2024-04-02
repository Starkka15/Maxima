use base64::{engine::general_purpose, Engine};
use derive_getters::Getters;
use log::info;
use std::{env, fmt::Display, path::PathBuf, sync::Arc, vec::Vec};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};
use uuid::Uuid;

use anyhow::{bail, Result};

use crate::{
    core::ecommerce::request_offer_data,
    ooa::{request_and_save_license, LicenseAuth},
    util::{
        registry::{bootstrap_path, read_game_path},
        simple_crypto,
    },
};

use serde::{Deserialize, Serialize};

use super::{ecommerce::CommerceOffer, Maxima};

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
    offer: Option<CommerceOffer>,
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
        offer: Option<CommerceOffer>,
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
) -> Result<()> {
    linux_setup().await?;

    let mut maxima = maxima_arc.lock().await;
    info!("Initiating game launch with {}...", mode);

    if let LaunchMode::OnlineOffline(ref content_id, _, _) = mode {
        if game_path_override.is_none() {
            bail!("Game path must be specified when launching in OnlineOffline mode");
        }

        if content_id.starts_with("Origin.OFR") {
            bail!("Content ID was specified as an offer ID when launching in OnlineOffline mode");
        }
    }

    let (content_id, online_offline, offer, access_token) =
        if let LaunchMode::Online(ref offer_id) = mode {
            let access_token = &maxima.access_token().await?;
            let offer =
                request_offer_data(access_token, offer_id, maxima.locale.full_str()).await?;

            let content_id = offer
                .publishing
                .publishing_attributes
                .content_id
                .as_ref()
                .unwrap()
                .to_owned();

            info!(
                "Requesting pre-game license for {}...",
                offer.localizable_attributes.display_name
            );

            (content_id, false, Some(offer), access_token.to_owned())
        } else if let LaunchMode::OnlineOffline(ref content_id, _, _) = mode {
            (content_id.to_owned(), true, None, String::new())
        } else {
            bail!("Offline mode is not yet supported");
        };

    // Need to move this into Maxima and have a "current game" system
    let path = if game_path_override.is_some() {
        PathBuf::from(game_path_override.as_ref().unwrap())
    } else if online_offline {
        // https://youtu.be/TGfQu0bQTKc?t=506
        let software = offer
            .as_ref()
            .unwrap()
            .publishing
            .software_list
            .as_ref()
            .unwrap()
            .software[0]
            .fulfillment_attributes
            .installation_directory
            .as_ref()
            .unwrap()
            .to_owned();

        read_game_path(&software)
            .expect("Failed to find game path")
            .join("starwarsbattlefrontii.exe")
    } else {
        bail!("Game path not found");
    };

    let dir = path.parent().unwrap().to_str().unwrap();
    let path = path.to_str().unwrap();
    info!("Game path: {}", path);

    match mode {
        LaunchMode::Offline(_) => {}
        LaunchMode::Online(_) => {
            let auth = LicenseAuth::AccessToken(maxima.access_token().await?);
            request_and_save_license(&auth, &content_id, path.into()).await?;
        }
        LaunchMode::OnlineOffline(_, ref persona, ref password) => {
            let auth = LicenseAuth::Direct(persona.to_owned(), password.to_owned());
            request_and_save_license(&auth, &content_id, path.into()).await?;
        }
    }

    // Append args from env
    if let Ok(args) = env::var("MAXIMA_LAUNCH_ARGS") {
        game_args.append(&mut parse_arguments(args.as_str()));
    }

    let mut child = Command::new(bootstrap_path());
    child.arg("launch");

    let bootstrap_args = BootstrapLaunchArgs {
        path: path.to_string(),
        args: game_args,
    };

    let b64 = general_purpose::STANDARD.encode(serde_json::to_string(&bootstrap_args).unwrap());
    child.arg(b64);

    let user = maxima.local_user().await?;
    let launch_id = Uuid::new_v4().to_string();

    child
        .current_dir(PathBuf::from(path).parent().unwrap())
        .env("MXLaunchId", launch_id.to_owned())
        .env("EAAuthCode", "unavailable")
        .env("EAEgsProxyIpcPort", "0")
        .env("EAEntitlementSource", "EA")
        .env("EAExternalSource", "EA")
        .env("EAFreeTrialGame", "false")
        .env("EAGameLocale", maxima.locale.full_str())
        .env("EAGenericAuthToken", access_token.to_owned())
        .env("EALaunchCode", "4AULYZZ2KJSN2RMHEVUH")
        .env(
            "EALaunchEAID",
            user.player().as_ref().unwrap().display_name(),
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
        .env("ContentId", content_id.to_owned());

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
pub async fn linux_setup() -> Result<()> {
    use crate::unix::wine::{
        check_dxvk_validity, check_vkd3d_validity, check_wine_validity, install_wine,
        setup_wine_registry, wine_install_dxvk, wine_install_vkd3d,
    };

    info!("Verifying wine dependencies...");

    let skip = std::env::var("MAXIMA_DISABLE_WINE_VERIFICATION").is_ok();

    if !skip && !check_wine_validity()? {
        install_wine().await?;
    }

    setup_wine_registry()?;

    if !skip && !check_dxvk_validity()? {
        wine_install_dxvk().await?;
    }

    if !skip && !check_vkd3d_validity()? {
        wine_install_vkd3d().await?;
    }

    Ok(())
}

#[cfg(not(unix))]
pub async fn linux_setup() -> Result<()> {
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
