use anyhow::{Ok, Result};
use egui::Context;
use log::info;
use maxima::core::{
    auth::{context::AuthContext, execute_auth_exchange, nucleus_connect_token},
    clients::JUNO_PC_CLIENT_ID,
    Maxima,
};
use std::sync::{mpsc::Sender, Arc};
use tokio::sync::Mutex;

use crate::interact_thread::{InteractThreadLoginResponse, MaximaLibResponse};

pub async fn login_creds(
    maxima_arc: Arc<Mutex<Maxima>>,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
    user: String,
    pass: String,
) -> Result<()> {
    let maxima = maxima_arc.lock().await;
    let login_result = maxima::core::auth::login::manual_login(&user, &pass).await;
    if (&login_result).is_err() {
        let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
            success: false,
            description: {
                if let Some(e) = login_result.err() {
                    e.to_string()
                } else {
                    "Failed for an unknown reason".to_string()
                }
            },
        });

        channel.send(lmessage)?;
        return Ok(()); // it's not actually ok but that's not what we care about reporting to the bridge
    }

    let mut auth_context = AuthContext::new()?;
    auth_context.set_access_token(&login_result.unwrap());
    let code = execute_auth_exchange(&auth_context, JUNO_PC_CLIENT_ID, "code").await?;
    auth_context.set_code(&code);

    if auth_context.code().is_none() {
        let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
            success: false,
            description: "Failed for an unknown reason".to_string(),
        });
        channel.send(lmessage)?;
        return Ok(());
    }

    let token_res = nucleus_connect_token(&auth_context).await;

    if token_res.is_err() {
        let desc = token_res.err().unwrap().to_string();
        let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
            success: false,
            description: desc.clone(),
        });
        channel.send(lmessage)?;
        return Ok(());
    }

    {
        let mut auth_storage = maxima.auth_storage().lock().await;
        auth_storage.add_account(&token_res.unwrap()).await?;
    }

    let user = maxima.local_user().await?;
    let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
        success: true,
        description: user.player().as_ref().unwrap().display_name().to_owned(),
    });
    info!("Successfully logged in with username/password");
    channel.send(lmessage)?;
    egui::Context::request_repaint(&ctx);
    Ok(())
}
