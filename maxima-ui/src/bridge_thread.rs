use egui::Context;
use log::{error, info, warn};

use crate::{
    bridge::{
        game_details::game_details_request, get_friends::get_friends_request,
        get_games::get_games_request, login_oauth::login_oauth, start_game::start_game_request,
    },
    event_thread::{EventThread, MaximaEventRequest, MaximaEventResponse},
    ui_image::UIImageCacheLoaderCommand,
    views::friends_view::UIFriend,
    GameDetails, GameInfo, GameSettings,
};
use maxima::{
    content::manager::{
        ContentManager, ContentManagerError, QueuedGameBuilder, QueuedGameBuilderError,
    },
    core::{
        auth::storage::{AuthError, TokenError},
        launch::LaunchError,
        library::LibraryError,
        manifest::{self, ManifestError, MANIFEST_RELATIVE_PATH},
        service_layer::{
            ServiceGameImagesRequestBuilderError, ServiceHeroBackgroundImageRequestBuilderError,
            ServiceLayerError, ServicePlayer,
        },
        LockedMaxima, Maxima, MaximaCreationError, MaximaOptionsBuilder, MaximaOptionsBuilderError,
    },
    lsx::service::LSXServerError,
    rtm::RtmError,
    util::{
        native::NativeError,
        registry::{check_registry_validity, set_up_registry, RegistryError},
    },
};
use std::sync::mpsc::{SendError, TryRecvError};
use std::{
    panic,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, SystemTime},
};

// TODO(headassbtw): integrate these all into the enums
pub struct InteractThreadLoginResponse {
    pub you: ServicePlayer,
}

pub struct InteractThreadGameListResponse {
    pub game: GameInfo,
    pub settings: GameSettings,
}

pub struct InteractThreadFriendListResponse {
    pub friend: UIFriend,
}

pub struct InteractThreadGameDetailsResponse {
    pub slug: String,
    pub response: GameDetails,
}

pub struct InteractThreadLocateGameFailure {
    pub reason: ManifestError,
    pub xml_path: String,
}

pub enum InteractThreadLocateGameResponse {
    Success,
    Error(InteractThreadLocateGameFailure),
}

pub struct InteractThreadDownloadProgressResponse {
    pub bytes: usize,
    pub bytes_total: usize,
}

pub enum MaximaLibRequest {
    StartService,
    LoginRequestOauth,
    GetGamesRequest,
    GetFriendsRequest,
    GetGameDetailsRequest(String),
    StartGameRequest(GameInfo, Option<GameSettings>),
    InstallGameRequest(String, PathBuf),
    LocateGameRequest(String),
    ShutdownRequest,
}

pub enum MaximaLibResponse {
    LoginResponse(Result<InteractThreadLoginResponse, anyhow::Error>),
    LoginCacheEmpty,
    ServiceNeedsStarting,
    ServiceStarted,
    GameInfoResponse(InteractThreadGameListResponse),
    FriendInfoResponse(InteractThreadFriendListResponse),
    GameDetailsResponse(InteractThreadGameDetailsResponse),
    LocateGameResponse(InteractThreadLocateGameResponse),
    // Alerts, rather than responses:
    CriticalError(Box<BackendError>),
    NonFatalError(Box<BackendError>),
    ActiveGameChanged(Option<String>),
    DownloadProgressChanged(String, InteractThreadDownloadProgressResponse),
    DownloadFinished(String),
    DownloadQueueUpdate(Option<String>, Vec<String>),
}
pub struct BridgeThread {
    pub backend_listener: Receiver<MaximaLibResponse>,
    pub backend_commander: Sender<MaximaLibRequest>,

    pub rtm_listener: Receiver<MaximaEventResponse>,
    pub rtm_commander: Sender<MaximaEventRequest>, // currently unused except for shutdown
}

#[derive(thiserror::Error, Debug)]
pub enum BackendError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    BackgroundServiceControl(#[from] maxima::util::BackgroundServiceControlError),
    #[error(transparent)]
    BackgroundServiceClient(#[from] maxima::core::error::BackgroundServiceClientError),
    #[error(transparent)]
    ContentManager(#[from] ContentManagerError),
    #[error(transparent)]
    Launch(#[from] LaunchError),
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    LSXServer(#[from] LSXServerError),
    #[error(transparent)]
    MaximaCreation(#[from] MaximaCreationError),
    #[error(transparent)]
    MaximaOptionsBuilder(#[from] MaximaOptionsBuilderError),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    QueuedGameBuilder(#[from] QueuedGameBuilderError),
    #[error(transparent)]
    RegistryError(#[from] RegistryError),
    #[error(transparent)]
    Rtm(#[from] RtmError),
    #[error(transparent)]
    SendResponse(#[from] SendError<MaximaLibResponse>),
    #[error(transparent)]
    SendImageCacheLoaderCommand(#[from] SendError<UIImageCacheLoaderCommand>),
    #[error(transparent)]
    ServiceGameImagesRequestBuilder(#[from] ServiceGameImagesRequestBuilderError),
    #[error(transparent)]
    ServiceHeroBackgroundImageRequestBuilder(#[from] ServiceHeroBackgroundImageRequestBuilderError),
    #[error(transparent)]
    ServiceLayer(#[from] ServiceLayerError),
    #[error(transparent)]
    Token(#[from] TokenError),
    #[error(transparent)]
    TryRecv(#[from] TryRecvError),

    #[error("backend-frontend communication channel disconnected")]
    ChannelDisconnected,
    #[error("tried to perform an action that requires being logged in, but was logged out")]
    LoggedOut,
}

impl BridgeThread {
    fn update_queue(
        content_manager: &ContentManager,
        backend_responder: Sender<MaximaLibResponse>,
    ) {
        let current = if let Some(now) = content_manager.queue().current() {
            Some(now.offer_id().to_owned())
        } else {
            None
        };

        let mut queue: Vec<String> = Vec::new();

        for game in content_manager.queue().queued() {
            queue.push(game.offer_id().to_owned());
        }

        backend_responder
            .send(MaximaLibResponse::DownloadQueueUpdate(current, queue))
            .unwrap();
    }

    pub fn new(ctx: &Context, remote_provider_channel: Sender<UIImageCacheLoaderCommand>) -> Self {
        puffin::profile_function!();
        let (backend_commander, backend_cmd_listener) = std::sync::mpsc::channel();
        let (backend_responder, backend_listener) = std::sync::mpsc::channel();

        let (rtm_commander, rtm_cmd_listener) = std::sync::mpsc::channel();
        let (rtm_responder, rtm_listener) = std::sync::mpsc::channel();
        let context = ctx.clone();

        tokio::task::spawn(async move {
            let die_fallback_transmitter = backend_responder.clone();
            //panic::set_hook(Box::new( |_| {}));
            let result = BridgeThread::run(
                backend_cmd_listener,
                backend_responder,
                rtm_cmd_listener,
                rtm_responder,
                remote_provider_channel,
                &context,
            )
            .await;
            if let Err(err) = result {
                die_fallback_transmitter
                    .send(MaximaLibResponse::CriticalError(Box::from(err)))
                    .unwrap();
            } else {
                info!("Interact thread shut down")
            }
        });

        Self {
            backend_listener,
            backend_commander,
            rtm_listener,
            rtm_commander,
        }
    }

    async fn run(
        backend_cmd_listener: Receiver<MaximaLibRequest>,
        backend_responder: Sender<MaximaLibResponse>,
        rtm_cmd_listener: Receiver<MaximaEventRequest>,
        rtm_responder: Sender<MaximaEventResponse>,
        remote_provider_channel: Sender<UIImageCacheLoaderCommand>,
        ctx: &Context,
    ) -> Result<(), BackendError> {
        // first things first check registry
        // the flow is different for windows/linux but windows needs an extra user prompt,
        // so we're doing both here, instead of selectively cfg'd functions!
        #[cfg(not(windows))]
        {
            if let Err(err) = check_registry_validity() {
                warn!("{}, fixing...", err);
                set_up_registry()?;
            }
        }
        #[cfg(windows)]
        {
            use maxima::{
                core::background_service::request_registry_setup,
                util::{
                    registry::check_registry_validity,
                    service::{
                        is_service_running, is_service_valid, register_service_user, start_service,
                    },
                },
            };
            if !is_elevated::is_elevated() {
                if !is_service_valid()? {
                    info!("Installing service...");
                    backend_responder.send(MaximaLibResponse::ServiceNeedsStarting)?;
                    'wait_for_user_to_authorize: loop {
                        let request = backend_cmd_listener.try_recv();
                        if request.is_err() {
                            continue;
                        }

                        match request.unwrap() {
                            MaximaLibRequest::StartService => {
                                register_service_user()?;
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                break 'wait_for_user_to_authorize;
                            }
                            MaximaLibRequest::ShutdownRequest => return Ok(()),
                            _ => {}
                        }
                    }
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
        }
        let maxima_arc: LockedMaxima = Maxima::new_with_options(
            MaximaOptionsBuilder::default()
                .dummy_local_user(false)
                .load_auth_storage(true)
                .build()?,
        )
        .await?;

        let logged_in = {
            let maxima = maxima_arc.lock().await;
            maxima.start_lsx(maxima_arc.clone()).await?;
            info!("LSX started");

            let mut auth_storage = maxima.auth_storage().lock().await;
            auth_storage.logged_in().await?
        };

        if !logged_in {
            backend_responder.send(MaximaLibResponse::LoginCacheEmpty)?;
            'outer: loop {
                let request = backend_cmd_listener.try_recv();
                if request.is_err() {
                    continue;
                }

                match request? {
                    MaximaLibRequest::LoginRequestOauth => {
                        let channel = backend_responder.clone();
                        let maxima = maxima_arc.clone();
                        let context = ctx.clone();
                        async move { login_oauth(maxima, channel, &context).await }
                            .await
                            .expect("// TODO(headassbtw): panic message");
                        break 'outer;
                    }
                    MaximaLibRequest::ShutdownRequest => return Ok(()),
                    _ => {}
                }
            }
        }

        {
            let maxima = maxima_arc.lock().await;
            let user = maxima.local_user().await?;

            if logged_in {
                let message = MaximaLibResponse::LoginResponse(Ok(InteractThreadLoginResponse {
                    you: user.player().as_ref().unwrap().to_owned(),
                }));
                backend_responder.send(message)?;
            }
            let res = remote_provider_channel.send(UIImageCacheLoaderCommand::ProvideRemote(
                crate::ui_image::UIImageType::Avatar(user.id().to_string()),
                user.player()
                    .as_ref()
                    .ok_or(ServiceLayerError::MissingField)?
                    .avatar()
                    .as_ref()
                    .ok_or(ServiceLayerError::MissingField)?
                    .medium()
                    .path()
                    .to_string(),
            ));
            if let Err(err) = res {
                error!("failed to send user pfp to loader: {:?}", err);
            }
            ctx.request_repaint();
        }

        let _ = EventThread::new(
            &ctx.clone(),
            maxima_arc.clone(),
            rtm_cmd_listener,
            rtm_responder,
        );

        let mut future = SystemTime::now();
        future = future.checked_add(Duration::from_millis(50)).unwrap();
        let mut playing_cache: Option<String> = None;
        'outer: loop {
            let now = SystemTime::now();
            if now >= future {
                // this sucks but it's non-blocking so oh well what are you going to do about it! it's on a non-ui thread anyway, i'm wasteful with it
                future = now.checked_add(Duration::from_millis(50)).unwrap();

                let mut maxima = maxima_arc.lock().await;
                maxima.update().await;
                let now_playing = maxima.playing();

                if let Some(ctx) = now_playing {
                    if let Some(offer) = ctx.offer() {
                        if playing_cache.is_none() {
                            playing_cache = Some(offer.slug().clone());
                            backend_responder.send(MaximaLibResponse::ActiveGameChanged(Some(
                                offer.slug().clone(),
                            )))?;
                        }
                    }
                } else {
                    if playing_cache.is_some() {
                        playing_cache = None;
                        backend_responder.send(MaximaLibResponse::ActiveGameChanged(None)).unwrap();
                    };
                }

                if let Some(dl) = maxima.content_manager().current() {
                    backend_responder.send(MaximaLibResponse::DownloadProgressChanged(
                        dl.offer_id().to_string(),
                        InteractThreadDownloadProgressResponse {
                            bytes: dl.bytes_downloaded(),
                            bytes_total: dl.bytes_total(),
                        },
                    ))?;
                }

                for ev in maxima.consume_pending_events() {
                    match ev {
                        maxima::core::MaximaEvent::ReceivedLSXRequest(_, _) => {}
                        maxima::core::MaximaEvent::InstallFinished(offer_id) => {
                            backend_responder
                                .send(MaximaLibResponse::DownloadFinished(offer_id))?;
                            Self::update_queue(maxima.content_manager(), backend_responder.clone());
                        }
                    }
                }
            }
            let request = backend_cmd_listener.try_recv();
            if request.is_err() {
                continue;
            }

            let action = match request? {
                MaximaLibRequest::LoginRequestOauth | MaximaLibRequest::StartService => {
                    error!("bro tried to log in twice");
                    Ok(())
                }
                MaximaLibRequest::GetGamesRequest => {
                    let channel = backend_responder.clone();
                    let channel1 = remote_provider_channel.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { get_games_request(maxima, channel, channel1, &context).await }
                        .await
                }
                MaximaLibRequest::GetFriendsRequest => {
                    let channel = backend_responder.clone();
                    let channel1 = remote_provider_channel.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { get_friends_request(maxima, channel, channel1, &context).await }
                        .await
                }
                MaximaLibRequest::GetGameDetailsRequest(slug) => {
                    let channel = backend_responder.clone();
                    let maxima = maxima_arc.clone();
                    let context = ctx.clone();
                    async move { game_details_request(maxima, slug.clone(), channel, &context).await }.await
                }
                MaximaLibRequest::LocateGameRequest(path) => {
                    #[cfg(unix)]
                    maxima::core::launch::mx_linux_setup().await?;
                    let mut path = path;
                    if path.ends_with("/") || path.ends_with("\\") {
                        path.remove(path.len() - 1);
                    }
                    let path = PathBuf::from(path);
                    let manifest = manifest::read(path.join(MANIFEST_RELATIVE_PATH)).await;
                    if let Ok(manifest) = manifest {
                        let guh = manifest.run_touchup(&path).await;
                        if let Err(err) = guh {
                            let _ = backend_responder.send(MaximaLibResponse::LocateGameResponse(
                                InteractThreadLocateGameResponse::Error(
                                    InteractThreadLocateGameFailure {
                                        reason: err,
                                        xml_path: path
                                            .join(MANIFEST_RELATIVE_PATH)
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                    },
                                ),
                            ));
                        } else {
                            let _ = backend_responder.send(MaximaLibResponse::LocateGameResponse(
                                InteractThreadLocateGameResponse::Success,
                            ));
                        }
                    } else {
                        let _ = backend_responder.send(MaximaLibResponse::LocateGameResponse(
                            InteractThreadLocateGameResponse::Error(
                                InteractThreadLocateGameFailure {
                                    reason: manifest.unwrap_err(),
                                    xml_path: path
                                        .join(MANIFEST_RELATIVE_PATH)
                                        .to_str()
                                        .unwrap()
                                        .to_string(),
                                },
                            ),
                        ));
                    }
                    info!("finished locating");
                    ctx.request_repaint();
                    Ok(())
                }
                MaximaLibRequest::InstallGameRequest(offer, path) => {
                    let mut maxima = maxima_arc.lock().await;
                    let builds =
                        maxima.content_manager().service().available_builds(&offer).await?;
                    let build = if let Some(build) = builds.live_build() {
                        build
                    } else {
                        continue;
                    };

                    let game = QueuedGameBuilder::default()
                        .offer_id(offer)
                        .build_id(build.build_id().to_owned())
                        .path(path.to_owned())
                        .build()?;
                    Ok(maxima.content_manager().add_install(game).await?)
                }
                MaximaLibRequest::StartGameRequest(info, settings) => {
                    Ok(start_game_request(maxima_arc.clone(), info, settings).await?)
                }
                MaximaLibRequest::ShutdownRequest => break 'outer Ok(()), //TODO: kill the bridge thread
            };
            if let Err(err) = action {
                let _ = backend_responder.send(MaximaLibResponse::NonFatalError(Box::from(err)));
            }

            puffin::GlobalProfiler::lock().new_frame();
        }
    }
}
