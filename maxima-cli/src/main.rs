use clap::{Parser, Subcommand};

use anyhow::{bail, Result};
use inquire::Select;
use lazy_static::lazy_static;
use log::{error, info, warn};
use regex::Regex;

use std::vec::Vec;

#[cfg(windows)]
use is_elevated::is_elevated;

#[cfg(windows)]
use maxima::{
    core::background_service::request_registry_setup,
    util::service::{is_service_running, is_service_valid, register_service_user, start_service},
};

use maxima::core::{
    auth::{execute_connect_token, TokenResponse},
    LockedMaxima,
};
use maxima::{
    content::{zip::ZipFile, ContentService},
    core::{
        auth::{
            context::AuthContext,
            execute_auth_exchange,
            login::{begin_oauth_login_flow, manual_login},
        },
        launch,
        service_layer::ServiceUserGameProduct,
        Maxima, MaximaEvent,
    },
    util::{log::init_logger, native::take_foreground_focus, registry::check_registry_validity},
};

lazy_static! {
    static ref MANUAL_LOGIN_PATTERN: Regex = Regex::new(r"^(.*):(.*)$").unwrap();
}

#[derive(Subcommand, Debug)]
enum Mode {
    Launch {
        #[arg(long)]
        game_path: Option<String>,

        #[arg(long)]
        game_args: Vec<String>,

        #[arg(short, long)]
        offer_id: Option<String>,
    },
    ListGames,
    AccountInfo,
    CreateAuthCode {
        #[arg(long)]
        client_id: Option<String>,
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

#[tokio::main]
async fn main() {
    let result = startup().await;

    if let Some(e) = result.err() {
        match std::env::var("RUST_BACKTRACE") {
            Ok(_) => error!("{}:\n{}", e, e.backtrace().to_string()),
            Err(_) => error!("{}", e),
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
        let code = execute_auth_exchange(&auth_context, "JUNO_PC_CLIENT", "code").await?;
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

    let token_res = execute_connect_token(&auth_context).await;
    if token_res.is_err() {
        bail!("Login failed: {}", token_res.err().unwrap().to_string());
    }

    token_res
}

async fn startup() -> Result<()> {
    let args = Args::parse();

    init_logger();

    info!("Starting Maxima...");

    native_setup().await?;

    // Take back the focus since the browser and bootstrap will take it
    take_foreground_focus()?;

    let maxima_arc = Maxima::new()?;

    {
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

    if args.mode.is_none() {
        run_interactive(maxima_arc.clone()).await?;
        return Ok(());
    }

    let mode = args.mode.unwrap();
    match mode {
        Mode::Launch{game_path, game_args, offer_id} => start_game(&offer_id.as_ref().expect("Please pass an Origin Offer ID with `--offer-id`. You can obtain one through the `list-games` mode"), game_path, game_args, maxima_arc.clone()).await,
        Mode::ListGames => list_games(maxima_arc.clone()).await,
        Mode::AccountInfo => print_account_info(maxima_arc.clone()).await,
        Mode::CreateAuthCode{client_id} => create_auth_code(maxima_arc.clone(), &client_id.unwrap()).await,
    }?;

    Ok(())
}

async fn run_interactive(maxima_arc: LockedMaxima) -> Result<()> {
    let launch_options = vec!["Launch Game", "Install Game", "List Games", "Account Info"];
    let name = Select::new(
        "Welcome to Maxima! What would you like to do?",
        launch_options,
    )
    .prompt()?;

    match name {
        "Launch Game" => interactive_start_game(maxima_arc.clone()).await?,
        "Install Game" => interactive_install_game(maxima_arc.clone()).await?,
        "List Games" => list_games(maxima_arc.clone()).await?,
        "Account Info" => print_account_info(maxima_arc.clone()).await?,
        _ => bail!("Something went wrong."),
    }

    Ok(())
}

async fn interactive_start_game(maxima_arc: LockedMaxima) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    let owned_games = maxima.owned_games(1).await?;
    let owned_games = owned_games.owned_game_products().as_ref().unwrap().items();
    let owned_games_strs = owned_games
        .iter()
        .map(|g| g.product().name())
        .collect::<Vec<String>>();

    let name = Select::new("What game would you like to play?", owned_games_strs).prompt()?;
    let game: &ServiceUserGameProduct = owned_games
        .iter()
        .find(|g| g.product().name() == name)
        .unwrap();

    drop(maxima);
    start_game(
        game.origin_offer_id().as_str(),
        None,
        Vec::new(),
        maxima_arc.clone(),
    )
    .await?;

    Ok(())
}

async fn interactive_install_game(maxima_arc: LockedMaxima) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    let owned_games = maxima.owned_games(1).await?;
    let owned_games = owned_games.owned_game_products().as_ref().unwrap().items();
    let owned_games_strs = owned_games
        .iter()
        .map(|g| g.product().name())
        .collect::<Vec<String>>();

    let name = Select::new("What game would you like to install?", owned_games_strs).prompt()?;
    let game: &ServiceUserGameProduct = owned_games
        .iter()
        .find(|g| g.product().name() == name)
        .unwrap();

    let content_service = ContentService::new(maxima.auth_storage().clone());
    let builds = content_service
        .available_builds(&game.origin_offer_id())
        .await?;
    let build = builds.live_build();
    if build.is_none() {
        bail!("Couldn't find a suitable game build");
    }

    let build = build.unwrap();
    info!("Installing game build {}", build.to_string());

    let url = content_service
        .download_url(&game.origin_offer_id(), Some(&build.build_id()))
        .await?;

    info!("URL: {}", url.url());

    let manifest = ZipFile::fetch(&url.url()).await?;
    info!("Entries: {}", manifest.entries().len());

    for ele in manifest.entries() {
        info!("File: {}", ele.name());
    }

    Ok(())
}

async fn print_account_info(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;
    let user = maxima.local_user().await?;

    info!("Access Token: {}", maxima.access_token().await?);

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

    let auth_code = execute_auth_exchange(&context, client_id, "code").await?;
    info!("Auth Code for {}: {}", client_id, auth_code);
    Ok(())
}

async fn list_games(maxima_arc: LockedMaxima) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    info!("Owned games:");
    let owned_games = maxima.owned_games(1).await?;
    for game in owned_games.owned_game_products().as_ref().unwrap().items() {
        info!(
            "{:<width$} - {:<width2$}",
            game.product().name(),
            game.origin_offer_id(),
            width = 55,
            width2 = 25
        );
    }

    Ok(())
}

async fn start_game(
    offer_id: &str,
    game_path_override: Option<String>,
    game_args: Vec<String>,
    maxima_arc: LockedMaxima,
) -> Result<()> {
    {
        let maxima = maxima_arc.lock().await;
        maxima.start_lsx(maxima_arc.clone()).await?;
    }

    launch::start_game(offer_id, game_path_override, game_args, maxima_arc.clone()).await?;

    loop {
        let mut maxima = maxima_arc.lock().await;

        for event in maxima.consume_pending_events() {
            match event {
                MaximaEvent::ReceivedLSXRequest(_pid, _request) => (),
                MaximaEvent::Unknown => todo!(),
            }
        }

        maxima.update_playing_status();
        if maxima.playing().is_none() {
            break;
        }

        drop(maxima);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(())
}
