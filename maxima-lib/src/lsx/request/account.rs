use anyhow::Result;

use crate::{
    core::ecommerce::request_entitlements,
    lsx::{
        connection::Connection,
        types::{
            LSXEntitlement, LSXQueryEntitlements, LSXQueryEntitlementsResponse, LSXResponseType,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_query_entitlements_request(
    connection: &mut Connection,
    request: LSXQueryEntitlements,
) -> Result<Option<LSXResponseType>> {
    let maxima = connection.maxima().await;
    let entitlements = request_entitlements(
        &maxima.access_token(),
        &request.attr_UserId.to_string(),
        Some(&request.attr_Group.to_owned()),
    )
    .await?;

    let mut lsx_entitlements = Vec::new();
    for entitlement in entitlements {
        lsx_entitlements.push(LSXEntitlement {
            attr_LastModifiedDate: entitlement.last_modified_date,
            attr_EntitlementId: entitlement.entitlement_id,
            attr_UseCount: entitlement.use_count,
            attr_Version: entitlement.version,
            attr_ItemId: entitlement.product_id,
            attr_ResourceId: String::new(),
            attr_GrantDate: entitlement.grant_date,
            attr_Group: request.attr_Group.to_owned(),
            attr_EntitlementTag: entitlement.entitlement_tag,
            attr_Type: entitlement.entitlement_type,
            attr_Expiration: "0000-00-00T00:00:00".to_string(),
            attr_Source: entitlement.entitlement_source,
        });
    }

    make_lsx_handler_response!(Response, QueryEntitlementsResponse, { entitlement: lsx_entitlements })
}
