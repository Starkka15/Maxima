use anyhow::{bail, Result};
use log::info;

use crate::{
    lsx::{
        connection::Connection,
        types::{LSXRequestLicense, LSXRequestLicenseResponse, LSXResponseType},
    },
    make_lsx_handler_response,
    ooa::{request_license, save_licenses},
};

pub async fn handle_license_request(
    connection: &mut Connection,
    request: LSXRequestLicense,
) -> Result<Option<LSXResponseType>> {
    info!("Requesting OOA License and Denuvo Token");

    let offer = connection.current_offer().await;
    let access_token = connection.access_token().await;

    let license = request_license(
        offer.publishing.publishing_attributes.content_id.unwrap().as_str(),
        "ca5f9ae34d7bcd895e037a17769de60338e6e84",
        access_token.as_str(),
        Some(request.attr_RequestTicket.as_str()),
        Some(request.attr_TicketEngine.as_str()),
    )
    .await?;

    if license.game_token.is_none() {
        bail!("Failed to retrieve Denuvo token");
    }

    info!("Successfully retrieved license tokens");
    save_licenses(&license)?;

    make_lsx_handler_response!(Response, RequestLicenseResponse, { attr_License: license.game_token.unwrap() })
}
