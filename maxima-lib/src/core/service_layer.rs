#![allow(non_snake_case)]

use anyhow::{bail, Result};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2_const::Sha256;
use ureq::OrAnyStatus;

use derive_getters::Getters;
use derive_builder::Builder;

use super::{endpoints::API_SERVICE_AGGREGATION_LAYER, locale::Locale};

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

pub struct GraphQLRequest {
    query: &'static str,
    operation: &'static str,
    hash: [u8; 32],
}

macro_rules! load_graphql_request {
    ($operation:expr) => {{
        let content = include_str!(concat!("graphql/", $operation, ".gql"));
        let hash = Sha256::new().update(content.as_bytes()).finalize();
        GraphQLRequest {
            query: content,
            operation: $operation,
            hash,
        }
    }};
}

macro_rules! define_graphql_request {
    ($operation:expr) => { paste::paste! {
        pub const [<SERVICE_REQUEST_ $operation:upper>]: &GraphQLRequest = &load_graphql_request!(stringify!($operation));
    }}
}

define_graphql_request!(availableBuilds);
define_graphql_request!(downloadUrl);
define_graphql_request!(GameImages);
define_graphql_request!(GetBasicPlayer);
define_graphql_request!(getPreloadedOwnedGames);
define_graphql_request!(GetUserPlayer);

pub async fn send_service_request<T, R>(
    access_token: &str,
    operation: &GraphQLRequest,
    variables: T,
) -> Result<R>
where
    T: Serialize,
    R: for<'a> Deserialize<'a>,
{
    let mut result = send_service_request2(access_token, operation, &variables, false).await;

    // On first error, try sending the full query
    if result.is_err() {
        result = send_service_request2(access_token, operation, variables, true).await;
    }

    result
}

async fn send_service_request2<T, R>(
    access_token: &str,
    operation: &GraphQLRequest,
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

    let agent = ureq::AgentBuilder::new().try_proxy_from_env(true).build();

    let mut request = if full_query {
        agent.post(API_SERVICE_AGGREGATION_LAYER)
    } else {
        agent.get(API_SERVICE_AGGREGATION_LAYER)
    };

    request = request.set("Authorization", &("Bearer ".to_string() + access_token));

    let res = if full_query {
        let data = FullServiceRequest {
            extensions,
            variables,
            operation_name: operation.operation,
            query: operation.query,
        };

        request
            .set("Content-Type", "application/json")
            .send_string(&serde_json::to_string(&data)?)
    } else {
        request
            .query("extensions", &serde_json::to_string(&extensions)?)
            .query("operationName", operation.operation)
            .query("variables", &serde_json::to_string(&variables)?)
            .call()
    }
    .or_any_status()?;

    if res.status() != StatusCode::OK {
        bail!(
            "Service request '{}' failed: {}",
            operation.operation,
            res.into_string()?
        );
    }

    let text = res.into_string()?;
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
        .values()
        .next()
        .unwrap()
        .to_owned();

    Ok(serde_json::from_value::<R>(data).unwrap())
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

service_layer_enum!(GameType, { DigitalFullGame, BaseGame, PrereleaseGame });

service_layer_enum!(Storefront, {
    Ea,
    Steam,
    Epic,
});

service_layer_enum!(Platform, { Pc });

service_layer_type!(GetPreloadedOwnedGamesRequest, {
    is_mac: bool,
    locale: Locale,
    limit: u32,
    next: String,
    r#type: ServiceGameType,
    #[serde(skip_serializing_if = "Option::is_none")]
    entitlement_enabled: Option<bool>,
    storefronts: Vec<ServiceStorefront>,
    platforms: Vec<ServicePlatform>,
});

service_layer_type!(GetUserPlayerRequest, {
    // There are presumably variables for this request,
    // but I'm not sure what they are.
});

// Responses

service_layer_type!(Image, {
    height: Option<u16>,
    width: Option<u16>,
    path: String,
});

service_layer_type!(AvatarList, {
    large: ServiceImage,
    medium: ServiceImage,
    small: ServiceImage,
});

service_layer_type!(Player, {
    id: String,
    pd: String,
    psd: String,
    display_name: String,
    unique_name: String,
    nickname: String,
    avatar: ServiceAvatarList,
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
    game_type: Option<ServiceGameType>,
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
    XgpVault,
    Steam,
    SteamVault,
    SteamSubscription,
    Epic,
});

service_layer_enum!(OwnershipStatus, {
    Active,
});

service_layer_type!(GameProductUserTrial, {
    trial_time_remaining_seconds: u32,
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
    game_version: String,
    build_live_date: Option<String>,
});

impl ServiceAvailableBuild {
    pub fn to_string(&self) -> String {
        let mut str = self.game_version.to_owned();

        if self.download_type.is_some() {
            str += &("/".to_owned() + &self.download_type.as_ref().unwrap().to_string());
        }

        if self.build_live_date.is_some() {
            str += &(" - ".to_owned() + &self.build_live_date.as_ref().unwrap());
        }

        str
    }
}

service_layer_type!(AvailableBuildsRequest, {
    offer_id: String,
});

service_layer_type!(DownloadUrlRequest, {
    offer_id: String,
    build_id: String,
});

service_layer_type!(DownloadUrl, {
    url: String,
    sync_url: String,
});
