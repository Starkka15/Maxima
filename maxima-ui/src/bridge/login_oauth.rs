use anyhow::{Ok, Result};
use egui::Context;
use maxima::{
    core::{
        auth::{context::AuthContext, login, nucleus_connect_token},
        Maxima,
    },
    util::native::take_foreground_focus,
};
use std::sync::{mpsc::Sender, Arc};
use tokio::sync::Mutex;

use crate::interact_thread::{InteractThreadLoginResponse, MaximaLibResponse};

pub async fn login_oauth(
    maxima_arc: Arc<Mutex<Maxima>>,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    {
        let mut auth_storage = maxima.auth_storage().lock().await;
        let mut context = AuthContext::new()?;
        login::begin_oauth_login_flow(&mut context).await?;
        let token_res = nucleus_connect_token(&context).await?;
        auth_storage.add_account(&token_res).await?;
    }

    let user = maxima.local_user().await?;
    let lmessage = MaximaLibResponse::LoginResponse(InteractThreadLoginResponse {
        success: true,
        description: user.player().as_ref().unwrap().display_name().to_owned(),
    });

    channel.send(lmessage)?;

    take_foreground_focus().unwrap();
    egui::Context::request_repaint(&ctx);
    Ok(())
}
