use super::{
    auth::storage::LockedAuthStorage,
    locale::Locale,
    manifest::{self, GameManifest, ManifestError, MANIFEST_RELATIVE_PATH},
    service_layer::{
        ServiceGameProductType, ServiceGetLegacyCatalogDefsRequestBuilder,
        ServiceGetPreloadedOwnedGamesRequest, ServiceGetPreloadedOwnedGamesRequestBuilder,
        ServiceGetPreloadedOwnedGamesRequestBuilderError, ServiceLayerClient, ServiceLayerError,
        ServiceLegacyOffer, ServicePlatform, ServiceStorefront, ServiceUser,
        ServiceUserGameProduct, SERVICE_REQUEST_GETLEGACYCATALOGDEFS,
        SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES,
    },
};
#[cfg(unix)]
use crate::unix::fs::case_insensitive_path;
use crate::util::native::{NativeError, SafeStr};
use crate::util::registry::{parse_partial_registry_path, parse_registry_path, RegistryError};
use derive_getters::Getters;
use std::{collections::HashMap, path::PathBuf, time::SystemTimeError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LibraryError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    ServiceGetPreloadedOwnedGamesRequestBuilderError(
        #[from] ServiceGetPreloadedOwnedGamesRequestBuilderError,
    ),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    ServiceLayer(#[from] ServiceLayerError),
    #[error(transparent)]
    Time(#[from] SystemTimeError),

    #[error("`{0}` has no manifest found")]
    NoManifest(String),
    #[error("`{0}` was not installed")]
    NotInstalled(String),
    #[error("`{0}`'s execute path was not found")]
    NoPath(String),
    #[error("`{0}`'s version info is unavailable")]
    NoVersion(String),
}

#[derive(Clone, Getters)]
pub struct OwnedOffer {
    slug: String,
    product: ServiceUserGameProduct,
    offer: ServiceLegacyOffer,
}

impl OwnedOffer {
    pub async fn is_installed(&self) -> bool {
        // I would love to throw an error here but that's just not feasible.
        // If you can't grab the path it may as well not be installed.
        let Some(path) = &self.offer.install_check_override().as_ref() else {
            return false;
        };
        let path = match parse_registry_path(path).await {
            Ok(path) => path,
            Err(_) => return false,
        };
        // If it wasn't replaced...
        if path.starts_with("[") {
            return false;
        }
        #[cfg(unix)]
        let path = case_insensitive_path(path);
        path.exists()
    }

    pub async fn install_check_path(&self) -> Result<String, ManifestError> {
        Ok(parse_registry_path(
            &self
                .offer
                .install_check_override()
                .as_ref()
                .ok_or(ManifestError::NoInstallPath(self.slug.clone()))?,
        )
        .await?
        .safe_str()?
        .to_owned())
    }

    pub async fn execute_path(&self, trial: bool) -> Result<PathBuf, LibraryError> {
        let manifest = match self.local_manifest().await? {
            Some(manifest) => manifest,
            None => return Err(LibraryError::NoManifest(self.slug.clone())),
        };

        let path = if let Some(path) = manifest.execute_path(trial) {
            &Some(path)
        } else {
            self.offer.execute_path_override()
        };

        if let Some(path) = path {
            Ok(parse_registry_path(path).await?)
        } else {
            Err(LibraryError::NoPath(self.slug.clone()))
        }
    }

    pub async fn installed_version(&self) -> Result<String, LibraryError> {
        if !self.is_installed().await {
            return Err(LibraryError::NotInstalled(self.slug.clone()));
        }

        let manifest = match self.local_manifest().await? {
            Some(manifest) => manifest,
            None => return Err(LibraryError::NoManifest(self.slug.clone())),
        };

        if let Some(version) = manifest.version() {
            Ok(version)
        } else {
            Err(LibraryError::NoVersion(self.slug.clone()))
        }
    }

    pub async fn local_manifest(&self) -> Result<Option<Box<dyn GameManifest>>, ManifestError> {
        let path = if self
            .offer
            .install_check_override()
            .as_ref()
            .ok_or(ManifestError::NoInstallPath(self.slug.clone()))?
            .contains("installerdata.xml")
        {
            let ic_path = PathBuf::from(self.install_check_path().await?);
            #[cfg(unix)]
            let ic_path = case_insensitive_path(ic_path);
            ic_path
        } else {
            let path = PathBuf::from(
                parse_partial_registry_path(
                    &self
                        .offer
                        .install_check_override()
                        .as_ref()
                        .ok_or(ManifestError::NoInstallPath(self.slug.clone()))?,
                )
                .await?
                .safe_str()?
                .to_owned(),
            );

            path.join(MANIFEST_RELATIVE_PATH)
        };

        Ok(Some(manifest::read(path).await?))
    }

    pub fn offer_id(&self) -> &String {
        self.offer.offer_id()
    }
}

#[derive(Clone, Getters)]
pub struct OwnedTitle {
    base_offer: OwnedOffer,
    offers: Vec<OwnedOffer>,
}

fn group_offers(products: Vec<OwnedOffer>) -> Vec<OwnedTitle> {
    let mut base_products = HashMap::new();
    let mut product_map = HashMap::new();

    for product in products {
        let slug = product
            .product
            .product()
            .base_item()
            .base_game_slug()
            .clone();

        let full_game = (|| {
            // Ensure it's the full game
            if product.offer.display_type() != "FullGame"
                || product
                    .product()
                    .product()
                    .base_item()
                    .game_type()
                    .as_ref()
                    .unwrap_or(&ServiceGameProductType::ExpansionPack)
                    != &ServiceGameProductType::BaseGame
            {
                return false;
            }

            if !product.offer.is_downloadable() {
                return false;
            }

            // Ensure it isn't a trial
            if product
                .product()
                .product()
                .game_product_user()
                .game_product_user_trial()
                .is_some()
            {
                return false;
            }

            true
        })();

        if full_game {
            match slug {
                Some(slug) => {
                    product_map
                        .entry(slug.clone())
                        .or_insert_with(Vec::new)
                        .push(product);
                }
                None => {
                    base_products.insert(product.slug.clone(), product.clone());
                }
            }
        }
    }

    let mut titles = Vec::new();

    for (base_slug, base_offer) in base_products {
        let associated_products = product_map.remove(&base_slug).unwrap_or_default();
        titles.push(OwnedTitle {
            base_offer,
            offers: associated_products,
        });
    }

    titles
}

impl OwnedTitle {
    pub fn new(base_offer: OwnedOffer, offers: Vec<OwnedOffer>) -> Self {
        Self { base_offer, offers }
    }

    pub fn name(&self) -> String {
        match self
            .base_offer
            .product
            .product()
            .base_item()
            .title()
            .as_ref()
        {
            Some(title) => title.replace("\n", "").to_owned(),
            None => "Unknown".to_owned(),
        }
    }

    pub fn base_game(&self) -> Option<OwnedOffer> {
        for offer in self.offers.iter() {
            if offer
                .product
                .product()
                .base_item()
                .game_type()
                .as_ref()
                .unwrap_or(&ServiceGameProductType::ExpansionPack)
                != &ServiceGameProductType::BaseGame
            {
                return None;
            }

            if !*offer.product.product().downloadable() {
                return None;
            }

            return Some(offer.clone());
        }

        None
    }

    pub fn offer(&self, slug: &str) -> Option<&OwnedOffer> {
        self.offers.iter().find(|x| x.slug == slug)
    }

    pub fn extra_offers(&self) -> &Vec<OwnedOffer> {
        &self.offers
    }
}

pub struct GameLibrary {
    service_layer: ServiceLayerClient,
    library: Vec<OwnedTitle>,
    last_request: u64,
}

impl GameLibrary {
    pub async fn new(auth: LockedAuthStorage) -> Self {
        Self {
            service_layer: ServiceLayerClient::new(auth),
            library: Vec::new(),
            last_request: 0,
        }
    }

    pub async fn games(&mut self) -> Result<&Vec<OwnedTitle>, LibraryError> {
        self.update_if_needed().await?;
        Ok(&self.library)
    }

    pub async fn title_by_base_offer(
        &mut self,
        offer_id: &str,
    ) -> Result<Option<&OwnedTitle>, LibraryError> {
        self.update_if_needed().await?;
        Ok(self
            .library
            .iter()
            .find(|x| x.base_offer.offer.offer_id() == offer_id))
    }

    pub async fn game_by_base_offer(
        &mut self,
        offer_id: &str,
    ) -> Result<Option<&OwnedOffer>, LibraryError> {
        self.update_if_needed().await?;
        Ok(self
            .library
            .iter()
            .find(|x| x.base_offer.offer.offer_id() == offer_id)
            .map(|x| &x.base_offer))
    }

    pub async fn game_by_base_slug(
        &mut self,
        slug: &str,
    ) -> Result<Option<&OwnedOffer>, LibraryError> {
        self.update_if_needed().await?;
        Ok(self
            .library
            .iter()
            .find(|x| x.base_offer.product.product().game_slug() == slug)
            .map(|x| &x.base_offer))
    }

    async fn update_if_needed(&mut self) -> Result<(), LibraryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        if now - self.last_request > 1200 {
            self.request_owned_games().await?;
        }

        Ok(())
    }

    async fn request_owned_games(&mut self) -> Result<(), LibraryError> {
        self.request_page_concurrent(Locale::EnUs, 1).await?;

        Ok(())
    }

    async fn request_page_concurrent(
        &mut self,
        locale: Locale,
        page: u32,
    ) -> Result<(), LibraryError> {
        let responses: Vec<ServiceUserGameProduct> = {
            let request = GameLibrary::library_request(
                &locale,
                ServiceGameProductType::DigitalFullGame,
                true,
                page,
            )?;

            let user: ServiceUser = self
                .service_layer
                .request(SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES, request)
                .await?;
            user.owned_game_products()
                .as_ref()
                .ok_or(ServiceLayerError::MissingField)?
                .items()
                .clone()
        };

        let offer_ids = responses
            .iter()
            .map(|x| x.origin_offer_id().to_owned())
            .collect();

        let defs: Vec<ServiceLegacyOffer> = self
            .service_layer
            .request(
                SERVICE_REQUEST_GETLEGACYCATALOGDEFS,
                ServiceGetLegacyCatalogDefsRequestBuilder::default()
                    .offer_ids(offer_ids)
                    .locale(locale)
                    .build()
                    .unwrap(),
            )
            .await?;

        let mut offers: Vec<OwnedOffer> = Vec::new();
        for product in responses {
            let def = match defs
                .iter()
                .find(|x| x.offer_id() == product.origin_offer_id())
            {
                None => {
                    continue;
                }
                Some(def) => def.clone(),
            };
            offers.push(OwnedOffer {
                slug: product.product().game_slug().to_owned(),
                product: product.clone(),
                offer: def,
            });
        }

        let mut titles = group_offers(offers);
        titles.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        self.library = titles;
        self.last_request = now;
        Ok(())
    }

    fn library_request(
        locale: &Locale,
        r#type: ServiceGameProductType,
        entitlement_enabled: bool,
        page: u32,
    ) -> Result<ServiceGetPreloadedOwnedGamesRequest, LibraryError> {
        Ok(ServiceGetPreloadedOwnedGamesRequestBuilder::default()
            .is_mac(false)
            .locale(locale.to_owned())
            .limit(1000)
            .next(((page - 1) * 1000).to_string())
            .r#type(r#type)
            .entitlement_enabled(None)
            .storefronts(vec![
                ServiceStorefront::Ea,
                ServiceStorefront::Steam,
                ServiceStorefront::Epic,
            ])
            .platforms(vec![ServicePlatform::Pc])
            .build()?)
    }
}
