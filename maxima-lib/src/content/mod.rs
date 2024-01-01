use std::time::Duration;

use anyhow::Result;

use crate::core::{
    auth::storage::LockedAuthStorage,
    cache::DynamicCache,
    service_layer::{
        ServiceAvailableBuild, ServiceAvailableBuildsRequestBuilder, ServiceDownloadType,
        ServiceDownloadUrlMetadata, ServiceDownloadUrlRequestBuilder, ServiceLayerClient,
        SERVICE_REQUEST_AVAILABLEBUILDS, SERVICE_REQUEST_DOWNLOADURL,
    },
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
    service_layer: ServiceLayerClient,
    request_cache: DynamicCache<String>,
}

impl ContentService {
    pub fn new(auth: LockedAuthStorage) -> Self {
        let request_cache = DynamicCache::new(
            100,
            Duration::from_secs(30 * 60),
            Duration::from_secs(5 * 60),
        );

        Self {
            service_layer: ServiceLayerClient::new(auth),
            request_cache,
        }
    }

    pub async fn available_builds(&self, offer_id: &str) -> Result<ServiceAvailableBuilds> {
        let cache_key = "builds_".to_owned() + offer_id;
        if let Some(cached) = self.request_cache.get(&cache_key) {
            return Ok(cached);
        }

        let builds: Vec<ServiceAvailableBuild> = self
            .service_layer
            .request(
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
    ) -> Result<ServiceDownloadUrlMetadata> {
        let cache_key = "download_url_".to_owned() + offer_id + "_" + build_id.unwrap_or("live");
        if let Some(cached) = self.request_cache.get(&cache_key) {
            return Ok(cached);
        }

        let url: ServiceDownloadUrlMetadata = self
            .service_layer
            .request(
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
