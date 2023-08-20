#![allow(non_snake_case)]

use anyhow::{bail, Result};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

pub const SERVICE_REQUEST_PLAYERBYPD: (&str, &str) = (
    "GetBasicPlayer",
    "b60b22e2071548c4c87ed1ebf7fba2a653f7cf9a7b62bf742bf30caba95d6346",
);
pub const SERVICE_REQUEST_GETUSERPLAYER: (&str, &str) = (
    "GetUserPlayer",
    "387cef4a793043a4c76c92ff4f2bceb7b25c3438f9c3c4fd5eb67eea18272657",
);
pub const SERVICE_REQUEST_GAMEIMAGES: (&str, &str) = (
    "GameImages",
    "ea5448b2ef84b418d150d66a13ba32a34559966c3c7bd30e506d26456a316be8", //5ab5a2453cd970cb95e39d9d8a96251584a432c39bc47093a1304ff7b8ca3f03
);
pub const SERVICE_REQUEST_GETPRELOADEDOWNEDGAMES: (&str, &str) = (
    "getPreloadedOwnedGames",
    "0c19ffbc5858eae560d8a6928cf0e0b0040b876bd96ceb39c6cf85a827caa270",
);

pub const SERVICE_REQUEST_GETGAME: (&str, &str) = (
    "getGame",
    include_str!("graphql/game.graphql")
);

struct GraphQLRequest {
    pub query_source: String,
    pub hash: String,
}

struct ServiceExecutor {

}

pub async fn send_service_request<T, R>(
    access_token: &str,
    (operation, hash): (&str, &str),
    variables: T,
) -> Result<R>
where
    T: Serialize,
    R: for<'a> Deserialize<'a>,
{
    let extensions = serde_json::to_string(&ServiceExtensions {
        persisted_query: PersistedQuery {
            version: 1,
            sha256_hash: hash.to_string(),
        },
    })
    .unwrap();

    let variable_json = serde_json::to_string(&variables).unwrap();
    let query = vec![
        ("extensions", extensions.as_str()),
        ("operationName", operation),
        ("variables", variable_json.as_str()),
    ];

    let res = ureq::get(API_SERVICE_AGGREGATION_LAYER)
        .query_pairs(query)
        .set("Authorization", &("Bearer ".to_string() + access_token))
        .call()?;
    if res.status() != StatusCode::OK {
        bail!("Service request '{}' failed: {}", operation, res.into_string()?);
    }

    let text = res.into_string()?;
    let result = serde_json::from_str::<Value>(text.as_str())?;
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
            #[derive(Debug, Serialize, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct [<Service $name>] {
                $($field)*
            }
        }
    };
}

macro_rules! service_layer_enum {
    ($name:ident, { $($field:tt)* }) => {
        paste::paste! {
            #[derive(Debug, Serialize, Deserialize, PartialEq)]
            #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
            pub enum [<Service $name>] {
                $($field)*
            }
        }
    };
}

// Requests

service_layer_type!(PlayerByPlayerIdRequest, {
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
