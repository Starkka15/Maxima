use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::{bail, Result};
use log::info;
use maxima::core::{
    auth::{
        context::AuthContext, login::begin_oauth_login_flow, nucleus_token_exchange, TokenResponse,
    },
    LockedMaxima, Maxima, MaximaOptionsBuilder,
};

pub struct InteractThreadLoginResponse {
    pub success: bool,
    pub name: String,
}

pub enum MaximaLibRequest {
    LoginRequest,
    GetGamesRequest,
    GetFriendsRequest,
    GetUserAvatarRequest(String, String),
    GetGameImagesRequest(String),
    GetGameDetailsRequest(String),
    StartGameRequest(String, bool),
    ShutdownRequest,
}

pub enum MaximaLibResponse {
    LoginResponse(InteractThreadLoginResponse),
    LoginCacheEmpty,
    GameInfoResponse(),
    FriendInfoResponse(),
    UserAvatarResponse(),
    GameDetailsResponse(),
    GameUIImagesResponse(),
    InteractionThreadDiedResponse,
}

pub struct BridgeThread {
    pub rx: Receiver<MaximaLibResponse>,
    pub tx: Sender<MaximaLibRequest>,
}

impl BridgeThread {
    pub fn new() -> Self {
        let (tx0, rx1) = mpsc::channel();
        let (tx1, rx0) = mpsc::channel();

        tokio::task::spawn(async move {
            let die_fallback_transmitter = tx1.clone();
            //panic::set_hook(Box::new( |_| {}));
            let result = BridgeThread::run(rx1, tx1).await;
            if result.is_err() {
                die_fallback_transmitter
                    .send(MaximaLibResponse::InteractionThreadDiedResponse)
                    .unwrap();
                panic!("Interact thread failed! {}", result.err().unwrap());
            } else {
                info!("Interact thread shut down")
            }
        });

        Self { rx: rx0, tx: tx0 }
    }

    async fn run(rx1: Receiver<MaximaLibRequest>, tx1: Sender<MaximaLibResponse>) -> Result<()> {
        let maxima_arc: LockedMaxima = Maxima::new_with_options(
            MaximaOptionsBuilder::default()
                .dummy_local_user(false)
                .load_auth_storage(true)
                .build()?,
        )
        .await?;

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
                let message = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
                    success: true,
                    name: user.player().as_ref().unwrap().display_name().to_owned(),
                });

                tx1.send(message)?;
            } else {
                tx1.send(MaximaLibResponse::LoginCacheEmpty)?;
            }
        }

        'outer: loop {
            let request = rx1.try_recv();
            if request.is_err() {
                continue;
            }

            match request? {
                MaximaLibRequest::LoginRequest => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                    async move {
                        let maxima = maxima.lock().await;

                        let mut auth_storage = maxima.auth_storage().lock().await;
                        let logged_in = auth_storage.logged_in().await?;
                        if !logged_in {
                            let res = login_flow().await.unwrap();
                            maxima.auth_storage().lock().await.add_account(&res);
                        };

                        channel
                            .send(MaximaLibResponse::LoginResponse(
                                InteractThreadLoginResponse {
                                    success: true,
                                    name: maxima
                                        .local_user()
                                        .await?
                                        .player()
                                        .as_ref()
                                        .unwrap()
                                        .display_name()
                                        .to_owned(),
                                },
                            ))
                            .unwrap();

                        Ok::<(), anyhow::Error>(())
                    }
                    .await?;
                }
                MaximaLibRequest::GetGamesRequest => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                }
                MaximaLibRequest::GetFriendsRequest => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                }
                MaximaLibRequest::GetGameImagesRequest(slug) => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                }
                MaximaLibRequest::GetUserAvatarRequest(id, url) => {
                    let channel = tx1.clone();
                }
                MaximaLibRequest::GetGameDetailsRequest(slug) => {
                    let channel = tx1.clone();
                    let maxima = maxima_arc.clone();
                }
                MaximaLibRequest::StartGameRequest(offer_id, hardcode) => {
                    //start_game_request(maxima_arc.clone(), offer_id.clone(), hardcode).await;
                }
                MaximaLibRequest::ShutdownRequest => break 'outer Ok(()),
            }
        }
    }
}

pub async fn login_flow() -> Result<TokenResponse> {
    let mut auth_context = AuthContext::new()?;
    begin_oauth_login_flow(&mut auth_context).await?;

    if auth_context.code().is_none() {
        bail!("Login failed!");
    }

    info!("Received login...");

    let token_res = nucleus_token_exchange(&auth_context).await;
    if token_res.is_err() {
        bail!("Login failed: {}", token_res.err().unwrap().to_string());
    }

    Ok(token_res?)
}
