use base64::{engine::general_purpose, Engine};
use log::info;
use std::{path::PathBuf, sync::Arc, vec::Vec, env};
use tokio::{sync::Mutex, process::Command, io::AsyncBufReadExt};

use anyhow::Result;

use crate::{
    core::ecommerce::request_offer_data,
    ooa::{request_license, save_licenses},
    util::{
        registry::{get_bootstrap_path, read_game_path},
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

pub struct ActiveGameContext {
    pub offer: CommerceOffer,
    pub injections: Vec<LibraryInjection>,
}

impl ActiveGameContext {
    pub fn new(offer: CommerceOffer) -> Self {
        Self {
            offer,
            injections: Vec::new(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct BootstrapLaunchArgs {
    pub path: String,
    pub args: Vec<String>,
}

pub async fn start_game(
    offer_id: &str,
    game_path_override: Option<String>,
    mut game_args: Vec<String>,
    maxima_arc: Arc<Mutex<Maxima>>,
) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;
    info!("Retrieving data about '{}'...", offer_id);

    let offer =
        request_offer_data(&maxima.access_token, offer_id, maxima.locale.full_str()).await?;
    maxima.playing = Some(ActiveGameContext::new(offer.clone()));
    let content_id = offer.publishing.publishing_attributes.content_id.to_owned();

    info!(
        "Requesting pre-game license for {}...",
        offer.localizable_attributes.display_name
    );

    let license = request_license(
        content_id.as_str(),
        "ca5f9ae34d7bcd895e037a17769de60338e6e84", // Need to figure out how this is calculated
        maxima.access_token.as_str(),
        None,
        None,
    )
    .await
    .unwrap();
    //save_licenses(&license).unwrap();

    let software = offer.publishing.software_list.unwrap().software[0]
        .fulfillment_attributes
        .installation_directory
        .as_ref()
        .unwrap()
        .to_owned();

    let user = maxima.get_local_user().await?;

    // Need to move this into Maxima and have a "current game" system
    let path = if game_path_override.is_some() {
        PathBuf::from(game_path_override.as_ref().unwrap())
    } else {
        read_game_path(software)
            .expect("Failed to find game path")
            .join("starwarsbattlefrontii.exe")
    };
    let path = path.to_str().unwrap();
    info!("Game path: {}", path);

    // Append args from env
    if let Ok(args) = env::var("MAXIMA_LAUNCH_ARGS") {
        game_args.append(&mut parse_arguments(args.as_str()));
    }

    let mut child = Command::new(get_bootstrap_path()?);
    child.arg("launch");

    let bootstrap_args = BootstrapLaunchArgs {
        path: path.to_string(),
        args: game_args,
    };

    let b64 = general_purpose::STANDARD.encode(serde_json::to_string(&bootstrap_args).unwrap());
    child.arg(b64);

    child
        .current_dir(PathBuf::from(path).parent().unwrap())
        //.stdout(std::process::Stdio::piped())
        .env("EAAuthCode", "unavailable")
        .env("EAConnectionId", offer_id.to_owned())
        .env("EAEgsProxyIpcPort", "0")
        .env("EAExternalSource", "EA")
        .env("EAFreeTrialGame", "false")
        .env("EAGameLocale", maxima.locale.full_str())
        .env("EAGenericAuthToken", maxima.access_token.to_owned())
        .env("EALaunchCode", "4AULYZZ2KJSN2RMHEVUH")
        .env("EALaunchEAID", user.player.unwrap().display_name)
        .env("EALaunchEnv", "production")
        //.env("EALaunchOOAUserEmail", "")
        //.env("EALaunchOOAUserPass", "")
        .env("EAOnErrorExitRetCode", "true")
        .env("EALaunchOfflineMode", "false")
        .env("EALaunchUserAuthToken", maxima.access_token.to_owned())
        .env("EALicenseToken", offer_id.to_owned())
        .env("EALsxPort", maxima.lsx_port.to_string())
        .env(
            "EARtPLaunchCode",
            simple_crypto::get_rtp_handshake().to_string(),
        )
        .env("EASteamProxyIpcPort", "0")
        .env("OriginSessionKey", "5a81a155-7bf8-444c-a229-c22133447d88")
        .env("ContentId", content_id.to_owned());

    drop(maxima);

    // TODO store handle somewhere
    let mut child = child.spawn().expect("Failed to start child");

    //let stdout = child.stdout.take().expect("no stdout");
    // Can do the same for stderr

    child.wait().await?;

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