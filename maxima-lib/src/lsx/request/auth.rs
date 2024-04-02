use anyhow::Result;
use log::{error, info};

use crate::{
    core::auth::{context::AuthContext, nucleus_auth_exchange},
    lsx::{
        connection::LockedConnectionState,
        types::{LSXAuthCode, LSXGetAuthCode, LSXResponseType},
    },
    make_lsx_handler_response,
};

pub async fn handle_auth_code_request(
    state: LockedConnectionState,
    request: LSXGetAuthCode,
) -> Result<Option<LSXResponseType>> {
    let client_id = request.attr_ClientId;
    info!("Retrieving authorization code for '{}'", client_id);

    let mut context = AuthContext::new()?;

    let access_token = state.write().await.access_token().await?;
    context.set_access_token(&access_token);

    let auth_res = nucleus_auth_exchange(&context, &client_id, "code").await;
    let auth_code = if let Err(err) = auth_res {
        error!(
            "Failed to retrieve LSX auth code for '{}': {:?}",
            client_id, err
        );

        String::from("invalid")
    } else {
        auth_res.unwrap()
    };

    make_lsx_handler_response!(Response, AuthCode, { attr_value: auth_code })
}
