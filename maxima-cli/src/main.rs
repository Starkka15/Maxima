use clap::{Parser, Subcommand};

use anyhow::{bail, Result};
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
    util::{
        log::init_logger,
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
    /// Install a game non-interactively by slug
    Install {
        /// Game slug (from list-games output)
        slug: String,

        /// Absolute path to install the game to
        #[arg(long)]
        path: String,
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

    Ok(token_res?)
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
        } => {
            let offer_id = if login.is_none() {
                let mut maxima = maxima_arc.lock().await;
                let offer = maxima.mut_library().game_by_base_slug(&slug).await;
                // TODO: ideally this function should return an Error type, but this frontend makes that complicated
                if let Err(err) = offer {
                    bail!("Error fetching offer for slug `{}`: {}", slug, err);
                } else {
                    let offer = offer.unwrap();
                    // TODO: could do a match here as well, same problem as above
                    if offer.is_some() {
                        offer.unwrap().offer_id().to_owned()
                    } else {
                        bail!("No owned offer found for '{}'", slug);
                    }
                }
            } else {
                slug
            };

            start_game(&offer_id, game_path, game_args, login, maxima_arc.clone()).await
        }
        Mode::ListGames => list_games(maxima_arc.clone()).await,
        Mode::LocateGame { path } => locate_game(maxima_arc.clone(), &path).await,
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
        Mode::Install { slug, path } => {
            install_game(maxima_arc.clone(), &slug, &path).await
        }
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

    start_game(&offer_id, None, Vec::new(), None, maxima_arc.clone()).await?;

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

async fn install_game(maxima_arc: LockedMaxima, slug: &str, path: &str) -> Result<()> {
    let mut maxima = maxima_arc.lock().await;

    // Resolve slug to offer_id
    let offer = maxima.mut_library().game_by_base_slug(slug).await?;
    if offer.is_none() {
        bail!("No owned game found for slug '{}'", slug);
    }
    let offer = offer.unwrap();
    let offer_id = offer.offer_id().to_owned();
    let game_name = offer.offer().display_name().to_owned();

    info!("Installing {} ({})", game_name, offer_id);

    // Get available builds and pick the live one
    let builds = maxima
        .content_manager()
        .service()
        .available_builds(&offer_id)
        .await?;
    let build = builds.live_build();
    if build.is_none() {
        bail!("No suitable build found for '{}'", slug);
    }

    let build = build.unwrap();
    info!("Build: {}", build.to_string());

    let install_path = PathBuf::from(path);
    if !install_path.is_absolute() {
        bail!("Path '{}' is not absolute", path);
    }

    let game = QueuedGameBuilder::default()
        .offer_id(offer_id)
        .build_id(build.build_id().to_owned())
        .path(install_path)
        .build()?;

    let start_time = Instant::now();
    maxima.content_manager().install_now(game).await?;

    drop(maxima);

    // Progress polling loop
    loop {
        let mut maxima = maxima_arc.lock().await;

        for event in maxima.consume_pending_events() {
            match event {
                MaximaEvent::ReceivedLSXRequest(_pid, _request) => (),
                MaximaEvent::InstallFinished(ref _oid) => {
                    info!("Download Complete");
                }
                _ => {}
            }
        }

        maxima.update().await;

        if let Some(downloader) = maxima.content_manager().current() {
            let percent = downloader.percentage_done();
            let downloaded_bytes = downloader.bytes_downloaded();
            let total_bytes = downloader.bytes_total();
            let downloaded_mib = downloaded_bytes as f64 / (1024.0 * 1024.0);
            let total_mib = total_bytes as f64 / (1024.0 * 1024.0);

            info!(
                "Progress: {:.2} ",
                if percent >= 100.0 { 99.99 } else { percent }
            );
            info!("Downloaded: {:.2} MiB", downloaded_mib);
            info!("Total: {:.2} MiB", total_mib);
        } else {
            break;
        }

        drop(maxima);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    let elapsed = start_time.elapsed();
    info!(
        "Download Complete",
    );
    info!(
        "Install finished in {}.{}s",
        elapsed.as_secs(),
        elapsed.subsec_millis()
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
    let (offer_id, content_id, exe_path, game_display_name) = {
        let offer = maxima.mut_library().game_by_base_slug(slug).await?;
        if offer.is_none() {
            bail!("No owned game found for slug '{}'", slug);
        }
        let offer = offer.unwrap();
        let oid = offer.offer_id().to_owned();
        let cid = offer.offer().content_id().to_owned();
        let dname = offer.offer().display_name().to_owned();
        let epath = if game_path_override.is_none() {
            Some(offer.execute_path(false).await?.clone())
        } else {
            None
        };
        (oid, cid, epath, dname)
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
        info!("Requesting game license for {}...", game_display_name);
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
    let titles = maxima.mut_library().games().await?;

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
