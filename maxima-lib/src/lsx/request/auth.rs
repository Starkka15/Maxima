use log::{error, info};

use crate::{
    core::auth::{context::AuthContext, nucleus_auth_exchange},
    lsx::{
        connection::LockedConnectionState,
        request::LSXRequestError,
        types::{LSXAuthCode, LSXGetAuthCode, LSXResponseType},
    },
    make_lsx_handler_response,
};

pub async fn handle_auth_code_request(
    state: LockedConnectionState,
    request: LSXGetAuthCode,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let client_id = request.attr_ClientId;
    info!("Retrieving authorization code for '{}'", client_id);

    let mut context = AuthContext::new()?;

    let access_token = state.write().await.access_token().await?;
    context.set_access_token(&access_token);

    let auth_res = nucleus_auth_exchange(&context, &client_id, "code").await;
    let auth_code = match auth_res {
        Ok(auth_code) => auth_code,
        Err(err) => {
            error!(
                "Failed to retrieve LSX auth code for '{}': {}",
                client_id, err
            );

            String::from("invalid")
        }
    };

    make_lsx_handler_response!(Response, AuthCode, { attr_value: auth_code })
}
