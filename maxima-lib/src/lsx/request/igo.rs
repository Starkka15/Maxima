use anyhow::Result;
use log::info;

use crate::lsx::{
    connection::Connection,
    types::{LSXResponseType, LSXShowIGOWindow},
};

pub async fn handle_show_igo_window_request(
    connection: &mut Connection,
    request: LSXShowIGOWindow,
) -> Result<Option<LSXResponseType>> {
    info!("Got request to show user {}", request.target_id);

    let data = connection.maxima()
        .await
        .player_by_id(&request.target_id.to_string())
        .await?;

    info!("{:?}", data);
    Ok(None)
}
