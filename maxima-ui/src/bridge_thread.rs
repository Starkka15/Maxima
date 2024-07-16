use anyhow::{Ok, Result};
use egui::Context;
use log::info;

use std::{
    panic,
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    }, time::{Duration, SystemTime},
};

use maxima::core::{launch::ActiveGameContext, LockedMaxima, Maxima, MaximaOptions, MaximaOptionsBuilder};

use crate::{
    bridge::{
        game_details::game_details_request,
        game_images::game_images_request, get_friends::get_friends_request,
        get_games::get_games_request, get_user_avatar::get_user_avatar_request,
        login_creds::login_creds, login_oauth::login_oauth, start_game::start_game_request,
    }, event_thread::{EventThread, MaximaEventRequest, MaximaEventResponse}, ui_image::UIImage, views::friends_view::UIFriend, GameDetails, GameInfo, GameUIImages
};

pub struct InteractThreadLoginResponse {
    pub success: bool,
    pub description: String,
}

pub struct InteractThreadGameListResponse {
    pub game: GameInfo,
}

pub struct InteractThreadFriendListResponse {
    pub friend: UIFriend,
}

pub struct InteractThreadUserAvatarResponse {
    pub id: String,
    pub response: Result<Arc<UIImage>>,
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
    GameInfoResponse(InteractThreadGameListResponse),
    FriendInfoResponse(InteractThreadFriendListResponse),
    UserAvatarResponse(InteractThreadUserAvatarResponse),
    GameDetailsResponse(InteractThreadGameDetailsResponse),
    GameUIImagesResponse(InteractThreadGameUIImagesResponse),
    InteractionThreadDiedResponse,

    // Alerts, rather than responses:

    ActiveGameChanged(Option<String>)
}

pub struct BridgeThread {
    pub backend_listener: Receiver<MaximaLibResponse>,
    pub backend_commander: Sender<MaximaLibRequest>,

    pub rtm_listener: Receiver<MaximaEventResponse>,
    pub rtm_commander: Sender<MaximaEventRequest>, // currently unused except for shutdown
}

impl BridgeThread {
    pub fn new(ctx: &Context) -> Self {
        let (backend_commander, backend_cmd_listener) = std::sync::mpsc::channel();
        let (backend_responder, backend_listener) = std::sync::mpsc::channel();

        let (rtm_commander, rtm_cmd_listener) = std::sync::mpsc::channel();
        let (rtm_responder, rtm_listener) = std::sync::mpsc::channel();
        let context = ctx.clone();

        tokio::task::spawn(async move {
            let die_fallback_transmittter = backend_responder.clone();
            //panic::set_hook(Box::new( |_| {}));
            let result = BridgeThread::run(backend_cmd_listener, backend_responder, rtm_cmd_listener, rtm_responder, &context).await;
            if result.is_err() {
                die_fallback_transmittter
                    .send(MaximaLibResponse::InteractionThreadDiedResponse)
                    .unwrap();
                panic!("Interact thread failed! {}", result.err().unwrap());
            } else {
                info!("Interact thread shut down")
            }
        });

        Self { backend_listener, backend_commander, rtm_listener, rtm_commander }
    }

    async fn run(
        backend_cmd_listener: Receiver<MaximaLibRequest>,
        backend_responder: Sender<MaximaLibResponse>,
        rtm_cmd_listener: Receiver<MaximaEventRequest>,
        rtm_responder: Sender<MaximaEventResponse>,
        ctx: &Context,
    ) -> Result<()> {
        let maxima_arc: LockedMaxima = Maxima::new_with_options(
            MaximaOptionsBuilder::default()
                .dummy_local_user(false)
                .load_auth_storage(true)
                .build()?,
        ).await?;

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

                backend_responder.send(lmessage)?;
                ctx.request_repaint();
            } else {
                backend_responder.send(MaximaLibResponse::LoginCacheEmpty)?;
            }
        }

        let ev_thread = EventThread::new(&ctx.clone(), maxima_arc.clone(), rtm_cmd_listener, rtm_responder);

        let mut future  = SystemTime::now();
        future = future.checked_add(Duration::from_millis(100)).unwrap();
        let mut playing_cache: Option<String> = None;
        'outer: loop {
            let now = SystemTime::now();
            if now >= future {
                // this sucks but it's non-blocking so oh well what are you going to do about it! it's on a non-ui thread anyway, i'm wasteful with it
                future = now.checked_add(Duration::from_millis(100)).unwrap();
                
                let mut maxima = maxima_arc.lock().await;
                maxima.update_playing_status().await;
                let now_playing = maxima.playing();

                if let Some(ctx) = now_playing {
                    if let Some(offer) = ctx.offer() {
                        if playing_cache.is_none() {
                            playing_cache = Some(offer.slug().clone());
                            backend_responder.send(MaximaLibResponse::ActiveGameChanged(Some(offer.slug().clone()))).unwrap();
                        }
                    }
                } else {
                    if playing_cache.is_some() {
                        playing_cache = None;
                        backend_responder.send(MaximaLibResponse::ActiveGameChanged(None)).unwrap();
                    };
                }
            }
            let request = backend_cmd_listener.try_recv();
            if request.is_err() {
                continue;
            }

            match request? {
                MaximaLibRequest::LoginRequestOauth => {
                    let channel = backend_responder.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { login_oauth(maxima, channel, &context).await }.await?;
                }
                MaximaLibRequest::LoginRequestUserPass(user, pass) => {
                    let channel = backend_responder.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { login_creds(maxima, channel, &context, user, pass).await }.await?;
                }
                MaximaLibRequest::GetGamesRequest => {
                    let channel = backend_responder.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { get_games_request(maxima, channel, &context).await }.await?;
                }
                MaximaLibRequest::GetFriendsRequest => {
                    let channel = backend_responder.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { get_friends_request(maxima, channel, &context).await }.await?;
                }
                MaximaLibRequest::GetGameImagesRequest(slug) => {
                    let channel = backend_responder.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { game_images_request(maxima, slug, channel, &context).await }
                        .await?;
                }
                MaximaLibRequest::GetUserAvatarRequest(id, url) => {
                    let channel = backend_responder.clone();
                    let context = ctx.clone();
                    async move { get_user_avatar_request(channel, id, url, &context).await }
                        .await?;
                }
                MaximaLibRequest::GetGameDetailsRequest(slug) => {
                    let channel = backend_responder.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move {
                        game_details_request(maxima, slug.clone(), channel, &context).await
                    }
                    .await?;
                }
                MaximaLibRequest::StartGameRequest(offer_id, hardcode) => {
                    start_game_request(maxima_arc.clone(), offer_id.clone(), hardcode).await;
                }
                MaximaLibRequest::ShutdownRequest => break 'outer Ok(()), //TODO: kill the bridge thread
            }
        }
    }
}
