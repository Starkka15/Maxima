use anyhow::Result;
use log::info;

use crate::{
    core::service_layer::{
        send_service_request, ServicePlayer, ServicePlayerByPlayerIdRequest,
        SERVICE_REQUEST_PLAYERBYPD,
    },
    lsx::{
        connection::Connection,
        types::{LSXResponseType, LSXShowIGOWindow},
    },
};

pub async fn handle_show_igo_window_request(
    connection: &mut Connection,
    request: LSXShowIGOWindow,
) -> Result<Option<LSXResponseType>> {
    info!("Got request to show user {}", request.target_id);
    let data: ServicePlayer = send_service_request(
        &connection.get_access_token().await,
        SERVICE_REQUEST_PLAYERBYPD,
        ServicePlayerByPlayerIdRequest {
            pd: request.target_id.to_string(),
        },
    )
    .await?;
    info!("{:?}", data);
    Ok(None)
}
