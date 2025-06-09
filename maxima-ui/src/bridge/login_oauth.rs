use crate::bridge_thread::{BackendError, InteractThreadLoginResponse, MaximaLibResponse};
use egui::Context;
use maxima::{
    core::{
        auth::{context::AuthContext, login, nucleus_token_exchange},
        service_layer::ServiceLayerError,
        LockedMaxima,
    },
    util::native::take_foreground_focus,
};
use std::sync::mpsc::Sender;

pub async fn login_oauth(
    maxima_arc: LockedMaxima,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
) -> Result<(), BackendError> {
    let maxima = maxima_arc.lock().await;

    {
        let mut auth_storage = maxima.auth_storage().lock().await;
        let mut context = AuthContext::new()?;
        login::begin_oauth_login_flow(&mut context).await?;
        let token_res = nucleus_token_exchange(&context).await?;
        auth_storage.add_account(&token_res).await?;
    }

    let user = maxima.local_user().await?;
    let message = MaximaLibResponse::LoginResponse(Ok(InteractThreadLoginResponse {
        you: user.player().as_ref().ok_or(ServiceLayerError::MissingField)?.to_owned(),
    }));

    channel.send(message)?;

    take_foreground_focus()?;
    ctx.request_repaint();
    Ok(())
}
