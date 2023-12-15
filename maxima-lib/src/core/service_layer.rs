#![allow(non_snake_case)]

use anyhow::{bail, Result};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2_const::Sha256;
use ureq::OrAnyStatus;

use super::{endpoints::API_SERVICE_AGGREGATION_LAYER, locale::Locale};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedQuery {
    pub version: u8,
    pub sha256_hash: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceExtensions {
    pub persisted_query: PersistedQuery,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullServiceRequest<'a, T: Serialize> {
    pub extensions: ServiceExtensions,
    pub variables: T,
    pub operation_name: &'a str,
    pub query: &'static str,
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

define_graphql_request!(GetBasicPlayer);
define_graphql_request!(GetUserPlayer);
define_graphql_request!(GameImages);
define_graphql_request!(getPreloadedOwnedGames);

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
            #[derive(Clone, Debug, Serialize, Deserialize)]
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
    pub pd: String,
});

service_layer_type!(GameImagesRequest, {
    pub should_fetch_context_image: bool,
    pub should_fetch_backdrop_images: bool,
    pub game_slug: String,
    pub locale: String,
});

service_layer_enum!(GameType, { DigitalFullGame, BaseGame, PrereleaseGame });

service_layer_enum!(Storefront, {
    Ea,
    Steam,
    Epic,
});

service_layer_enum!(Platform, { Pc });

service_layer_type!(GetPreloadedOwnedGamesRequest, {
    pub is_mac: bool,
    pub locale: Locale,
    pub limit: u32,
    pub next: String,
    pub r#type: ServiceGameType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entitlement_enabled: Option<bool>,
    pub storefronts: Vec<ServiceStorefront>,
    pub platforms: Vec<ServicePlatform>,
});

service_layer_type!(GetUserPlayerRequest, {
    // There are presumably variables for this request,
    // but I'm not sure what they are.
});

// Responses

service_layer_type!(Image, {
    pub height: Option<u16>,
    pub width: Option<u16>,
    pub path: String,
});

service_layer_type!(AvatarList, {
    pub large: ServiceImage,
    pub medium: ServiceImage,
    pub small: ServiceImage,
});

service_layer_type!(Player, {
    pub id: String,
    pub pd: String,
    pub psd: String,
    pub display_name: String,
    pub unique_name: String,
    pub nickname: String,
    pub avatar: ServiceAvatarList,
    pub relationship: String,
});

service_layer_type!(ImageRendition, {
    pub path: Option<String>,
    pub title: Option<String>,
    pub aspect_1x1_image: Option<ServiceImage>,
    pub aspect_2x1_image: Option<ServiceImage>,
    pub aspect_10x3_image: Option<ServiceImage>,
    pub aspect_8x3_image: Option<ServiceImage>,
    pub aspect_7x1_image: Option<ServiceImage>,
    pub aspect_7x2_image: Option<ServiceImage>,
    pub aspect_7x5_image: Option<ServiceImage>,
    pub aspect_5x3_image: Option<ServiceImage>,
    pub aspect_9x16_image: Option<ServiceImage>,
    pub aspect_16x9_image: Option<ServiceImage>,
    pub largest_image: Option<ServiceImage>,
    pub raw_images: Option<Vec<ServiceImage>>,
});

service_layer_type!(Game, {
    pub id: String,
    pub slug: Option<String>,
    pub base_game_slug: Option<String>,
    pub game_type: Option<ServiceGameType>,
    pub title: Option<String>,
    pub key_art: Option<ServiceImageRendition>,
    pub pack_art: Option<ServiceImageRendition>,
    pub primary_logo: Option<ServiceImageRendition>,
    pub context_image: Option<Vec<ServiceImageRendition>>,
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
    pub trial_time_remaining_seconds: u32,
});

service_layer_type!(GameProductUser, {
    pub ownership_methods: Vec<ServiceOwnershipMethod>,
    pub initial_entitlement_date: String,
    pub entitlement_id: Option<String>,
    pub game_product_user_trial: Option<ServiceGameProductUserTrial>,
    pub status: ServiceOwnershipStatus,
});

service_layer_type!(PurchaseStatus, {
    pub repurchasable: bool,
});

service_layer_enum!(TrialType, {
    PlayFirstTrial,
    OpenTrial,
});

service_layer_type!(TrialDetails, {
    pub trial_type: ServiceTrialType,
});

service_layer_type!(GameProduct, {
    pub id: String,
    pub name: String,
    pub downloadable: bool,
    pub game_slug: String,
    pub trial_details: Option<ServiceTrialDetails>,
    pub base_item: ServiceGame,
    pub game_product_user: ServiceGameProductUser,
    pub purchase_status: ServicePurchaseStatus,
});

impl ServiceGameProduct {
    pub fn get_name(&self) -> String {
        self.name.replace("\n", "")
    }
}

service_layer_type!(UserGameProduct, {
    pub id: String,
    pub origin_offer_id: String,
    pub status: ServiceOwnershipStatus,
    pub product: ServiceGameProduct,
});

service_layer_type!(UserGameProductCursorPage, {
    pub next: Option<String>, // Unknown
    pub total_count: u32,
    pub items: Vec<ServiceUserGameProduct>,
});

service_layer_type!(User, {
    pub id: String,
    pub pd: Option<String>, // Persona ID
    pub player: Option<ServicePlayer>,
    pub owned_game_products: Option<ServiceUserGameProductCursorPage>,
});
