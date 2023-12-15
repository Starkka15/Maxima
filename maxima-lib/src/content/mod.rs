use std::{sync::Arc, time::Duration};

use anyhow::Result;
use tokio::sync::Mutex;

use crate::core::{
    cache::DynamicCache,
    service_layer::{
        send_service_request, ServiceAvailableBuild, ServiceAvailableBuildsRequest,
        ServiceAvailableBuildsRequestBuilder, ServiceDownloadType, ServiceDownloadUrl,
        ServiceDownloadUrlRequest, ServiceDownloadUrlRequestBuilder,
        SERVICE_REQUEST_AVAILABLEBUILDS, SERVICE_REQUEST_DOWNLOADURL,
    },
    Maxima,
};

pub mod downloader;
pub mod patcher;
pub mod zip;

#[derive(Clone)]
pub struct ServiceAvailableBuilds {
    builds: Vec<ServiceAvailableBuild>,
}

impl ServiceAvailableBuilds {
    pub fn live_build(&self) -> Option<&ServiceAvailableBuild> {
        self.builds.iter().find(|b| {
            b.download_type()
                .as_ref()
                .unwrap_or(&ServiceDownloadType::None)
                == &ServiceDownloadType::Live
        })
    }
}

pub struct ContentService {
    maxima: Arc<Mutex<Maxima>>,
    request_cache: DynamicCache<String>,
}

impl ContentService {
    pub fn new(maxima: Arc<Mutex<Maxima>>) -> Self {
        let request_cache = DynamicCache::new(
            100,
            Duration::from_secs(30 * 60),
            Duration::from_secs(5 * 60),
        );

        Self {
            maxima,
            request_cache,
        }
    }

    pub async fn available_builds(&self, offer_id: &str) -> Result<ServiceAvailableBuilds> {
        let cache_key = "builds_".to_owned() + offer_id;
        if let Some(cached) = self.request_cache.get(&cache_key) {
            return Ok(cached);
        }

        let builds: Vec<ServiceAvailableBuild> = send_service_request(
            &self.maxima.lock().await.access_token(),
            SERVICE_REQUEST_AVAILABLEBUILDS,
            ServiceAvailableBuildsRequestBuilder::default()
                .offer_id(offer_id.to_owned())
                .build()?,
        )
        .await?;

        let builds = ServiceAvailableBuilds { builds };
        self.request_cache.insert(cache_key, builds.clone());
        Ok(builds)
    }

    pub async fn download_url(
        &self,
        offer_id: &str,
        build_id: Option<&str>,
    ) -> Result<ServiceDownloadUrl> {
        let cache_key = "download_url_".to_owned() + offer_id + "_" + build_id.unwrap_or("live");
        if let Some(cached) = self.request_cache.get(&cache_key) {
            return Ok(cached);
        }

        let url: ServiceDownloadUrl = send_service_request(
            &self.maxima.lock().await.access_token(),
            SERVICE_REQUEST_DOWNLOADURL,
            ServiceDownloadUrlRequestBuilder::default()
                .offer_id(offer_id.to_owned())
                .build_id(build_id.unwrap_or_default().to_owned())
                .build()?,
        )
        .await?;

        self.request_cache.insert(cache_key, url.clone());
        Ok(url)
    }
}
