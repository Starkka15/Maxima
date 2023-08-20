pub mod auth;
pub mod background_service;
pub mod ecommerce;
pub mod endpoints;
pub mod launch;
pub mod locale;
pub mod service_layer;
pub mod settings;

use std::{
    env,
    fs::{create_dir_all, File},
    {io, io::Read},
    path::PathBuf,
};

use anyhow::{bail, Result};
use directories::ProjectDirs;
use log::error;

use core::slice::SlicePattern;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::lsx::{self, types::LSXRequestType};

use self::{
    launch::ActiveGameContext,
    locale::Locale,
    service_layer::{
        send_service_request, ServiceGameType, ServiceGetPreloadedOwnedGamesRequest,
        ServiceGetUserPlayerRequest, ServiceImage, ServicePlatform, ServicePlayer,
        ServicePlayerByPlayerIdRequest, ServiceStorefront, ServiceUser,
        SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES, SERVICE_REQUEST_GETUSERPLAYER,
        SERVICE_REQUEST_PLAYERBYPD,
    },
};

#[derive(Clone)]
pub enum MaximaEvent {
    ReceivedLSXRequest(LSXRequestType),
}

pub struct Maxima {
    pub locale: Locale,
    pub lsx_port: u16,
    pub access_token: String,
    pub playing: Option<ActiveGameContext>,
    pub project_dir: ProjectDirs,
    pending_events: Vec<MaximaEvent>,
}

impl Maxima {
    pub fn new() -> Self {
        let dirs = ProjectDirs::from("com", "Maxima", "Maxima");

        let lsx_port = if let Ok(lsx_port) = env::var("MAXIMA_LSX_PORT") {
            lsx_port.parse::<u16>().unwrap()
        } else {
            3216
        };

        Self {
            locale: Locale::EnUs,
            lsx_port,
            access_token: String::new(),
            playing: None,
            project_dir: dirs.expect("Failed to create Maxima config directories!"),
            pending_events: Vec::new(),
        }
    }

    pub async fn start_lsx(&self, arc: Arc<Mutex<Maxima>>) -> Result<()> {
        let lsx_port = self.lsx_port;
        let _server_handle = tokio::spawn(async move {
            if let Err(e) = lsx::service::start_server(lsx_port, arc).await {
                error!("Error starting LSX server: {}", e);
            }
        });

        Ok(())
    }

    pub async fn get_local_user(&self) -> Result<ServiceUser> {
        let user: ServiceUser = send_service_request(
            &self.access_token,
            SERVICE_REQUEST_GETUSERPLAYER,
            ServiceGetUserPlayerRequest {},
        )
        .await?;

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

    pub async fn get_owned_games(&self, page: u32) -> Result<ServiceUser> {
        let data: ServiceUser = send_service_request(
            self.access_token.as_str(),
            SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES,
            ServiceGetPreloadedOwnedGamesRequest {
                is_mac: false,
                locale: self.locale.to_owned(),
                limit: 1000,
                next: ((page - 1) * 100).to_string(),
                r#type: ServiceGameType::DigitalFullGame,
                entitlement_enabled: None,
                storefronts: vec![
                    ServiceStorefront::Ea,
                    ServiceStorefront::Steam,
                    ServiceStorefront::Epic,
                ],
                platforms: vec![ServicePlatform::Pc],
            },
        )
        .await?;

        Ok(data)
    }

    pub async fn get_player_by_id(&self, id: &str) -> Result<ServicePlayer> {
        let data: ServicePlayer = send_service_request(
            self.access_token.as_str(),
            SERVICE_REQUEST_PLAYERBYPD,
            ServicePlayerByPlayerIdRequest { pd: id.to_string() },
        )
        .await?;

        self.cache_avatar_image(&id, &data.avatar.large).await?;
        self.cache_avatar_image(&id, &data.avatar.medium).await?;
        self.cache_avatar_image(&id, &data.avatar.small).await?;

        Ok(data)
    }

    async fn cache_avatar_image(&self, id: &str, image: &ServiceImage) -> Result<()> {
        let path = self.get_cached_avatar_path(
            id,
            image.width.unwrap_or(727),
            image.height.unwrap_or(727),
        )?;
        if path.exists() {
            return Ok(());
        }

        let response = ureq::get(&image.path).call()?;
        let mut body: Vec<u8> = vec![];
        response.into_reader()
            .take((1024 + 1) as u64)
            .read_to_end(&mut body)?;

        let mut file = File::create(path)?;
        io::copy(&mut body.as_slice(), &mut file)?;

        Ok(())
    }

    pub async fn get_avatar_image(&self, id: &str, width: u16, height: u16) -> Result<PathBuf> {
        let path = self.get_cached_avatar_path(id, width, height)?;
        if !path.exists() {
            self.get_player_by_id(id).await?;
        }

        if !path.exists() {
            bail!("Failed to cache avatar images for {}", id);
        }

        Ok(path)
    }

    pub fn get_cached_avatar_path(&self, id: &str, width: u16, height: u16) -> Result<PathBuf> {
        let dir = self.project_dir.cache_dir().join("Avatars");
        create_dir_all(&dir)?;

        Ok(dir.join(format!("{}_{}x{}.jpg", id, width, height)))
    }
}
