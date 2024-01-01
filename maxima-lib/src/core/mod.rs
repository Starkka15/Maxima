pub mod auth;
pub mod cache;
pub mod clients;
pub mod ecommerce;
pub mod endpoints;
pub mod launch;
pub mod locale;
pub mod service_layer;
pub mod settings;

#[cfg(target_os = "windows")]
pub mod background_service {
    include!("background_service_win.rs");
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
use derive_getters::Getters;
use log::error;
use strum_macros::IntoStaticStr;
use sysinfo::{Pid, PidExt, ProcessExt, ProcessStatus, System, SystemExt};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    lsx::{self, types::LSXRequestType},
    util::native::maxima_dir,
};

use self::{
    cache::DynamicCache,
    ecommerce::CommerceOffer,
    launch::ActiveGameContext,
    locale::Locale,
    service_layer::{
        ServiceGameType, ServiceGetBasicPlayerRequestBuilder,
        ServiceGetPreloadedOwnedGamesRequestBuilder, ServiceGetUserPlayerRequest, ServiceImage,
        ServicePlatform, ServicePlayer, ServiceStorefront, ServiceUser,
        SERVICE_REQUEST_GETBASICPLAYER, SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES,
        SERVICE_REQUEST_GETUSERPLAYER, ServiceLayerClient,
    }, auth::storage::{AuthStorage, LockedAuthStorage},
};

#[derive(Clone, IntoStaticStr)]
pub enum MaximaEvent {
    /// PID, Request Type
    ReceivedLSXRequest(u32, LSXRequestType),
    /// To fix erroneous warning in maxima-native, remove once there are more events
    Unknown,
}

pub type MaximaLSXEventCallback = extern "C" fn(*const c_char);

#[derive(Getters)]
pub struct Maxima {
    locale: Locale,

    auth_storage: LockedAuthStorage,
    service_layer: ServiceLayerClient,

    playing: Option<ActiveGameContext>,

    lsx_port: u16,
    lsx_event_callback: Option<MaximaLSXEventCallback>,
    lsx_connections: u16,

    #[getter(skip)]
    request_cache: DynamicCache<String>,

    #[getter(skip)]
    pending_events: Vec<MaximaEvent>,
}

pub type LockedMaxima = Arc<Mutex<Maxima>>;

impl Maxima {
    pub fn new() -> Result<Arc<Mutex<Self>>> {
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

        let auth_storage = AuthStorage::load()?;

        Ok(Arc::new(Mutex::new(Self {
            locale: Locale::EnUs,
            auth_storage: auth_storage.clone(),
            service_layer: ServiceLayerClient::new(auth_storage),
            playing: None,
            lsx_port,
            lsx_event_callback: None,
            lsx_connections: 0,
            request_cache,
            pending_events: Vec::new(),
        })))
    }

    pub async fn start_lsx(&self, maxima: Arc<Mutex<Maxima>>) -> Result<()> {
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
        let account = auth_storage.current();
        if account.is_none() {
            bail!("You are not signed in");
        }

        Ok(account.unwrap().access_token().await?.to_owned())
    }

    pub async fn local_user(&self) -> Result<ServiceUser> {
        let cache_key = "user_player";
        if let Some(cached) = self.request_cache.get(cache_key) {
            return Ok(cached);
        }

        let user: ServiceUser = self.service_layer.request(
            SERVICE_REQUEST_GETUSERPLAYER,
            ServiceGetUserPlayerRequest {},
        )
        .await?;

        self.request_cache
            .insert(cache_key.to_owned(), user.clone());
        Ok(user)
    }

    pub fn call_event(&mut self, event: MaximaEvent) {
        self.pending_events.push(event);
    }

    pub fn consume_pending_events(&mut self) -> Vec<MaximaEvent> {
        let events = self.pending_events.clone();
        self.pending_events.clear();
        events
    }

    pub async fn owned_games(&self, page: u32) -> Result<ServiceUser> {
        let data: ServiceUser = self.service_layer.request(
            SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES,
            ServiceGetPreloadedOwnedGamesRequestBuilder::default()
                .is_mac(false)
                .locale(self.locale.to_owned())
                .limit(1000)
                .next(((page - 1) * 1000).to_string())
                .r#type(ServiceGameType::DigitalFullGame)
                .entitlement_enabled(None)
                .storefronts(vec![
                    ServiceStorefront::Ea,
                    ServiceStorefront::Steam,
                    ServiceStorefront::Epic,
                ])
                .platforms(vec![ServicePlatform::Pc])
                .build()?,
        )
        .await?;

        Ok(data)
    }

    pub async fn current_offer(&self) -> Option<CommerceOffer> {
        if self.playing.is_none() {
            return None;
        }

        Some(self.playing.as_ref().unwrap().offer().to_owned())
    }

    pub async fn player_by_id(&self, id: &str) -> Result<ServicePlayer> {
        let cache_key = "basic_player_".to_owned() + id;
        if let Some(cached) = self.request_cache.get(&cache_key) {
            return Ok(cached);
        }

        let data: ServicePlayer = self.service_layer.request(
            SERVICE_REQUEST_GETBASICPLAYER,
            ServiceGetBasicPlayerRequestBuilder::default()
                .pd(id.to_string())
                .build()?,
        )
        .await?;

        let avatars = data.avatar();
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

    pub fn update_playing_status(&mut self) {
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

        self.playing = None;
    }
}
