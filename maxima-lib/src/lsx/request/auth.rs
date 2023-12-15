use anyhow::Result;
use log::info;

use crate::{
    core::auth::execute_auth_exchange,
    lsx::{
        connection::Connection,
        types::{LSXAuthCode, LSXGetAuthCode, LSXResponseType},
    },
    make_lsx_handler_response,
};

pub async fn handle_auth_code_request(
    connection: &mut Connection,
    request: LSXGetAuthCode,
) -> Result<Option<LSXResponseType>> {
    let access_token = connection.access_token().await;
    let client_id = request.attr_ClientId;
    info!("Retrieving authorization code for '{}'", client_id);
    
    let auth_code = execute_auth_exchange(&access_token, &client_id, "code").await.unwrap();
    make_lsx_handler_response!(Response, AuthCode, { attr_value: auth_code })
}
