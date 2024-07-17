pub mod auth;
pub mod cache;
pub mod clients;
pub mod cloudsync;
pub mod concurrency;
pub mod dip;
pub mod ecommerce;
pub mod endpoints;
pub mod launch;
pub mod library;
pub mod locale;
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
    os::raw::c_char,
    path::PathBuf,
    time::Duration,
    {io, io::Read},
};

use anyhow::{bail, Result};
use cloudsync::{CloudSyncClient, CloudSyncLockMode};
use derive_builder::Builder;
use derive_getters::Getters;
use log::{error, info, warn};
use strum_macros::IntoStaticStr;
use sysinfo::{Pid, PidExt, ProcessExt, ProcessStatus, System, SystemExt};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    content::manager::ContentManager,
    lsx::{self, types::LSXRequestType},
    rtm::client::{BasicPresence, RtmClient},
    util::native::maxima_dir,
};

use self::{
    auth::storage::{AuthStorage, LockedAuthStorage},
    cache::DynamicCache,
    launch::ActiveGameContext,
    library::GameLibrary,
    locale::Locale,
    service_layer::{
        ServiceAvatarListBuilder, ServiceFriends, ServiceGetBasicPlayerRequestBuilder,
        ServiceGetMyFriendsRequestBuilder, ServiceGetUserPlayerRequest, ServiceImage,
        ServiceImageBuilder, ServiceLayerClient, ServicePlayer, ServicePlayerBuilder, ServiceUser,
        ServiceUserBuilder, SERVICE_REQUEST_GETBASICPLAYER, SERVICE_REQUEST_GETMYFRIENDS,
        SERVICE_REQUEST_GETUSERPLAYER,
    },
};

mod error;

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

pub type LockedMaxima = Arc<Mutex<Maxima>>;

impl Maxima {
    pub async fn new_with_options(options: MaximaOptions) -> Result<LockedMaxima> {
        let lsx_port = if let Ok(lsx_port) = env::var("MAXIMA_LSX_PORT") {
            lsx_port.parse::<u16>().unwrap()
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

    pub async fn new() -> Result<LockedMaxima> {
        Maxima::new_with_options(
            MaximaOptionsBuilder::default()
                .load_auth_storage(true)
                .dummy_local_user(false)
                .build()?,
        )
        .await
    }

    pub async fn start_lsx(&self, maxima: LockedMaxima) -> Result<()> {
        let lsx_port = self.lsx_port;

        tokio::spawn(async move {
            if let Err(e) = lsx::service::start_server(lsx_port, maxima).await {
                error!("Error starting LSX server: {}", e);
            }
        });

        tokio::task::yield_now().await;
        Ok(())
    }

    pub async fn access_token(&mut self) -> Result<String> {
        let mut auth_storage = self.auth_storage.lock().await;
        let access_token = auth_storage.access_token().await?;
        if access_token.is_none() {
            bail!("You are not signed in");
        }

        Ok(access_token.unwrap())
    }

    pub async fn local_user(&self) -> Result<ServiceUser> {
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

    pub async fn friends(&self, page: u32) -> Result<Vec<ServicePlayer>> {
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
                    .build()?,
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

    pub async fn player_by_id(&self, id: &str) -> Result<ServicePlayer> {
        if let Some(user) = &self.dummy_local_user {
            return Ok(user.player().as_ref().unwrap().clone());
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
                    .build()?,
            )
            .await?;

        let avatars = data.avatar();
        if avatars.is_none() {
            bail!("No avatars found");
        }

        let avatars = avatars.as_ref().unwrap();
        self.cache_avatar_image(&id, avatars.large()).await?;
        self.cache_avatar_image(&id, avatars.medium()).await?;
        self.cache_avatar_image(&id, avatars.small()).await?;

        self.request_cache.insert(cache_key, data.clone());
        Ok(data)
    }

    async fn cache_avatar_image(&self, id: &str, image: &ServiceImage) -> Result<()> {
        let path = self.cached_avatar_path(
            id,
            image.width().unwrap_or(727),
            image.height().unwrap_or(727),
        )?;

        if path.exists() {
            return Ok(());
        }

        let response = ureq::get(&image.path()).call()?;
        let mut body: Vec<u8> = vec![];
        response
            .into_reader()
            .take((1024 + 1) as u64)
            .read_to_end(&mut body)?;

        let mut file = File::create(path)?;
        io::copy(&mut body.as_slice(), &mut file)?;

        Ok(())
    }

    pub async fn avatar_image(&self, id: &str, width: u16, height: u16) -> Result<PathBuf> {
        let path = self.cached_avatar_path(id, width, height)?;
        if !path.exists() {
            self.player_by_id(id).await?;
        }

        if let Some(_) = &self.dummy_local_user {
            return Ok(path);
        }

        if !path.exists() {
            bail!("Failed to cache avatar images for {}", id);
        }

        Ok(path)
    }

    pub fn cached_avatar_path(&self, id: &str, width: u16, height: u16) -> Result<PathBuf> {
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
        if self.playing.is_none() {
            return;
        }

        self.playing.as_mut().unwrap().set_started();
    }

    /// Call this as often as possible from the loop you consume events from
    pub async fn update(&mut self) {
        self.update_playing_status().await;

        let result = self.content_manager.update().await;
        if let Err(err) = result {
            warn!("Failed to update content manager: {}", err);
        } else {
            let result = result.unwrap();
            if let Some(event) = result {
                self.call_event(event);
            }
        }
    }

    async fn update_playing_status(&mut self) {
        if self.lsx_connections > 0
            || self.playing.is_none()
            || !self.playing.as_ref().unwrap().started()
        {
            return;
        }

        let playing = self.playing.as_ref().unwrap();
        if let Some(pid) = playing.process().id() {
            let sys = System::new_all();
            let proc = sys.process(Pid::from_u32(pid));

            if let Some(proc) = proc {
                if proc.status() == ProcessStatus::Run {
                    return;
                }
            }
        }

        info!("Game stopped");

        if let Some(offer) = playing.offer() {
            let result = self
                .cloud_sync
                .obtain_lock(offer, CloudSyncLockMode::Write)
                .await;
            if let Err(err) = result {
                error!("Failed to obtain CloudSync write lock: {}", err);
            } else {
                let lock = result.unwrap();
                let result = lock.sync_files().await;
                if let Err(err) = result {
                    error!("Failed to write to CloudSync: {}", err);
                }

                lock.release().await.ok();
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
