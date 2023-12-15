use anyhow::{bail, Result, Error};
use log::{info, error};
use tokio::sync::Mutex;

use std::{
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
    vec::Vec, fs,
};

use maxima::core::{auth::login, launch, service_layer::{ServiceOwnershipMethod, ServiceGame, SERVICE_REQUEST_GAMEIMAGES, ServiceGameImagesRequest, ServiceGameImagesRequestBuilder}};
use maxima::{
    core::{
        self,
        ecommerce::request_offer_data,
        service_layer::{
            send_service_request, ServiceGetUserPlayerRequest, ServiceUser, ServiceUserGameProduct,
            SERVICE_REQUEST_GETUSERPLAYER,
        },
        Maxima, MaximaEvent,
    },
    ooa::{request_license, save_licenses},
    util::{
        self,
        log::LOGGER,
        native::take_foreground_focus,
        registry::{check_registry_validity, bootstrap_path, launch_bootstrap, read_game_path},
    },
};

use crate::{GameInfo, GameImage};

pub struct InteractThreadLoginResponse {
    pub res: Option<String>,
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
    EnableRepaintRequest(egui::Context),
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
    pub fn new() -> Self {
        let (tx0, rx1) = std::sync::mpsc::channel();
        let (tx1, rx0) = std::sync::mpsc::channel();

        tokio::task::spawn(async move {
            let result = MaximaThread::run(rx1, tx1).await;
            if result.is_err() {
                panic!("Interact thread failed! {}", result.err().unwrap());
            }
        });

        Self { rx: rx0, tx: tx0}
    }

    async fn run(rx1: Receiver<MaximaLibRequest>, tx1: Sender<MaximaLibResponse>) -> Result<()> {
        let mut maxima_arc: Option<Arc<Mutex<Maxima>>> = None;

        let mut ui_ctx: Option<egui::Context> = None;
        loop {
            let request = rx1.recv()?;
            match request {
                MaximaLibRequest::LoginRequestOauth => {
                    let token = login::begin_oauth_login_flow().await;
                    let token = token.expect("Login Failed!").expect("Login Failed x2!");
                    let user: ServiceUser = send_service_request(
                        token.as_ref(),
                        SERVICE_REQUEST_GETUSERPLAYER,
                        ServiceGetUserPlayerRequest {},
                    )
                    .await
                    .unwrap();

                    let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                        res: Some(user.player().as_ref().unwrap().display_name().to_owned()),
                    });

                    let local_maxima_arc = Maxima::new();
                    {
                        let mut maxima = local_maxima_arc.lock().await;
                        maxima.set_access_token(token);
                        if maxima.start_lsx(local_maxima_arc.clone()).await.is_ok() {
                            info!("LSX started");
                        } else {
                            info!("LSX failed to start!");
                        }
                    }
                    maxima_arc = Some(local_maxima_arc);
                    tx1.send(lmessage)?;
                    
                    take_foreground_focus().unwrap();
                },
                MaximaLibRequest::LoginRequestUserPass(user, pass) => {
                    todo!();
                },
                MaximaLibRequest::GetGamesRequest => {
                    println!("recieved request to load games");
                    if let Some(maxima) = maxima_arc.clone() {
                        let maxima = maxima.lock().await;
                        let owned_games = maxima.owned_games(1).await.unwrap();
                        println!("{:?}", owned_games);
                        if let Some(games_list) = owned_games.owned_game_products() {
                            for game in games_list.items() {
                                // includes EA play titles, but also lesser editions of owned games
                                /* !game.product.game_product_user.ownership_methods.contains(&ServiceOwnershipMethod::XgpVault) */
                                if true {
                                    
                                    let images: Option<ServiceGame> = // TODO: make it a result
                                        if 
                                        !fs::metadata(format!("./res/{}/hero.jpg",game.product().game_slug().clone())).is_ok()
                                        || !fs::metadata(format!("./res/{}/logo.png",game.product().game_slug().clone())).is_ok()
                                        { //game hasn't been cached yet
                                            // TODO: image downloading
                                            send_service_request(&maxima.access_token(), SERVICE_REQUEST_GAMEIMAGES, ServiceGameImagesRequestBuilder::default().should_fetch_context_image(true).should_fetch_backdrop_images(true).game_slug(game.product().game_slug().clone()).locale(maxima.locale().short_str().to_owned()).build()?).await?
                                        } else { None };

                                    let game = GameInfo {
                                        slug: game.product().game_slug().clone(),
                                        name: game.product().name().clone(),
                                        offer: game.origin_offer_id().clone(),
                                        icon: None,
                                        icon_renderable: None,
                                        hero: GameImage {
                                            retained: None,
                                            renderable: None,
                                            fs_path: format!("./res/{}/hero.jpg",game.product().game_slug().clone()),
                                            url: if let Some(img) = &images {
                                                if let Some(pack) = &img.pack_art() {
                                                    if let Some(img) = &pack.aspect_2x1_image() {
                                                        info!("Setting hero path for {} to {:?}", game.product().game_slug().clone(), img.path().clone());
                                                        img.path().clone()
                                                    } else {
                                                        error!("Failed to get hero path for {}", game.product().game_slug().clone());
                                                        String::new()
                                                    }
                                                } else {
                                                    error!("Failed to get pack art for {}", game.product().game_slug().clone());
                                                    String::new()
                                                }
                                            } else {
                                                error!("Failed to get pack art image container for {}", game.product().game_slug().clone());
                                                String::new()
                                            }
                                        }.into(),
                                        logo: GameImage {
                                            retained: None,
                                            renderable: None,
                                            fs_path: format!("./res/{}/logo.png",game.product().game_slug().clone()),
                                            url: if let Some(img) = &images {
                                                if let Some(logo) = &img.primary_logo() {
                                                    if let Some(l_image) = &logo.largest_image() {
                                                        l_image.path().clone()
                                                    } else {
                                                        error!("Failed to get largest logo for {}", game.product().game_slug().clone());
                                                        String::new()
                                                    }
                                                } else {
                                                    error!("Failed to get logo for {}", game.product().game_slug().clone());
                                                    String::new()
                                                }
                                            } else {
                                                error!("Failed to get logo image container for {}", game.product().game_slug().clone());
                                                String::new()
                                            }
                                        }.into(),
                                        time: 0,
                                        achievements_unlocked: 0,
                                        achievements_total: 0,
                                        installed: false,
                                        path: String::new(),
                                    };

                                    let res = MaximaLibResponse::GameInfoResponse(
                                        InteractThreadGameListResponse {
                                            game,
                                            idx: 0,
                                            total: *games_list.total_count() as usize,
                                        },
                                    );
                                    tx1.send(res)?;

                                    if let Some(ctx) = &ui_ctx {
                                        egui::Context::request_repaint(&ctx);
                                    }
                                }
                            }
                        }
                    } else {
                        println!("Ignoring request to load games, not logged in.");
                    }
                }
                MaximaLibRequest::StartGameRequest(offer_id) => {
                    if let Some(maxima) = maxima_arc.clone() {
                        println!("got request to start game {:?}", offer_id);
                        let maybe_path: Option<String> = if offer_id.eq("Origin.OFR.50.0001456") {
                            Some(
                                "H:\\SteamLibrary\\steamapps\\common\\Titanfall2\\Titanfall2.exe"
                                    .to_owned(),
                            )
                        } else if offer_id.eq("Origin.OFR.50.0000739") {
                            Some(
                                "H:\\SteamLibrary\\steamapps\\common\\Titanfall\\Titanfall.exe"
                                    .to_owned(),
                            )
                        } else if offer_id.eq("Origin.OFR.50.0002688") {
                            Some(
                                "F:\\Games\\ea\\Anthem\\Anthem.exe"
                                    .to_owned(),
                            )
                        } else if offer_id.eq("Origin.OFR.50.0002148") {
                            Some(
                                "/home/battledash/games/battlefront/starwarsbattlefrontii.exe"
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

                        let result =
                            launch::start_game(&offer_id, maybe_path, maybe_args, maxima.clone())
                                .await;
                        if result.is_err() {
                            println!("Failed to start game! Reason: {}", result.err().unwrap());
                        }
                    } else {
                        println!("Ignoring request to start game, not logged in.");
                    }
                }
                MaximaLibRequest::EnableRepaintRequest(egui_context) => {
                    ui_ctx = Some(egui_context);
                }
                MaximaLibRequest::BitchesRequest => {
                    println!("———————————No bitches?————————");
                    println!("⠀⣞⢽⢪⢣⢣⢣⢫⡺⡵⣝⡮⣗⢷⢽⢽⢽⣮⡷⡽⣜⣜⢮⢺⣜⢷⢽⢝⡽⣝");
                    println!("⠸⡸⠜⠕⠕⠁⢁⢇⢏⢽⢺⣪⡳⡝⣎⣏⢯⢞⡿⣟⣷⣳⢯⡷⣽⢽⢯⣳⣫⠇");
                    println!("⠀⠀⢀⢀⢄⢬⢪⡪⡎⣆⡈⠚⠜⠕⠇⠗⠝⢕⢯⢫⣞⣯⣿⣻⡽⣏⢗⣗⠏⠀");
                    println!("⠀⠪⡪⡪⣪⢪⢺⢸⢢⢓⢆⢤⢀⠀⠀⠀⠀⠈⢊⢞⡾⣿⡯⣏⢮⠷⠁⠀⠀⠀");
                    println!("⠀⠀⠀⠈⠊⠆⡃⠕⢕⢇⢇⢇⢇⢇⢏⢎⢎⢆⢄⠀⢑⣽⣿⢝⠲⠉⠀⠀⠀⠀");
                    println!("⠀⠀⠀⠀⠀⡿⠂⠠⠀⡇⢇⠕⢈⣀⠀⠁⠡⠣⡣⡫⣂⣿⠯⢪⠰⠂⠀⠀⠀⠀");
                    println!("⠀⠀⠀⠀⡦⡙⡂⢀⢤⢣⠣⡈⣾⡃⠠⠄⠀⡄⢱⣌⣶⢏⢊⠂⠀⠀⠀⠀⠀⠀");
                    println!("⠀⠀⠀⠀⢝⡲⣜⡮⡏⢎⢌⢂⠙⠢⠐⢀⢘⢵⣽⣿⡿⠁⠁⠀⠀⠀⠀⠀⠀⠀");
                    println!("⠀⠀⠀⠀⠨⣺⡺⡕⡕⡱⡑⡆⡕⡅⡕⡜⡼⢽⡻⠏⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    println!("⠀⠀⠀⠀⣼⣳⣫⣾⣵⣗⡵⡱⡡⢣⢑⢕⢜⢕⡝⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    println!("⠀⠀⠀⣴⣿⣾⣿⣿⣿⡿⡽⡑⢌⠪⡢⡣⣣⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    println!("⠀⠀⠀⡟⡾⣿⢿⢿⢵⣽⣾⣼⣘⢸⢸⣞⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    println!("⠀⠀⠀⠀⠁⠇⠡⠩⡫⢿⣝⡻⡮⣒⢽⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
                    println!("————————————————————————————-—");
                }
            }
        }
    }
}
