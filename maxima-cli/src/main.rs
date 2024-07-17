use clap::{Parser, Subcommand};

use anyhow::{bail, Result};
use futures::StreamExt;
use inquire::{Select, Text};
use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use regex::Regex;

use std::{path::PathBuf, sync::Arc, time::Instant};

#[cfg(windows)]
use is_elevated::is_elevated;

#[cfg(windows)]
use maxima::{
    core::background_service::request_registry_setup,
    util::service::{is_service_running, is_service_valid, register_service_user, start_service},
};

use maxima::{
    content::{
        downloader::ZipDownloader,
        manager::{QueuedGame, QueuedGameBuilder},
        ContentService,
    },
    core::{
        auth::{
            context::AuthContext,
            login::{begin_oauth_login_flow, manual_login},
            nucleus_auth_exchange, nucleus_token_exchange, TokenResponse,
        },
        clients::JUNO_PC_CLIENT_ID,
        cloudsync::CloudSyncLockMode,
        dip::{DiPManifest, DIP_RELATIVE_PATH},
        launch::{self, LaunchMode},
        library::OwnedTitle,
        service_layer::{
            ServiceGetBasicPlayerRequestBuilder, ServiceGetLegacyCatalogDefsRequestBuilder,
            ServiceLegacyOffer, ServicePlayer, SERVICE_REQUEST_GETBASICPLAYER,
            SERVICE_REQUEST_GETLEGACYCATALOGDEFS,
        },
        LockedMaxima, Maxima, MaximaEvent, MaximaOptionsBuilder,
    },
    ooa,
    rtm::client::BasicPresence,
    util::{log::init_logger, native::take_foreground_focus, registry::check_registry_validity},
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

        #[arg(long)]
        game_args: Vec<String>,

        /// When set, offer_id must be a content ID, and the only authenticated
        /// requests are made to the license server. A dummy name will be used
        /// in place of your real username, and any online LSX requests will fail
        #[arg(long)]
        login: Option<String>,
    },
    ListGames,
    LocateGame {
        path: String,
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

    token_res
}

async fn startup() -> Result<()> {
    let args = Args::parse();

    init_logger();

    info!("Starting Maxima...");

    native_setup().await?;

    let skip_login = {
        if let Some(Mode::Launch {
            game_path: _,
            game_args: _,
            slug: _,
            ref login,
        }) = args.mode
        {
            login.is_some()
        } else {
            false
        }
    };

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
        } => start_game(&slug, game_path, game_args, login, maxima_arc.clone()).await,
        Mode::ListGames => list_games(maxima_arc.clone()).await,
        Mode::LocateGame { path } => locate_game(maxima_arc.clone(), &path).await,
        Mode::CloudSync { game_slug, write } => {
            do_cloud_sync(maxima_arc.clone(), &game_slug, write).await
        }
        Mode::AccountInfo => print_account_info(maxima_arc.clone()).await,
        Mode::CreateAuthCode { client_id } => {
            create_auth_code(maxima_arc.clone(), &client_id).await
        }
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
        "List Games" => list_games(maxima_arc.clone()).await?,
        "Account Info" => print_account_info(maxima_arc.clone()).await?,
        _ => bail!("Something went wrong."),
    }

    Ok(())
}

async fn interactive_start_game(maxima_arc: LockedMaxima) -> Result<()> {
    let offer_id = {
        let mut maxima = maxima_arc.lock().await;

        let mut owned_games = Vec::new();
        for game in maxima.mut_library().games().await {
            if !game.base_offer().installed().await {
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

    start_game(&offer_id, None, Vec::new(), None, maxima_arc.clone()).await?;

    Ok(())
}

async fn interactive_install_game(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    let offer_id = {
        let mut owned_games = Vec::new();
        for game in maxima.mut_library().games().await {
            if game.base_offer().installed().await {
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

    let downloader = ZipDownloader::new("test-game", &url.url(), "DownloadTest").await?;
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

    let owned_games = maxima.mut_library().games().await;
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
    info!("PC Sign: {}", AuthContext::new()?.generate_pc_sign());

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
    let maxima = maxima_arc.lock().await;

    // match maxima.owned_game_by_slug(slug).await {
    //     Ok(game) => info!("Game: {}", game.id()),
    //     Err(err) => error!("{}", err),
    // };

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

async fn list_games(maxima_arc: LockedMaxima) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    info!("Owned games:");
    let titles = maxima.mut_library().games().await;

    for title in titles {
        info!(
            "{:<width$} - {:<width2$} - {:<width3$} - Installed: {}",
            title.base_offer().slug(),
            title.name(),
            title.base_offer().offer_id(),
            title.base_offer().installed().await,
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

async fn locate_game(maxima_arc: LockedMaxima, path: &str) -> Result<()> {
    let path = PathBuf::from(path);
    let manifest = DiPManifest::read(&path.join(DIP_RELATIVE_PATH)).await?;
    manifest.run_touchup(&path).await?;
    info!("Installed!");
    Ok(())
}

async fn do_cloud_sync(maxima_arc: LockedMaxima, game_slug: &str, write: bool) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;
    let offer = maxima
        .mut_library()
        .game_by_base_slug(game_slug)
        .await
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
    res.unwrap();

    info!("Done");

    Ok(())
}

async fn start_game(
    offer_id: &str,
    game_path_override: Option<String>,
    game_args: Vec<String>,
    login: Option<String>,
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

    if login.is_none() {
        launch::start_game(
            maxima_arc.clone(),
            LaunchMode::Online(offer_id.to_owned()),
            game_path_override,
            game_args,
        )
        .await?;
    } else if let Some(captures) = MANUAL_LOGIN_PATTERN.captures(&login.unwrap()) {
        let persona = &captures[1];
        let password = &captures[2];

        launch::start_game(
            maxima_arc.clone(),
            LaunchMode::OnlineOffline(offer_id.to_owned(), persona.to_owned(), password.to_owned()),
            game_path_override,
            game_args,
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
