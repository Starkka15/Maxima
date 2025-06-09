pub mod auth;
pub mod cache;
pub mod clients;
pub mod cloudsync;
pub mod concurrency;
pub mod ecommerce;
pub mod endpoints;
pub mod error;
pub mod launch;
pub mod library;
pub mod locale;
pub mod manifest;
pub mod service_layer;
pub mod settings;

#[cfg(target_os = "windows")]
mod background_service_win;

#[cfg(target_os = "linux")]
mod background_service_nix;

pub mod background_service {
    #[cfg(target_os = "windows")]
    pub use super::background_service_win::*;

    #[cfg(target_os = "linux")]
    pub use super::background_service_nix::*;
}

use std::{
    env,
    fs::{create_dir_all, File},
    io,
    os::raw::c_char,
    path::PathBuf,
    time::Duration,
};

use cloudsync::{CloudSyncClient, CloudSyncLockMode};
use derive_builder::Builder;
use derive_getters::Getters;
use log::{error, info, warn};
use strum_macros::IntoStaticStr;

use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

use self::{
    auth::storage::{AuthError, AuthStorage, LockedAuthStorage, TokenError},
    cache::DynamicCache,
    launch::ActiveGameContext,
    library::GameLibrary,
    locale::Locale,
    service_layer::{
        ServiceAvatarListBuilder, ServiceAvatarListBuilderError, ServiceFriends,
        ServiceGetBasicPlayerRequestBuilder, ServiceGetMyFriendsRequestBuilder,
        ServiceGetUserPlayerRequest, ServiceImage, ServiceImageBuilder, ServiceImageBuilderError,
        ServiceLayerClient, ServiceLayerError, ServicePlayer, ServicePlayerBuilder,
        ServicePlayerBuilderError, ServiceUser, ServiceUserBuilder, ServiceUserBuilderError,
        SERVICE_REQUEST_GETBASICPLAYER, SERVICE_REQUEST_GETMYFRIENDS,
        SERVICE_REQUEST_GETUSERPLAYER,
    },
};
use crate::{
    content::manager::{ContentManager, ContentManagerError},
    lsx::{self, service::LSXServerError, types::LSXRequestType},
    rtm::client::{BasicPresence, RtmClient},
    util::native::{maxima_dir, NativeError},
};

#[derive(Clone, IntoStaticStr)]
pub enum MaximaEvent {
    /// PID, Request Type
    ReceivedLSXRequest(u32, LSXRequestType),
    /// Offer ID. Use `maxima.mut_library().title_by_base_offer(id)` for details
    InstallFinished(String),
}

pub type MaximaLSXEventCallback = extern "C" fn(*const c_char);

#[derive(Getters)]
pub struct Maxima {
    locale: Locale,

    auth_storage: LockedAuthStorage,
    service_layer: ServiceLayerClient,

    #[getter(skip)]
    library: GameLibrary,

    playing: Option<ActiveGameContext>,

    lsx_port: u16,
    lsx_event_callback: Option<MaximaLSXEventCallback>,
    lsx_connections: u16,

    cloud_sync: CloudSyncClient,

    #[getter(skip)]
    content_manager: ContentManager,

    #[getter(skip)]
    rtm: RtmClient,

    #[getter(skip)]
    request_cache: DynamicCache<String>,

    #[getter(skip)]
    dummy_local_user: Option<ServiceUser>,

    #[getter(skip)]
    pending_events: Vec<MaximaEvent>,
}

#[derive(Builder)]
pub struct MaximaOptions {
    load_auth_storage: bool,
    dummy_local_user: bool,
}

#[derive(Error, Debug)]
pub enum MaximaCreationError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    ContentManager(#[from] ContentManagerError),
    #[error(transparent)]
    MaximaOptionsBuilder(#[from] MaximaOptionsBuilderError),
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),
    #[error(transparent)]
    ServiceAvatarListBuilder(#[from] ServiceAvatarListBuilderError),
    #[error(transparent)]
    ServiceImageBuilder(#[from] ServiceImageBuilderError),
    #[error(transparent)]
    ServicePlayerBuilder(#[from] ServicePlayerBuilderError),
    #[error(transparent)]
    ServiceUserBuilder(#[from] ServiceUserBuilderError),
}

pub type LockedMaxima = Arc<Mutex<Maxima>>;

impl Maxima {
    pub async fn new_with_options(
        options: MaximaOptions,
    ) -> Result<LockedMaxima, MaximaCreationError> {
        let lsx_port = if let Ok(lsx_port) = env::var("MAXIMA_LSX_PORT") {
            lsx_port.parse::<u16>()?
        } else {
            3216
        };

        let request_cache = DynamicCache::new(
            10_000,
            Duration::from_secs(30 * 60),
            Duration::from_secs(5 * 60),
        );

        let auth_storage = if options.load_auth_storage {
            AuthStorage::load()?
        } else {
            AuthStorage::new()
        };

        let dummy_local_user = if options.dummy_local_user {
            let avatar_image = ServiceImageBuilder::default()
                .height(Some(256))
                .width(Some(256))
                .path("".to_owned())
                .build()?;

            let name = "DummyUser".to_owned();

            let avatar_list = ServiceAvatarListBuilder::default()
                .large(avatar_image.clone())
                .medium(avatar_image.clone())
                .small(avatar_image)
                .build()?;

            let player = ServicePlayerBuilder::default()
                .id("0".to_owned())
                .pd("0".to_owned())
                .psd("0".to_owned())
                .display_name(name.to_owned())
                .unique_name(name.to_owned())
                .nickname(name)
                .avatar(Some(avatar_list))
                .relationship("self".to_owned())
                .build()?;

            Some(
                ServiceUserBuilder::default()
                    .id("0".to_owned())
                    .pd(Some("0".to_owned()))
                    .player(Some(player))
                    .owned_game_products(None)
                    .build()?,
            )
        } else {
            None
        };

        Ok(Arc::new(Mutex::new(Self {
            locale: Locale::EnUs,
            auth_storage: auth_storage.clone(),
            service_layer: ServiceLayerClient::new(auth_storage.clone()),
            library: GameLibrary::new(auth_storage.clone()).await,
            playing: None,
            lsx_port,
            lsx_event_callback: None,
            lsx_connections: 0,
            cloud_sync: CloudSyncClient::new(auth_storage.clone()),
            content_manager: ContentManager::new(auth_storage.clone(), false).await?,
            rtm: RtmClient::new(auth_storage),
            request_cache,
            dummy_local_user,
            pending_events: Vec::new(),
        })))
    }

    pub async fn new() -> Result<LockedMaxima, MaximaCreationError> {
        Maxima::new_with_options(
            MaximaOptionsBuilder::default()
                .load_auth_storage(true)
                .dummy_local_user(false)
                .build()?,
        )
        .await
    }

    pub async fn start_lsx(&self, maxima: LockedMaxima) -> Result<(), LSXServerError> {
        let lsx_port = self.lsx_port;

        tokio::spawn(async move {
            if let Err(e) = lsx::service::start_server(lsx_port, maxima).await {
                error!("Error starting LSX server: {}", e);
            }
        });

        tokio::task::yield_now().await;
        Ok(())
    }

    pub async fn access_token(&mut self) -> Result<String, TokenError> {
        let mut auth_storage = self.auth_storage.lock().await;
        match auth_storage.access_token().await? {
            None => Err(TokenError::Absent),
            Some(token) => Ok(token),
        }
    }

    pub async fn local_user(&self) -> Result<ServiceUser, ServiceLayerError> {
        if let Some(user) = self.dummy_local_user.clone() {
            return Ok(user);
        }

        let cache_key = "user_player";
        if let Some(cached) = self.request_cache.get(cache_key) {
            return Ok(cached);
        }

        let user: ServiceUser = self
            .service_layer
            .request(
                SERVICE_REQUEST_GETUSERPLAYER,
                ServiceGetUserPlayerRequest {},
            )
            .await?;

        self.request_cache
            .insert(cache_key.to_owned(), user.clone());
        Ok(user)
    }

    pub async fn friends(&self, page: u32) -> Result<Vec<ServicePlayer>, ServiceLayerError> {
        let cache_key = format!("friends_{}", page);
        if let Some(cached) = self.request_cache.get(&cache_key) {
            return Ok(cached);
        }

        let friends: ServiceFriends = self
            .service_layer
            .request(
                SERVICE_REQUEST_GETMYFRIENDS,
                ServiceGetMyFriendsRequestBuilder::default()
                    .limit(100)
                    .offset(page)
                    .is_mutual_friends_enabled(false)
                    .build()
                    .unwrap(),
            )
            .await?;

        let friends: Vec<ServicePlayer> = friends
            .friends()
            .items()
            .into_iter()
            .map(|x| x.player().clone())
            .collect();

        self.request_cache.insert(cache_key, friends.clone());
        Ok(friends)
    }

    pub fn call_event(&mut self, event: MaximaEvent) {
        self.pending_events.push(event);
    }

    pub fn consume_pending_events(&mut self) -> Vec<MaximaEvent> {
        let events = self.pending_events.clone();
        self.pending_events.clear();
        events
    }

    pub async fn player_by_id(&self, id: &str) -> Result<ServicePlayer, ServiceLayerError> {
        if let Some(user) = &self.dummy_local_user {
            return Ok(user
                .player()
                .as_ref()
                .ok_or(ServiceLayerError::MissingField)?
                .clone());
        }

        let cache_key = "basic_player_".to_owned() + id;
        if let Some(cached) = self.request_cache.get(&cache_key) {
            return Ok(cached);
        }

        let data: ServicePlayer = self
            .service_layer
            .request(
                SERVICE_REQUEST_GETBASICPLAYER,
                ServiceGetBasicPlayerRequestBuilder::default()
                    .pd(id.to_string())
                    .build()
                    .unwrap(),
            )
            .await?;

        let avatars = data.avatar();

        let avatars = avatars.as_ref().ok_or(ServiceLayerError::MissingField)?;
        let _ = self.cache_avatar_image(&id, avatars.large()).await;
        let _ = self.cache_avatar_image(&id, avatars.medium()).await;
        let _ = self.cache_avatar_image(&id, avatars.small()).await;

        self.request_cache.insert(cache_key, data.clone());
        Ok(data)
    }

    async fn cache_avatar_image(
        &self,
        id: &str,
        image: &ServiceImage,
    ) -> Result<(), error::CacheRetrievalError> {
        let path = self.cached_avatar_path(
            id,
            image.width().unwrap_or(727),
            image.height().unwrap_or(727),
        )?;

        if path.exists() {
            return Ok(());
        }

        let response = reqwest::get(image.path()).await?;
        let body: Vec<u8> = response.bytes().await?.to_vec();

        let mut file = File::create(path)?;
        io::copy(&mut body.as_slice(), &mut file)?;

        Ok(())
    }

    pub async fn avatar_image(
        &self,
        id: &str,
        width: u16,
        height: u16,
    ) -> Result<PathBuf, error::CacheRetrievalError> {
        let path = self.cached_avatar_path(id, width, height)?;
        if !path.exists() {
            self.player_by_id(id).await?;
        }

        if let Some(_) = &self.dummy_local_user {
            return Ok(path);
        }

        if !path.exists() {
            return Err(error::CacheRetrievalError::Incapable(id.to_string()));
        }

        Ok(path)
    }

    pub fn cached_avatar_path(
        &self,
        id: &str,
        width: u16,
        height: u16,
    ) -> Result<PathBuf, NativeError> {
        let dir = maxima_dir()?.join("cache/avatars");
        create_dir_all(&dir)?;

        Ok(dir.join(format!("{}_{}x{}.jpg", id, width, height)))
    }

    pub fn library(&self) -> &GameLibrary {
        &self.library
    }

    pub fn mut_library(&mut self) -> &mut GameLibrary {
        &mut self.library
    }

    pub fn content_manager(&mut self) -> &mut ContentManager {
        &mut self.content_manager
    }

    pub fn rtm(&mut self) -> &mut RtmClient {
        &mut self.rtm
    }

    pub fn set_lsx_port(&mut self, port: u16) {
        self.lsx_port = port;
    }

    pub(super) fn set_lsx_connections(&mut self, connections: u16) {
        self.lsx_connections = connections;
    }

    pub fn set_player_started(&mut self) {
        match &mut self.playing {
            Some(ref mut playing) => playing.set_started(),
            None => return,
        }
    }

    /// Call this as often as possible from the loop you consume events from
    pub async fn update(&mut self) {
        self.update_playing_status().await;

        let result = self.content_manager.update().await;
        match result {
            Err(err) => warn!("Failed to update content manager: {}", err),
            Ok(result) => {
                if let Some(event) = result {
                    self.call_event(event);
                }
            }
        }
    }

    async fn update_playing_status(&mut self) {
        if self.lsx_connections > 0 || self.playing.is_none() {
            return;
        }

        let playing = self.playing.as_mut().unwrap();
        match playing.process_mut().try_wait() {
            Ok(None) => return,
            _ => (),
        }

        info!("Game stopped");

        if let Some(offer) = playing.offer() {
            if offer.offer().has_cloud_save() {
                let result = self
                    .cloud_sync
                    .obtain_lock(offer, CloudSyncLockMode::Write)
                    .await;
                match result {
                    Err(err) => error!("Failed to obtain CloudSync write lock: {}", err),
                    Ok(lock) => {
                        let result = lock.sync_files().await;
                        if let Err(err) = result {
                            error!("Failed to write to CloudSync: {}", err);
                        }

                        lock.release().await.ok();
                    }
                }
            }
        }

        // We need to store your BasicPresence somewhere
        self.rtm
            .set_presence(BasicPresence::Online, "", "")
            .await
            .ok();
        self.playing = None;
    }

    /// Returns whether this Maxima instance was constructed with a dummy
    /// user. This is usually paired with not loading/interacting with auth
    /// storage.
    pub fn dummy_local_user(&self) -> bool {
        self.dummy_local_user.is_some()
    }
}
