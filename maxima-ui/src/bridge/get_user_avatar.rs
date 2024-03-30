use anyhow::{Ok, Result};
use egui::Context;
use std::sync::mpsc::Sender;

use crate::{
    bridge_thread::{MaximaLibResponse, InteractThreadUserAvatarResponse},
    ui_image::UIImage
};

pub async fn get_user_avatar_request(
    channel: Sender<MaximaLibResponse>,
    id: String,
    url: String,
    ctx: &Context,
) -> Result<()> {
    let image = UIImage::load_friend(id.clone(), url, ctx.clone()).await;
    let _ = channel.send(MaximaLibResponse::UserAvatarResponse(InteractThreadUserAvatarResponse {
        id,
        response: if image.is_err() {
            Err(image.err().unwrap())
        } else {
            Ok(image?.into())
        },
    }));

    Ok(())
}