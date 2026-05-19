use crate::{
    core::service_layer::{
        ServiceEntitlement, ServiceSdkEntitlementsRequestBuilder, ServiceSdkEntitlementsResult,
        SERVICE_REQUEST_SDKENTITLEMENTS,
    },
    lsx::{
        connection::LockedConnectionState,
        request::LSXRequestError,
        types::{
            LSXEntitlement, LSXQueryEntitlements, LSXQueryEntitlementsResponse, LSXResponseType,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_query_entitlements_request(
    state: LockedConnectionState,
    request: LSXQueryEntitlements,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let maxima = state.write().await.maxima_arc();
    let maxima = maxima.lock().await;
    let service_layer = maxima.service_layer();

    let mut entitlements: Vec<ServiceEntitlement> = Vec::new();

    let response: ServiceSdkEntitlementsResult = service_layer
        .request(
            SERVICE_REQUEST_SDKENTITLEMENTS,
            ServiceSdkEntitlementsRequestBuilder::default()
                .page_number(1)
                .page_size(100)
                .product_ids(Vec::new())
                .include_child_groups(false)
                .entitlement_tag("".to_string())
                .group_names([request.attr_Group.clone()].to_vec())
                .build()
                .unwrap(),
        )
        .await?;

    entitlements.append(&mut response.sdk_entitlements().entitlements().clone());
    // there's some hints of pagination here but i'm not sure how to handle that :)

    let mut lsx_entitlements = Vec::new();
    for entitlement in entitlements {
        lsx_entitlements.push(LSXEntitlement {
            attr_LastModifiedDate: "0000-00-00T00:00:00".to_string(), // it's like this in EAD too
            attr_EntitlementId: entitlement.id().parse::<u64>()?,
            attr_UseCount: entitlement.use_count().clone(),
            attr_Version: entitlement.version().clone(),
            attr_ItemId: entitlement.product_id().clone(),
            attr_ResourceId: String::new(),
            attr_GrantDate: entitlement.grant_date().to_string(),
            attr_Group: request.attr_Group.to_string(),
            attr_EntitlementTag: entitlement.entitlement_tag().clone(),
            attr_Type: entitlement.entitlement_type().clone(),
            attr_Expiration: entitlement
                .termination_date()
                .clone()
                .unwrap_or("0000-00-00T00:00:00".to_string()),
            attr_Source: "".to_string(),
        });
    }

    make_lsx_handler_response!(Response, QueryEntitlementsResponse, { entitlement: lsx_entitlements })
}
