#![allow(non_snake_case)]

use anyhow::{bail, Result};
use reqwest::{Client, StatusCode};

use serde::{Deserialize, Serialize};

use super::endpoints::API_ECOMMERCE;

pub async fn request_entitlements(
    access_token: &str,
    user_id: &str,
    group_name: Option<&str>,
) -> Result<Vec<CommerceEntitlement>> {
    let mut query = Vec::new();

    if let Some(group_name) = group_name {
        query.push(("groupName", group_name));
    }

    let res = Client::new()
        .get(format!("{}/entitlements/{}", API_ECOMMERCE, user_id))
        .query(&query)
        .header("AuthToken", access_token)
        .header("Accept", "application/json")
        .send()
        .await?;
    if res.status() != StatusCode::OK {
        bail!("Ecommerce request failed: {}", res.text().await?);
    }

    let text = res.text().await?;
    let result: CommerceEntitlements = serde_json::from_str(text.as_str())?;
    Ok(result.entitlements)
}

pub async fn request_offer_data(
    access_token: &str,
    offer: &str,
    locale: &str,
) -> Result<CommerceOffer> {
    let res = ureq::get(&format!("{}/private/{}/{}", API_ECOMMERCE, offer, locale))
        .set("AuthToken", access_token)
        .call()?;
    if res.status() != StatusCode::OK {
        bail!("Ecommerce request failed: {}", res.into_string()?);
    }

    let text = res.into_string()?;
    let result = quick_xml::de::from_str(text.as_str())?;
    Ok(result)
}

macro_rules! ecommerce_type {
    (
        $(#[$message_attr:meta])*
        $message_name:ident;
        attr {
            $(
                $(#[$attr_field_attr:meta])*
                $attr_field:ident: $attr_field_type:ty
            ),* $(,)?
        },
        data {
            $(
                $(#[$field_attr:meta])*
                $field:ident: $field_type:ty
            ),* $(,)?
        }
    ) => {
        paste::paste! {
            // Main struct definition
            $(#[$message_attr])*
            #[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
            #[serde(rename_all = "camelCase")]
            pub struct [<Commerce $message_name>] {
                $(
                    $(#[$attr_field_attr])*
                    #[serde(rename = "@" $attr_field)]
                    pub [<attr_ $attr_field>]: $attr_field_type,
                )*
                $(
                    $(#[$field_attr])*
                    pub $field: $field_type,
                )*
            }
        }
    }
}

macro_rules! ecommerce_enum {
    ($name:ident, { $($field:tt)* }) => {
        paste::paste! {
            #[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
            #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
            pub enum [<Commerce $name>] {
                #[default]
                $($field)*
            }
        }
    };
}

ecommerce_type!(
    PublishingAttributes;
    attr {},
    data {
        content_id: String,
        grey_market_controls: Option<bool>,
        is_downloadable: bool,
        game_distribution_sub_type: Option<String>,
        origin_display_type: String,
        is_published: bool,
    }
);

ecommerce_type!(
    Software;
    attr {
        softwarePlatform: String,
    },
    data {
        software_id: String,
        fulfillment_attributes: CommerceFulfillmentAttributes,
    }
);

ecommerce_type!(
    SoftwareList;
    attr {},
    data {
        software: Vec<CommerceSoftware>,
    }
);

ecommerce_type!(
    Publishing;
    attr {},
    data {
        publishing_attributes: CommercePublishingAttributes,
        software_list: Option<CommerceSoftwareList>,
    }
);

ecommerce_type!(
    FulfillmentAttributes;
    attr {},
    data {
        installation_directory: Option<String>,
    }
);

ecommerce_type!(
    LocalizableAttributes;
    attr {},
    data {
        display_name: String,
    }
);

ecommerce_type!(
    Offer;
    attr {
        itemName: String,
        offerType: String,
        offerId: String,
        projectNumber: String,
    },
    data {
        item_id: String,
        store_group_id: String,
        finance_id: String,
        default_locale: String,
        publishing: CommercePublishing,
        localizable_attributes: CommerceLocalizableAttributes,
    }
);

ecommerce_enum!(EntitlementExternalType, {
    Steam,
    Subscription,
});

ecommerce_enum!(EntitlementSource, {
    Steam,
    #[serde(rename = "ORIGIN-STORE-CLIENT-US")]
    OriginStoreClientUs,
});

ecommerce_enum!(EntitlementType, {
    Default,
    OnlineAccess,
});

ecommerce_enum!(EntitlementStatus, {
    Active,
});

ecommerce_type!(
    Entitlement;
    attr {},
    data {
        external_type: Option<CommerceEntitlementExternalType>,
        product_id: String,
        last_modified_date: String,
        entitlement_source: CommerceEntitlementSource,
        entitlement_id: u64,
        grant_date: String,
        entitlement_type: CommerceEntitlementType,
        version: u16,
        is_consumable: bool,
        product_catalog: String,
        group_name: String,
        entitlement_tag: String,
        origin_permissions: String,
        use_count: u32,
        project_id: String,
        status: CommerceEntitlementStatus,
    }
);

ecommerce_type!(
    Entitlements;
    attr {},
    data {
        entitlements: Vec<CommerceEntitlement>,
    }
);