use clap::{Parser, ValueEnum};

use anyhow::{bail, Result};
use inquire::Select;
use log::{debug, error, info, warn};
use tokio::{sync::Mutex, time::sleep};

use std::{sync::Arc, vec::Vec, time::Duration};

use is_elevated::is_elevated;

use maxima::{
    core::{
        self,
        ecommerce::request_offer_data,
        launch,
        service_layer::{
            send_service_request, ServiceGetUserPlayerRequest, ServiceUser, ServiceUserGameProduct,
            SERVICE_REQUEST_GETUSERPLAYER,
        },
        Maxima, MaximaEvent, background_service::request_registry_setup, auth::get_auth_code,
    },
    util::{
        log::init_logger,
        native::take_foreground_focus,
        registry::check_registry_validity, service::{is_service_running, start_service, is_service_valid, register_service_user},
    },
};

#[derive(ValueEnum, Debug, Clone, PartialEq)]
enum Mode {
    Launch,
    ListGames,
    AccountInfo,
    CreateAuthCode,
    Interactive,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(value_enum, long, default_value_t = Mode::Interactive)]
    mode: Mode,

    #[arg(long)]
    access_token: Option<String>,

    #[arg(long)]
    client_id: Option<String>,

    #[arg(short, long)]
    offer_id: Option<String>,

    #[arg(long)]
    game_path: Option<String>,

    #[arg(long)]
    game_args: Vec<String>,
}

#[tokio::main]
async fn main() {
    let result = startup().await;
    if result.is_err() {
        error!("{}", result.err().unwrap());
    }
}

async fn startup() -> Result<()> {
    let args = Args::parse();

    init_logger()?;

    info!("Starting Maxima...");
    
    if !is_elevated() {
        if !is_service_valid()? {
            info!("Installing service...");
            register_service_user()?;
            sleep(Duration::from_secs(1)).await;
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

    debug!("Logging in...");
    let token = if let Some(access_token) = args.access_token {
        Some(access_token)
    } else {
        core::auth::login::execute().await.unwrap()
    };
    
    if token.is_none() {
        error!("Login failed!");
        return Ok(());
    }

    let user: ServiceUser = send_service_request(
        token.as_ref().unwrap(),
        SERVICE_REQUEST_GETUSERPLAYER,
        ServiceGetUserPlayerRequest {},
    )
    .await?;

    info!("Logged in as {}!", user.player.unwrap().display_name);

    // Take back the focus since the browser and bootstrap will take it
    take_foreground_focus()?;

    let maxima_arc = Arc::new(Mutex::new(Maxima::new()));
    {
        let mut maxima = maxima_arc.lock().await;
        maxima.access_token = token.unwrap().to_string();
    }

    match args.mode {
        Mode::Launch => start_game(&args.offer_id.as_ref().expect("Please pass an Origin Offer ID with `--offer-id`. You can obtain one through the `list-games` mode"), args.game_path, args.game_args, maxima_arc.clone()).await,
        Mode::ListGames => list_games(maxima_arc.clone()).await,
        Mode::AccountInfo => print_auth_token(maxima_arc.clone()).await,
        Mode::CreateAuthCode => create_auth_code(maxima_arc.clone(), &args.client_id.unwrap()).await,
        Mode::Interactive => run_interactive(maxima_arc.clone()).await,
    }?;

    Ok(())
}

async fn run_interactive(maxima_arc: Arc<Mutex<Maxima>>) -> Result<()> {
    let launch_options = vec!["Launch Game", "List Games"];
    let name = Select::new(
        "Welcome to Maxima! What would you like to do?",
        launch_options,
    )
    .prompt()?;

    match name {
        "Launch Game" => interactive_start_game(maxima_arc.clone()).await?,
        "List Games" => list_games(maxima_arc.clone()).await?,
        _ => bail!("Something went wrong."),
    }

    Ok(())
}

async fn interactive_start_game(maxima_arc: Arc<Mutex<Maxima>>) -> Result<()> {
    let maxima = maxima_arc.lock().await;
    let owned_games = maxima
        .get_owned_games(1)
        .await?
        .owned_game_products
        .unwrap()
        .items;

    let owned_games_strs = owned_games
        .iter()
        .map(|g| g.product.name.replace("\n", ""))
        .collect::<Vec<String>>();

    let name = Select::new("What game would you like to play?", owned_games_strs).prompt()?;
    let game: &ServiceUserGameProduct = owned_games
        .iter()
        .find(|g| g.product.name.replace("\n", "") == name)
        .unwrap();

    drop(maxima);
    start_game(
        game.origin_offer_id.as_str(),
        None,
        Vec::new(),
        maxima_arc.clone(),
    )
    .await?;

    Ok(())
}

async fn print_auth_token(maxima_arc: Arc<Mutex<Maxima>>) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    let user: ServiceUser = send_service_request(
        &maxima.access_token,
        SERVICE_REQUEST_GETUSERPLAYER,
        ServiceGetUserPlayerRequest {},
    )
    .await?;

    info!("Access Token: {}", &maxima.access_token);

    let player = user.player.unwrap();
    info!("Username: {}", player.unique_name);
    info!("User ID: {}", user.id);
    info!("Persona ID: {}", player.psd);
    Ok(())
}

async fn create_auth_code(maxima_arc: Arc<Mutex<Maxima>>, client_id: &str) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    let auth_code = get_auth_code(&maxima.access_token, client_id).await?;
    info!("Auth Code for {}: {}", client_id, auth_code);
    Ok(())
}

async fn list_games(maxima_arc: Arc<Mutex<Maxima>>) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    info!("Owned games:");
    let owned_games = maxima.get_owned_games(1).await?;
    for game in owned_games.owned_game_products.unwrap().items {
        let offer = request_offer_data(
            &maxima.access_token,
            &game.origin_offer_id,
            maxima.locale.full_str(),
        )
        .await?;

        info!(
            "{:<width$} - {:<width2$} - {}",
            game.product.name.replace("\n", ""),
            game.origin_offer_id,
            offer.publishing.publishing_attributes.content_id,
            width = 55,
            width2 = 25
        );
    }

    Ok(())
}

async fn start_game(
    offer_id: &str,
    game_path_override: Option<String>,
    mut game_args: Vec<String>,
    maxima_arc: Arc<Mutex<Maxima>>,
) -> Result<()> {
    {
        let maxima = maxima_arc.lock().await;
        maxima.start_lsx(maxima_arc.clone()).await?;
    }

    //game_args.push("-dataPath".to_string());
    //game_args.push("/mnt/battlefront/ModData/CollectionUnlocker".to_string());

    launch::start_game(offer_id, game_path_override, game_args, maxima_arc.clone()).await?;

    // loop {
    //     let mut maxima = maxima_arc.lock().await;

    //     for event in maxima.consume_pending_events() {
    //         match event {
    //             MaximaEvent::ReceivedLSXRequest(_request) => (),
    //         }
    //     }

    //     drop(maxima);
    //     std::thread::sleep(std::time::Duration::from_secs(1));
    // }

    Ok(())
}
