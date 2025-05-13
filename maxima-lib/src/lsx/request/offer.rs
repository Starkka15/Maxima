use anyhow::Result;
use log::debug;
use crate::{
    core::service_layer::{ServiceAddonSearchRequestBuilder, ServiceAddonSearchResult, ServiceAddonSearchResultRoot, SERVICE_REQUEST_ADDONSEARCH},
    lsx::{
        connection::LockedConnectionState,
        types::{
            LSXOffer,
            LSXQueryOffers,
            LSXQueryOffersResponse,
            LSXResponseType,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_query_offers_request(
    conn: LockedConnectionState,
    request: LSXQueryOffers,
) -> Result<Option<LSXResponseType>> {
    let mut rtn: Vec<LSXOffer> = Vec::new();

    let category = if let Some(category) = request.FilterCategories.first() {
        category.clone()
    } else {
        String::new()
    };

    let mut conn = conn.write().await;
    let maxima = conn.maxima().await;
    let offers: ServiceAddonSearchResultRoot = maxima.service_layer().request(
        SERVICE_REQUEST_ADDONSEARCH,
        ServiceAddonSearchRequestBuilder::default()
            .platform(String::new())
            .category_id(category)
            .master_title_id(String::new())
            .offer_ids(Vec::new())
            .build()?
    ).await?;

    for offer in offers.addonSearch().addonOffers() {
        rtn.push(LSXOffer {
            attr_InventorySold: 0,
            attr_LocalizedPrice: offer.display_list_price().to_string(),
            attr_OriginalPrice: offer.price().to_string(),
            attr_DownloadDate: "0000-00-00T00:00:00".to_string(),
            attr_Currency: offer.currency().to_string(),
            attr_InventoryAvailable: 0,
            attr_PurchaseDate: "0000-00-00T00:00:00".to_string(),
            attr_DownloadSize: 0,
            attr_bCanPurchase: offer.user_can_purchase().clone(),
            attr_Price: offer.list_price().to_string(),
            attr_Type: offer.origin_display_type().to_string(),
            attr_LocalizedOriginalPrice: offer.display_price().to_string(),
            attr_InventoryCap: 0,
            attr_Description: offer.long_description().to_string(),
            attr_bHidden: !offer.is_published().clone(),
            attr_PlayableDate: offer.published_date().to_string(),
            attr_Name: offer.display_name().to_string(),
            attr_ImageId: String::new(),
            attr_bIsDiscounted: offer.is_discount().clone(),
            attr_OfferId: offer.offer_id().clone(),
            attr_bIsOwned: offer.is_owned().clone(),
        })
    }


    make_lsx_handler_response!(Response, QueryOffersResponse, { offer: rtn })
}
