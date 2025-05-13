#![allow(non_snake_case)]

use anyhow::{bail, Result};

use log::debug;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2_const::Sha256;

use derive_builder::Builder;
use derive_getters::Getters;

use crate::core::endpoints::API_CONTENTFUL_PROXY;

use super::{
    auth::storage::LockedAuthStorage, endpoints::API_SERVICE_AGGREGATION_LAYER, locale::Locale,
};

const LARGE_AVATAR_PATH: &str =
    "https://eaavatarservice.akamaized.net/production/avatar/prod/1/599/416x416.JPEG";
const MEDIUM_AVATAR_PATH: &str =
    "https://eaavatarservice.akamaized.net/production/avatar/prod/1/599/208x208.JPEG";
const SMALL_AVATAR_PATH: &str =
    "https://eaavatarservice.akamaized.net/production/avatar/prod/1/599/40x40.JPEG";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedQuery {
    version: u8,
    sha256_hash: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceExtensions {
    persisted_query: PersistedQuery,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FullServiceRequest<'a, T: Serialize> {
    extensions: ServiceExtensions,
    variables: T,
    operation_name: &'a str,
    query: &'static str,
}

pub enum ServiceLayerRequestType {
    ServiceAggregationLayer,
    ContentfulProxy,
}

pub struct ServiceLayerGraphQLRequest {
    query: &'static str,
    operation: &'static str,
    key: &'static str,
    hash: [u8; 32],
    r#type: ServiceLayerRequestType,
}

macro_rules! load_graphql_request {
    ($type:ident, $operation:expr, $key:expr) => {{
        let content = include_str!(concat!("graphql/", $operation, ".gql"));
        let hash = Sha256::new().update(content.as_bytes()).finalize();
        ServiceLayerGraphQLRequest {
            query: content,
            operation: $operation,
            key: $key,
            hash,
            r#type: ServiceLayerRequestType::$type,
        }
    }};
}

macro_rules! define_graphql_request {
    ($type:ident, $operation:expr, $key:expr) => { paste::paste! {
        pub const [<SERVICE_REQUEST_ $operation:upper>]: &ServiceLayerGraphQLRequest = &load_graphql_request!($type, stringify!($operation), stringify!($key));
    }}
}

define_graphql_request!(ServiceAggregationLayer, addonSearch, me); // Input: ServiceAddonSearchRequest, Output: AddonSearchResult
define_graphql_request!(ServiceAggregationLayer, availableBuilds, availableBuilds); // Input: ServiceAvailableBuildsRequest, Output: ServiceAvailableBuild[]
define_graphql_request!(ServiceAggregationLayer, downloadUrl, downloadUrl); // Input: ServiceDownloadUrlRequest, Output: ServiceDownloadUrlMetadata
define_graphql_request!(ServiceAggregationLayer, GameImages, game); // Input: ServiceGameImagesRequest, Output: ServiceGame
define_graphql_request!(ServiceAggregationLayer, GetBasicPlayer, playerByPd); // Input: ServiceGetBasicPlayerRequest, Output: ServicePlayer
define_graphql_request!(ServiceAggregationLayer, getPreloadedOwnedGames, me); // Input: ServiceGetPreloadedOwnedGamesRequest, Output: ServiceUser (with owned_game_products field set)
define_graphql_request!(ServiceAggregationLayer, GetUserPlayer, me); // Input: ServiceGetUserPlayerRequest, Output: ServiceUser
define_graphql_request!(ServiceAggregationLayer, GameSystemRequirements, game); // Input: ServiceGameSystemRequirementsRequest, Output: ServiceGameSystemRequirements
define_graphql_request!(ServiceAggregationLayer, GetMyFriends, me); // Input: ServiceGetMyFriendsRequest, Output: ServiceFriends
define_graphql_request!(ServiceAggregationLayer, SearchPlayer, players); // Input: ServiceSearchPlayerRequest, Output: ServicePlayersPage
define_graphql_request!(ServiceAggregationLayer, getLegacyCatalogDefs, legacyOffers); // Input: ServiceGetLegacyCatalogDefsRequest, Output: Vec<ServiceLegacyOffer>
define_graphql_request!(ServiceAggregationLayer, getGameProducts, gameProducts); // Input: ServiceGetLegacyCatalogDefsRequest, Output: Vec<ServiceLegacyProduct>
define_graphql_request!(ServiceAggregationLayer, GetGamePlayTimes, me); // Input: ServiceGetLegacyCatalogDefsRequest, Output: Vec<ServiceLegacyProduct>
define_graphql_request!(ContentfulProxy, GetHeroBackgroundImage, gameHubCollection); // Input: ServiceHeroBackgroundImageRequest, Output: ServiceGameHubCollection

#[derive(Clone)]
pub struct ServiceLayerClient {
    auth: LockedAuthStorage,
    client: Client,
}

impl ServiceLayerClient {
    pub fn new(auth: LockedAuthStorage) -> Self {
        Self {
            auth,
            client: Client::new(),
        }
    }

    pub async fn request<T, R>(
        &self,
        operation: &ServiceLayerGraphQLRequest,
        variables: T,
    ) -> Result<R>
    where
        T: Serialize,
        R: for<'a> Deserialize<'a>,
    {
        let mut result = self.request2(operation, &variables, false).await;

        // On first error, try sending the full query
        if result.is_err() {
            result = self.request2(operation, variables, true).await;
        }

        result
    }

    async fn request2<T, R>(
        &self,
        operation: &ServiceLayerGraphQLRequest,
        variables: T,
        full_query: bool,
    ) -> Result<R>
    where
        T: Serialize,
        R: for<'a> Deserialize<'a>,
    {
        let extensions = ServiceExtensions {
            persisted_query: PersistedQuery {
                version: 1,
                sha256_hash: hex::encode(operation.hash),
            },
        };

        let host = match operation.r#type {
            ServiceLayerRequestType::ServiceAggregationLayer => API_SERVICE_AGGREGATION_LAYER,
            ServiceLayerRequestType::ContentfulProxy => API_CONTENTFUL_PROXY,
        };

        let mut request = if full_query {
            self.client.post(host)
        } else {
            self.client.get(host)
        };

        let access_token = self.auth.lock().await.access_token().await?;
        if let Some(access_token) = access_token {
            request = request.header("Authorization", &("Bearer ".to_owned() + &access_token));
        }

        let res = if full_query {
            let data = FullServiceRequest {
                extensions,
                variables,
                operation_name: operation.operation,
                query: operation.query,
            };

            request
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&data)?)
        } else {
            request.query(&[
                ("extensions", serde_json::to_string(&extensions)?.as_str()),
                ("operationName", operation.operation),
                ("variables", serde_json::to_string(&variables)?.as_str()),
            ])
        }
        .send()
        .await?;

        let status = res.status();
        let text = res.text().await?;
        if status != StatusCode::OK {
            bail!("Service request '{}' failed: {}", operation.operation, text);
        }

        debug!(
            "Service layer response for {}: {}",
            operation.operation, text
        );

        let result = serde_json::from_str::<Value>(text.as_str())?;
        let errors = result.get("errors");
        if errors.is_some() {
            let errors = errors.unwrap().as_array().unwrap();
            let error = if let Value::Object(o) = &errors[0] {
                o
            } else {
                bail!("Service request '{}' failed", operation.operation);
            };

            bail!(
                "Service request '{}' failed: {}",
                operation.operation,
                error.get("message").unwrap().as_str().unwrap()
            );
        }

        let data = result
            .get("data")
            .unwrap()
            .as_object()
            .unwrap()
            .get(operation.key)
            .unwrap()
            .to_owned();

        Ok(serde_json::from_value::<R>(data).unwrap())
    }
}

macro_rules! service_layer_type {
    ($name:ident, { $($field:tt)* }) => {
        paste::paste! {
            #[derive(Clone, Debug, Serialize, Deserialize, Getters, Builder)]
            #[serde(rename_all = "camelCase")]
            #[repr(C)]
            pub struct [<Service $name>] {
                $($field)*
            }
        }
    };
}

macro_rules! service_layer_enum {
    ($name:ident, { $($field:tt)* }) => {
        paste::paste! {
            #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
            #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
            #[repr(C)]
            pub enum [<Service $name>] {
                $($field)*
            }
        }
    };
}

// Requests

service_layer_type!(GetBasicPlayerRequest, {
    pd: String,
});

service_layer_type!(GameImagesRequest, {
    should_fetch_context_image: bool,
    should_fetch_backdrop_images: bool,
    game_slug: String,
    locale: String,
});

service_layer_enum!(GameProductType, {
    DigitalFullGame,
    DigitalExtraContent,
    PackagedFullGame,
    PackagedExtraContent,
    MicroContent,
    ExpansionPack,
    BundlePack,
    BaseGame,
    PrereleaseGame
});

service_layer_enum!(Storefront, {
    Ea,
    Steam,
    Epic,
});

service_layer_enum!(Platform, { Pc, MacPc });

service_layer_type!(GetPreloadedOwnedGamesRequest, {
    is_mac: bool,
    locale: Locale,
    limit: u32,
    next: String,
    r#type: ServiceGameProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    entitlement_enabled: Option<bool>,
    storefronts: Vec<ServiceStorefront>,
    platforms: Vec<ServicePlatform>,
});

service_layer_type!(GetUserPlayerRequest, {
    // There are presumably variables for this request,
    // but I'm not sure what they are.
});

service_layer_type!(AddonSearchRequest, {
    master_title_id: String,
    category_id: String,
    offer_ids: Vec<String>,
    platform: String,
});

// Responses

service_layer_type!(Image, {
    height: Option<u16>,
    width: Option<u16>,
    path: String,
});

service_layer_type!(AvatarList, {
    #[serde(deserialize_with = "ServiceImage::deserialize_large_avatar")]
    large: ServiceImage,
    #[serde(deserialize_with = "ServiceImage::deserialize_medium_avatar")]
    medium: ServiceImage,
    #[serde(deserialize_with = "ServiceImage::deserialize_small_avatar")]
    small: ServiceImage,
});

service_layer_type!(Player, {
    id: String,
    pd: String,
    psd: String,
    display_name: String,
    unique_name: String,
    nickname: String,
    avatar: Option<ServiceAvatarList>,
    relationship: String,
});

service_layer_type!(ImageRendition, {
    path: Option<String>,
    title: Option<String>,
    aspect_1x1_image: Option<ServiceImage>,
    aspect_2x1_image: Option<ServiceImage>,
    aspect_10x3_image: Option<ServiceImage>,
    aspect_8x3_image: Option<ServiceImage>,
    aspect_7x1_image: Option<ServiceImage>,
    aspect_7x2_image: Option<ServiceImage>,
    aspect_7x5_image: Option<ServiceImage>,
    aspect_5x3_image: Option<ServiceImage>,
    aspect_9x16_image: Option<ServiceImage>,
    aspect_16x9_image: Option<ServiceImage>,
    largest_image: Option<ServiceImage>,
    raw_images: Option<Vec<ServiceImage>>,
});

service_layer_type!(Game, {
    id: String,
    slug: Option<String>,
    base_game_slug: Option<String>,
    game_type: Option<ServiceGameProductType>,
    title: Option<String>,
    key_art: Option<ServiceImageRendition>,
    pack_art: Option<ServiceImageRendition>,
    primary_logo: Option<ServiceImageRendition>,
    context_image: Option<Vec<ServiceImageRendition>>,
});

// Game Product

service_layer_enum!(OwnershipMethod, {
    Unknown,
    Association,
    Purchase,
    Redemption,
    GiftReceipt,
    GiftPurchase,
    EntitlementGrant,
    DirectEntitlement,
    PreOrderPurchase,
    Vault,
    XgpVault, // Xbox Game Pass
    Steam,
    SteamVault,
    SteamSubscription,
    Epic,
    EpicVault,
});

service_layer_enum!(OwnershipStatus, {
    Active,
    Disabled,
});

service_layer_type!(GameProductUserTrial, {
    trial_time_remaining_seconds: Option<u32>,
});

service_layer_type!(GameProductUser, {
    ownership_methods: Vec<ServiceOwnershipMethod>,
    initial_entitlement_date: String,
    entitlement_id: Option<String>,
    game_product_user_trial: Option<ServiceGameProductUserTrial>,
    status: ServiceOwnershipStatus,
});

service_layer_type!(PurchaseStatus, {
    repurchasable: bool,
});

service_layer_enum!(TrialType, {
    PlayFirstTrial,
    OpenTrial,
    UngatedTrial,
});

service_layer_type!(TrialDetails, {
    trial_type: ServiceTrialType,
});

service_layer_type!(GameProduct, {
    id: String,
    #[getter(skip)]
    name: String,
    downloadable: bool,
    game_slug: String,
    trial_details: Option<ServiceTrialDetails>,
    base_item: ServiceGame,
    game_product_user: ServiceGameProductUser,
    purchase_status: ServicePurchaseStatus,
});

impl ServiceGameProduct {
    pub fn name(&self) -> String {
        self.name.replace("\n", "")
    }
}

service_layer_type!(UserGameProduct, {
    id: String,
    origin_offer_id: String,
    status: ServiceOwnershipStatus,
    product: ServiceGameProduct,
});

service_layer_type!(UserGameProductCursorPage, {
    next: Option<String>, // Unknown
    total_count: u32,
    items: Vec<ServiceUserGameProduct>,
});

service_layer_type!(User, {
    id: String,
    pd: Option<String>, // Persona ID
    player: Option<ServicePlayer>,
    owned_game_products: Option<ServiceUserGameProductCursorPage>,
});

service_layer_enum!(DownloadType, {
    Staged,
    Live,
    None,
});

impl ServiceDownloadType {
    pub fn to_string(&self) -> String {
        match self {
            ServiceDownloadType::Staged => "Staged".to_owned(),
            ServiceDownloadType::Live => "Live".to_owned(),
            ServiceDownloadType::None => "None".to_owned(),
        }
    }
}

service_layer_type!(AvailableBuild, {
    build_id: String,
    download_type: Option<ServiceDownloadType>,
    game_version: Option<String>,
    build_release_version: Option<String>,
    build_live_date: Option<String>,
});

impl ServiceAvailableBuild {
    pub fn to_string(&self) -> String {
        let mut str = self.game_version.to_owned().unwrap_or("UnkVer".to_owned());

        if self.download_type.is_some() {
            str += &("/".to_owned() + &self.download_type.as_ref().unwrap().to_string());
        }

        if self.build_live_date.is_some() {
            str += &(" - ".to_owned() + &self.build_live_date.as_ref().unwrap());
        }

        str
    }
}

service_layer_type!(AvailableBuilds, {
    pub builds: Vec<ServiceAvailableBuild>,
});

impl ServiceAvailableBuilds {
    pub fn live_build(&self) -> Option<&ServiceAvailableBuild> {
        self.builds.iter().find(|b| {
            b.download_type()
                .as_ref()
                .unwrap_or(&ServiceDownloadType::None)
                == &ServiceDownloadType::Live
        })
    }

    pub fn build(&self, id: &str) -> Option<&ServiceAvailableBuild> {
        self.builds
            .iter()
            .find(|b| b.game_version() == &Some(id.to_owned()))
    }
}

service_layer_type!(AvailableBuildsRequest, {
    offer_id: String,
});

service_layer_type!(DownloadUrlRequest, {
    offer_id: String,
    build_id: String,
});

service_layer_type!(DownloadUrlMetadata, {
    url: String,
    sync_url: Option<String>,
});

service_layer_type!(GrantEntitlementInput, {
    offer_id: String,
    source: Option<String>,
});

service_layer_type!(GrantEntitlementRequest, {
    input: ServiceGrantEntitlementInput,
});

service_layer_type!(GameSessionStartInput, {
    game_slug: String,
    platform: ServicePlatform,
    session_id: String,
});

service_layer_type!(GameSessionStartRequest, {
    input: ServiceGameSessionStartInput,
});

service_layer_type!(GameSessionEndInput, {
    session_id: String,
});

service_layer_type!(GameSessionEndRequest, {
    input: ServiceGameSessionEndInput,
});

service_layer_type!(GameBundleInput, {
    offerId: String,
});

service_layer_type!(GameBundleRequest, {
    bundles: Vec<ServiceGameBundleInput>,
    region: String,
});

service_layer_type!(GameSystemRequirementsRequest, {
    slug: String,
    locale: String, // Short string, eg "en"
});

service_layer_type!(SystemRequirements, {
    minimum: String,
    recommended: String,
    platform: ServicePlatform,
});

service_layer_type!(GameSystemRequirements, {
    id: String,
    game_type: ServiceGameProductType,
    system_requirements: Vec<ServiceSystemRequirements>,
});

service_layer_type!(GetMyFriendsRequest, {
    offset: u32,
    limit: u32,
    is_mutual_friends_enabled: bool,
});

service_layer_type!(Friend, {
    id: String,
    pd: String,
    player: ServicePlayer,
});

service_layer_type!(FriendsOffsetPage, {
    total_count: u32,
    has_next_page: bool,
    has_previous_page: bool,
    items: Vec<ServiceFriend>,
});

service_layer_type!(Friends, {
    id: String,
    pd: String,
    friends: ServiceFriendsOffsetPage,
});

service_layer_type!(SearchPlayerRequest, {
    is_mutual_friends_enabled: bool,
    page_number: u32,
    page_size: u32,
    search_text: String,
});

service_layer_type!(PlayersPage, {
    items: Vec<ServicePlayer>,
});

service_layer_type!(GetLegacyCatalogDefsRequest, {
    offer_ids: Vec<String>,
    locale: Locale,
});

service_layer_type!(LegacyOffer, {
    offer_id: String,
    content_id: String,
    primary_master_title_id: String,
    #[serde(rename = "gameLauncherURL")]
    game_launcher_url: Option<String>,
    #[serde(rename = "gameLauncherURLClientID")]
    game_launcher_url_client_id: Option<String>,
    multiplayer_id: Option<String>,
    execute_path_override: Option<String>,
    installation_directory: Option<String>,
    install_check_override: Option<String>,
    monitor_play: Option<bool>,
    display_name: String,
    display_type: String,
    dip_manifest_relative_path: Option<String>,
    //downloads: Vec<ServiceAvailableBuild>,
    is_downloadable: bool,
    cloud_save_configuration_override: Option<String>,
});

impl ServiceLegacyOffer {
    pub fn has_cloud_save(&self) -> bool {
        !self
            .cloud_save_configuration_override
            .clone()
            .unwrap_or_default()
            .is_empty()
    }
}

service_layer_type!(AddonOffer, {
        offer_id: String,
        offer_type: String,
        finance_id: String,
        default_locale: String,
        platform: crate::core::ecommerce::CommercePlatform,
        image_server: String,
        game_edition_type_facet_key_rank_desc: String,
        long_description: String,
        display_name: String,
        short_description: String,
        pack_art_small: String,
        pack_art_medium: String,
        pack_art_large: String,
        origin_display_type: String,
        is_published: bool,
        published_date: String,
        cdn_asset_root: String,
        origin_store_preview: bool,
        is_owned: bool,
        user_can_purchase: bool,
        price: f32,
        display_price: String,
        list_price: f64,
        display_list_price: String,
        currency_type: String,
        currency: String,
        is_discount: bool,
    }
);

service_layer_type!(AddonSearchResultRoot, {
    addonSearch: ServiceAddonSearchResult,
});

service_layer_type!(AddonSearchResult, {
    addonOffers: Vec<ServiceAddonOffer>,
});

service_layer_type!(RecentGames, {});

service_layer_type!(HeroBackgroundImageRequest, {
    game_slug: String,
    locale: String, // Short string, eg "en"
});

service_layer_type!(Asset, {
    url: Option<String>,
});

service_layer_type!(GameHub, {
    background_video: Option<ServiceAsset>,
    hero_background: ServiceImageRendition,
});

service_layer_type!(GameHubCollection, {
    items: Vec<ServiceGameHub>,
});

// Serde treats a field being null differently from the field not being there, so we need to do custom deserialization to handle this.
impl ServiceImage {
    fn deserialize_large_avatar<'de, D>(deserializer: D) -> Result<ServiceImage, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            ServiceImage::deserialize(deserializer).unwrap_or(ServiceImage {
                height: Some(416),
                width: Some(416),
                path: LARGE_AVATAR_PATH.to_owned(),
            }),
        )
    }

    fn deserialize_medium_avatar<'de, D>(deserializer: D) -> Result<ServiceImage, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            ServiceImage::deserialize(deserializer).unwrap_or(ServiceImage {
                width: Some(208),
                height: Some(208),
                path: MEDIUM_AVATAR_PATH.to_owned(),
            }),
        )
    }

    fn deserialize_small_avatar<'de, D>(deserializer: D) -> Result<ServiceImage, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            ServiceImage::deserialize(deserializer).unwrap_or(ServiceImage {
                width: Some(40),
                height: Some(40),
                path: SMALL_AVATAR_PATH.to_owned(),
            }),
        )
    }
}
