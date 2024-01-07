use anyhow::{Ok, Result};
use egui::Context;
use log::info;
use tokio::sync::Mutex;

use std::{
    panic,
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
};

use maxima::core::Maxima;

use crate::{
    bridge::{
        bitches::bitches_request, game_details::game_details_request,
        game_images::game_images_request, get_games::get_games_request, login_creds::login_creds,
        login_oauth::login_oauth, start_game::start_game_request,
    },
    GameDetails, GameInfo, GameUIImages,
};

pub struct InteractThreadLoginResponse {
    pub success: bool,
    pub description: String,
}

pub struct InteractThreadGameListResponse {
    pub game: GameInfo,
}

pub struct InteractThreadGameDetailsResponse {
    pub slug: String,
    pub response: Result<GameDetails>,
}

pub struct InteractThreadGameUIImagesResponse {
    pub slug: String,
    pub response: Result<GameUIImages>,
}

pub enum MaximaLibRequest {
    LoginRequestOauth,
    LoginRequestUserPass(String, String),
    GetGamesRequest,
    GetGameImagesRequest(String),
    GetGameDetailsRequest(String),
    StartGameRequest(String),
    BitchesRequest,
    ShutdownRequest,
}

pub enum MaximaLibResponse {
    LoginResponse(InteractThreadLoginResponse),
    GameInfoResponse(InteractThreadGameListResponse),
    GameDetailsResponse(InteractThreadGameDetailsResponse),
    GameUIImagesResponse(InteractThreadGameUIImagesResponse),
    InteractionThreadDiedResponse,
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
            let die_fallback_transmittter = tx1.clone();
            //panic::set_hook(Box::new( |_| {}));
            let result = MaximaThread::run(rx1, tx1, &context).await;
            if result.is_err() {
                die_fallback_transmittter
                    .send(MaximaLibResponse::InteractionThreadDiedResponse).unwrap();
                panic!("Interact thread failed! {}", result.err().unwrap());
            } else {
                info!("Interact thread shut down")
            }
        });

        Self { rx: rx0, tx: tx0 }
    }

    async fn run(
        rx1: Receiver<MaximaLibRequest>,
        tx1: Sender<MaximaLibResponse>,
        ctx: &Context,
    ) -> Result<()> {
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
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { login_oauth(maxima, channel, &context).await }.await?;
                }
                MaximaLibRequest::LoginRequestUserPass(user, pass) => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { login_creds(maxima, channel, &context, user, pass).await }.await?;
                }
                MaximaLibRequest::GetGamesRequest => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { get_games_request(maxima, channel, &context).await }.await?;
                }
                MaximaLibRequest::GetGameImagesRequest(slug) => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { game_images_request(maxima, slug, channel, &context).await }
                        .await?;
                }
                MaximaLibRequest::GetGameDetailsRequest(slug) => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move {
                        game_details_request(maxima, slug.clone(), channel, &context).await
                    }
                    .await?;
                }
                MaximaLibRequest::StartGameRequest(offer_id) => {
                    start_game_request(maxima_arc.clone(), offer_id.clone()).await;
                }
                MaximaLibRequest::BitchesRequest => {
                    bitches_request();
                }
                MaximaLibRequest::ShutdownRequest => break 'outer Ok(()),
            }
        }
    }
}
