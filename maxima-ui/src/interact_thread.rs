use anyhow::{Result, Ok};
use egui::{vec2, Context};
use log::{info, error, debug};
use tokio::sync::Mutex;

use std::{
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
    vec::Vec, fs,
};

use maxima::{
    core::{
        auth::{
            login, context::AuthContext, execute_auth_exchange
        },
        launch,
        Maxima,
        service_layer::{
            ServiceGame, SERVICE_REQUEST_GAMEIMAGES, ServiceGameImagesRequestBuilder
        },
        clients::JUNO_PC_CLIENT_ID
    },
    util::native::{
        maxima_dir,
        take_foreground_focus
    }
};
use maxima::core::auth::nucleus_connect_token;

use crate::{GameInfo, GameImage};

pub struct InteractThreadLoginResponse {
    pub success: bool,
    pub description: String,
}

pub struct InteractThreadGameListResponse {
    pub game: GameInfo,
    pub idx: usize,   // what game out of the total is this
    pub total: usize, // total games
}

pub enum MaximaLibRequest {
    LoginRequestOauth,
    LoginRequestUserPass(String, String),
    GetGamesRequest,
    StartGameRequest(String),
    BitchesRequest,
    ShutdownRequest,
}

pub enum MaximaLibResponse {
    LoginResponse(InteractThreadLoginResponse),
    GameInfoResponse(InteractThreadGameListResponse),
}

pub struct MaximaThread {
    pub rx: Receiver<MaximaLibResponse>,
    pub tx: Sender<MaximaLibRequest>,
}

impl MaximaThread {
    pub fn new(ctx: &Context) -> Self {
        let (tx0, rx1) = std::sync::mpsc::channel();
        let (tx1, rx0) = std::sync::mpsc::channel();
        let context = ctx.clone();
        tokio::task::spawn(async move {
            let result = MaximaThread::run(rx1, tx1, &context).await;
            if result.is_err() {
                panic!("Interact thread failed! {}", result.err().unwrap());
            } else {
                info!("Interact thread shut down")
            }
        });

        Self { rx: rx0, tx: tx0,}
    }

    async fn run(rx1: Receiver<MaximaLibRequest>, tx1: Sender<MaximaLibResponse>, ctx: &Context) -> Result<()> {
        let maxima_arc: Arc<Mutex<Maxima>> = Maxima::new()?;

        {
            let maxima = maxima_arc.lock().await;
            if maxima.start_lsx(maxima_arc.clone()).await.is_ok() {
                info!("LSX started");
            } else {
                info!("LSX failed to start!");
            }

            let mut auth_storage = maxima.auth_storage().lock().await;
            let logged_in = auth_storage.logged_in().await?;
            if logged_in {
                drop(auth_storage);

                let user = maxima.local_user().await?;
                let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                    success: true,
                    description: user.player().as_ref().unwrap().display_name().to_owned(),
                });

                tx1.send(lmessage)?;
            }
        }

        'outer: loop {
            let request = rx1.recv()?;
            match request {
                MaximaLibRequest::LoginRequestOauth => {
                    let maxima = maxima_arc.lock().await;

                    {
                        let mut auth_storage = maxima.auth_storage().lock().await;
                        let mut context = AuthContext::new()?;
                        login::begin_oauth_login_flow(&mut context).await?;
                        let token_res = nucleus_connect_token(&context).await?;
                        auth_storage.add_account(&token_res).await?;
                    }

                    let user = maxima.local_user().await?;
                    let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                        success: true,
                        description: user.player().as_ref().unwrap().display_name().to_owned(),
                    });

                    tx1.send(lmessage)?;
                    
                    take_foreground_focus().unwrap();
                    egui::Context::request_repaint(&ctx);
                },
                MaximaLibRequest::LoginRequestUserPass(user, pass) => {
                    let maxima = maxima_arc.lock().await;

                    {
                        let login_result = maxima::core::auth::login::manual_login(&user, &pass).await;
                        if login_result.is_err() {
                            let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                                success: false,
                                description: {
                                    if let Some(e) = login_result.err() {
                                        e.to_string()
                                    } else {
                                        "Failed for an unknown reason".to_string()
                                    }
                                }
                            });
        
                            tx1.send(lmessage)?;
                            continue;
                        }

                        let mut auth_context = AuthContext::new()?;
                        auth_context.set_access_token(&login_result.unwrap());
                        let code = execute_auth_exchange(&auth_context, JUNO_PC_CLIENT_ID, "code").await?;
                        auth_context.set_code(&code);

                        if auth_context.code().is_none() {
                            let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                                success: false,
                                description: "Failed for an unknown reason".to_string(),
                            });
                            tx1.send(lmessage)?;
                            continue;
                        }

                        let token_res = nucleus_connect_token(&auth_context).await;

                        if token_res.is_err() {
                            let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                                success: false,
                                description: token_res.err().unwrap().to_string(),
                            });
                            tx1.send(lmessage)?;
                            continue;
                        }

                        {
                            let mut auth_storage = maxima.auth_storage().lock().await;
                            auth_storage.add_account(&token_res.unwrap()).await?;
                        }

                        let user = maxima.local_user().await?;
                        let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                            success: true,
                            description: user.player().as_ref().unwrap().display_name().to_owned(),
                        });
                        info!("Successfully logged in with username/password");
                        tx1.send(lmessage)?;
                    }
                    egui::Context::request_repaint(&ctx);
                },
                MaximaLibRequest::GetGamesRequest => {
                    debug!("recieved request to load games");
                    let maxima = maxima_arc.lock().await;
                    let logged_in = maxima.auth_storage().lock().await.current().is_some();
                    if !logged_in {
                        info!("Ignoring request to load games, not logged in.");
                        continue;
                    }

                    let owned_games = maxima.owned_games(1).await.unwrap();
                    debug!("{:?}", owned_games);
                    if let Some(games_list) = owned_games.owned_game_products() {
                        for game in games_list.items() {
                            let slug = game.product().game_slug();
                            let game_hero = maxima_dir().unwrap().join("cache/ui/images/").join(&slug).join("hero.jpg");
                            let game_logo = maxima_dir().unwrap().join("cache/ui/images/").join(&slug).join("logo.png");
                            // includes EA play titles, but also lesser editions of owned games
                            /* !game.product.game_product_user.ownership_methods.contains(&ServiceOwnershipMethod::XgpVault) */
                            if true {
                                let has_hero = fs::metadata(&game_hero).is_ok();
                                let has_logo = fs::metadata(&game_logo).is_ok();
                                let images: Option<ServiceGame> = // TODO: make it a result
                                    if 
                                        !has_hero
                                    || !has_logo
                                    { //game hasn't been cached yet
                                        // TODO: image downloading
                                        maxima.service_layer().request(SERVICE_REQUEST_GAMEIMAGES, ServiceGameImagesRequestBuilder::default().should_fetch_context_image(!has_logo).should_fetch_backdrop_images(!has_hero).game_slug(game.product().game_slug().clone()).locale(maxima.locale().short_str().to_owned()).build()?).await?
                                    } else { None };

                            // TODO:: there's probably a cleaner way to do this
                            debug!("jank ass shit incoming frfrfrfrfrfrfrfr");
                            
                            let logo_url_option: Option<String> =
                            if let Some(img) = &images {
                                if let Some(logos) = &img.primary_logo() {
                                    if let Some(largest_logo) = &logos.largest_image() {
                                        Some(largest_logo.path().clone())
                                    } else {
                                        error!("Failed to get largest ServiceImage logo for {}", &slug);
                                        None
                                    }
                                } else {
                                    error!("Failed to get ServiceImageRendition logos for {}", &slug);
                                    None
                                }
                            } else {
                                // There used to be an error here, however the only way to get here is if the game's assets are already cached
                                None
                            };

                            let game_logo: Option<Arc<GameImage>> = 
                            if let Some(logo_url) = logo_url_option {
                                debug!("sending GameImage struct for {}", &slug);
                                Some(GameImage {
                                    retained: None,
                                    renderable: None,
                                    _fs_path: game_logo.to_str().unwrap().to_string().clone(),
                                    url: logo_url,
                                    size: vec2(0.0, 0.0)
                                }.into())
                            } else if has_logo {
                                // override, we don't ask EA for the logo if we have it on disk, but that creates a condition where we tell the UI we don't have it, but what we mean is we didn't look for it on EA's servers
                                Some(GameImage {
                                    retained: None,
                                    renderable: None,
                                    _fs_path: game_logo.to_str().unwrap().to_string().clone(),
                                    url: String::new(),
                                    size: vec2(0.0, 0.0)
                                }.into())
                            } else {
                                None
                            };

                                let game = GameInfo {
                                    slug: game.product().game_slug().clone(),
                                    name: game.product().name().clone(),
                                    offer: game.origin_offer_id().clone(),
                                    //icon: None,
                                    icon_renderable: None,
                                    hero: GameImage {
                                        retained: None,
                                        renderable: None,
                                        _fs_path: game_hero.to_str().unwrap().to_string().clone(),
                                        url: if let Some(img) = &images {
                                            if let Some(pack) = &img.key_art() {
                                                if let Some(img) = &pack.aspect_2x1_image() {
                                                    info!("Setting hero path for {} to {:?}", &slug, img.path().clone());
                                                    img.path().clone()
                                                } else if let Some(img) = &pack.aspect_16x9_image() {
                                                    info!("Setting hero path for {} to {:?}", &slug, img.path().clone());
                                                    img.path().clone()
                                                } else if let Some(img) = &pack.largest_image() {
                                                    info!("Setting hero path for {} to {:?}", &slug, img.path().clone());
                                                    img.path().clone()
                                                } else {
                                                    error!("Failed to get hero path for {}", &slug);
                                                    String::new()
                                                }
                                            } else {
                                                error!("Failed to get pack art for {}", &slug);
                                                String::new()
                                            }
                                        } else {
                                            error!("Failed to get pack art image container for {}", &slug);
                                            String::new()
                                        },
                                        size: vec2(0.0, 0.0)
                                    }.into(),
                                    logo: game_logo,
                                    time: 0,
                                    achievements_unlocked: 0,
                                    achievements_total: 0,
                                    //installed: false,
                                    path: String::new(),
                                    //mods: None,
                                    //tab: crate::GameInfoTab::Achievements
                                };

                                let res = MaximaLibResponse::GameInfoResponse(
                                    InteractThreadGameListResponse {
                                        game,
                                        idx: 0,
                                        total: *games_list.total_count() as usize,
                                    },
                                );
                                tx1.send(res)?;

                                egui::Context::request_repaint(&ctx);
                            }
                        }
                    }
                }
                MaximaLibRequest::StartGameRequest(offer_id) => {
                    let maxima = maxima_arc.lock().await;
                    let logged_in = maxima.auth_storage().lock().await.current().is_some();
                    if !logged_in {
                        info!("Ignoring request to start game, not logged in.");
                        continue;
                    }

                    debug!("got request to start game {:?}", offer_id);
                    let maybe_path: Option<String> = if offer_id.eq("Origin.OFR.50.0001456") {
                        Some(
                            "/home/headass/.local/share/Steam/steamapps/common/Titanfall2/Titanfall2.exe"
                                .to_owned(),
                        )
                    } else if offer_id.eq("Origin.OFR.50.0000739") {
                        Some(
                            "H:\\SteamLibrary\\steamapps\\common\\Titanfall\\Titanfall.exe"
                                .to_owned(),
                        )
                    } else if offer_id.eq("Origin.OFR.50.0004976") {
                        Some(
                            "/kronos/Games/Steam/steamapps/common/Excalibur/NeedForSpeedUnbound.exe"
                                .to_owned(),
                        )
                        
                    } else if offer_id.eq("Origin.OFR.50.0002688") {
                        Some(
                            "/kronos/Games/Oregon/Anthem/Anthem.exe"
                                .to_owned(),
                        )
                    } else if offer_id.eq("Origin.OFR.50.0002148") {
                        Some(
                            "/home/battledash/games/battlefront/starwarsbattlefrontii.exe"
                                .to_owned(),
                        )
                        
                    } else if offer_id.eq("OFB-EAST:109552314") {
                        Some(
                            "/kronos/Games/Steam/steamapps/common/Battlefield 4/bf4.exe"
                                .to_owned(),
                        )
                    } else {
                        None
                    };
                    let maybe_args: Vec<String> = if offer_id.eq("Origin.OFR.50.0001456") {
                        vec!["-windowed".to_string(), "-novid".to_string()]
                    } else if offer_id.eq("Origin.OFR.50.0000739") {
                        vec!["-windowed".to_string(), "-novid".to_string()]
                    } else {
                        vec![]
                    };

                    drop(maxima);
                    let result =
                        launch::start_game(&offer_id, maybe_path, maybe_args, maxima_arc.clone())
                            .await;
                    if result.is_err() {
                        error!("Failed to start game! Reason: {}", result.err().unwrap());
                    }
                }
                MaximaLibRequest::BitchesRequest => {
                    info!("———————————No bitches?————————");
                    info!("⠀⣞⢽⢪⢣⢣⢣⢫⡺⡵⣝⡮⣗⢷⢽⢽⢽⣮⡷⡽⣜⣜⢮⢺⣜⢷⢽⢝⡽⣝");
                    info!("⠸⡸⠜⠕⠕⠁⢁⢇⢏⢽⢺⣪⡳⡝⣎⣏⢯⢞⡿⣟⣷⣳⢯⡷⣽⢽⢯⣳⣫⠇");
                    info!("⠀⠀⢀⢀⢄⢬⢪⡪⡎⣆⡈⠚⠜⠕⠇⠗⠝⢕⢯⢫⣞⣯⣿⣻⡽⣏⢗⣗⠏⠀");
                    info!("⠀⠪⡪⡪⣪⢪⢺⢸⢢⢓⢆⢤⢀⠀⠀⠀⠀⠈⢊⢞⡾⣿⡯⣏⢮⠷⠁⠀⠀⠀");
                    info!("⠀⠀⠀⠈⠊⠆⡃⠕⢕⢇⢇⢇⢇⢇⢏⢎⢎⢆⢄⠀⢑⣽⣿⢝⠲⠉⠀⠀⠀⠀");
                    info!("⠀⠀⠀⠀⠀⡿⠂⠠⠀⡇⢇⠕⢈⣀⠀⠁⠡⠣⡣⡫⣂⣿⠯⢪⠰⠂⠀⠀⠀⠀");
                    info!("⠀⠀⠀⠀⡦⡙⡂⢀⢤⢣⠣⡈⣾⡃⠠⠄⠀⡄⢱⣌⣶⢏⢊⠂⠀⠀⠀⠀⠀⠀");
                    info!("⠀⠀⠀⠀⢝⡲⣜⡮⡏⢎⢌⢂⠙⠢⠐⢀⢘⢵⣽⣿⡿⠁⠁⠀⠀⠀⠀⠀⠀⠀");
                    info!("⠀⠀⠀⠀⠨⣺⡺⡕⡕⡱⡑⡆⡕⡅⡕⡜⡼⢽⡻⠏⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    info!("⠀⠀⠀⠀⣼⣳⣫⣾⣵⣗⡵⡱⡡⢣⢑⢕⢜⢕⡝⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    info!("⠀⠀⠀⣴⣿⣾⣿⣿⣿⡿⡽⡑⢌⠪⡢⡣⣣⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    info!("⠀⠀⠀⡟⡾⣿⢿⢿⢵⣽⣾⣼⣘⢸⢸⣞⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    info!("⠀⠀⠀⠀⠁⠇⠡⠩⡫⢿⣝⡻⡮⣒⢽⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    info!("————————————————————————————-—");
                }
                MaximaLibRequest::ShutdownRequest => {
                    break 'outer
                    Ok(())
                }
            }
        }
    }
}
