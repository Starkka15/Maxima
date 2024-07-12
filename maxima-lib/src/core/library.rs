use std::{collections::HashMap, path::PathBuf};

use anyhow::{bail, Result};
use derive_getters::Getters;
use tokio::{fs::File, io::BufReader};

use crate::util::registry::parse_registry_path;

use super::{
    auth::storage::LockedAuthStorage,
    dip::DiPManifest,
    locale::Locale,
    service_layer::{
        ServiceGameProductType, ServiceGetLegacyCatalogDefsRequestBuilder,
        ServiceGetPreloadedOwnedGamesRequest, ServiceGetPreloadedOwnedGamesRequestBuilder,
        ServiceLayerClient, ServiceLegacyOffer, ServicePlatform, ServiceStorefront, ServiceUser,
        ServiceUserGameProduct, SERVICE_REQUEST_GETLEGACYCATALOGDEFS,
        SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES,
    },
};

#[derive(Clone, Getters)]
pub struct OwnedOffer {
    slug: String,
    product: ServiceUserGameProduct,
    offer: ServiceLegacyOffer,
}

impl OwnedOffer {
    pub fn installed(&self) -> bool {
        let path = parse_registry_path(&self.offer.install_check_override());
        path.exists()
    }

    pub fn install_check_path(&self) -> String {
        parse_registry_path(&self.offer.install_check_override())
            .to_str()
            .unwrap()
            .to_owned()
    }

    pub async fn execute_path(&self, trial: bool) -> Result<PathBuf> {
        let manifest = self.local_manifest().await;
        if manifest.is_none() {
            bail!("No DiP manifest found for {}", self.slug);
        }

        let path = if let Some(path) = manifest.unwrap().file_path(trial) {
            path
        } else if !self.offer.execute_path_override().is_empty() {
            parse_registry_path(&self.offer.execute_path_override())
                .to_str()
                .unwrap()
                .to_owned()
        } else {
            bail!("No execute path found");
        };

        Ok(parse_registry_path(&path))
    }

    pub async fn local_manifest(&self) -> Option<DiPManifest> {
        let path = if self
            .offer
            .install_check_override()
            .contains("installerdata.xml")
        {
            self.install_check_path()
        } else {
            String::new()
        };

        if path.is_empty() {
            return None;
        }

        let file = tokio::fs::read_to_string(path).await.ok()?;
        let manifest: DiPManifest = quick_xml::de::from_str(&file).unwrap();
        Some(manifest)
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
        if let Some(base_slug) = &product.product.product().base_item().base_game_slug() {
            product_map
                .entry(base_slug.clone())
                .or_insert_with(Vec::new)
                .push(product);
        } else {
            base_products.insert(product.slug.clone(), product);
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
        self.base_offer
            .product
            .product()
            .base_item()
            .title()
            .as_ref()
            .unwrap()
            .to_owned()
            .replace("\n", "")
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
        let mut lib = Self {
            service_layer: ServiceLayerClient::new(auth),
            library: Vec::new(),
            last_request: 0,
        };
        //lib.update_if_needed().await;
        lib
    }

    pub async fn games(&mut self) -> &Vec<OwnedTitle> {
        self.update_if_needed().await;
        &self.library
    }

    pub async fn game_by_base_offer(&mut self, offer_id: &str) -> Option<&OwnedOffer> {
        self.update_if_needed().await;
        self.library
            .iter()
            .find(|x| x.base_offer.offer.offer_id() == offer_id)
            .map(|x| &x.base_offer)
    }

    async fn update_if_needed(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now - self.last_request > 1200 {
            self.request_owned_games().await;
        }
    }

    pub async fn request_owned_games(&mut self) {
        self.request_page_concurrent(Locale::EnUs, 1).await.unwrap();
    }

    async fn request_page_concurrent(&mut self, locale: Locale, page: u32) -> Result<()> {
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
                .await
                .unwrap();
            user.owned_game_products().as_ref().unwrap().items().clone()
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
            .await
            .unwrap();

        let mut offers: Vec<OwnedOffer> = Vec::new();
        for product in responses {
            offers.push(OwnedOffer {
                slug: product.product().game_slug().to_owned(),
                product: product.clone(),
                offer: defs
                    .iter()
                    .find(|x| x.offer_id() == product.origin_offer_id())
                    .unwrap()
                    .clone(),
            });
        }

        let titles = group_offers(offers);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
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
    ) -> Result<ServiceGetPreloadedOwnedGamesRequest> {
        Ok(ServiceGetPreloadedOwnedGamesRequestBuilder::default()
            .is_mac(false)
            .locale(locale.to_owned())
            .limit(100)
            .next(((page - 1) * 100).to_string())
            .r#type(r#type)
            .entitlement_enabled(Some(entitlement_enabled))
            .storefronts(vec![
                ServiceStorefront::Ea,
                ServiceStorefront::Steam,
                ServiceStorefront::Epic,
            ])
            .platforms(vec![ServicePlatform::Pc])
            .build()?)
    }
}
