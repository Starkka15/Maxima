use egui::Context;
use log::debug;
use maxima::{core::LockedMaxima, rtm::client::BasicPresence};
use std::sync::mpsc::Sender;

use crate::{
    bridge_thread::{BackendError, InteractThreadFriendListResponse, MaximaLibResponse},
    ui_image::UIImageCacheLoaderCommand,
    views::friends_view::UIFriend,
};

pub async fn get_friends_request(
    maxima_arc: LockedMaxima,
    channel: Sender<MaximaLibResponse>,
    remote_provider_channel: Sender<UIImageCacheLoaderCommand>,
    ctx: &Context,
) -> Result<(), BackendError> {
    debug!("received request to load friends");
    let maxima = maxima_arc.lock().await;
    let logged_in = maxima.auth_storage().lock().await.current().is_some();
    if !logged_in {
        return Err(BackendError::LoggedOut);
    }

    let friends = maxima.friends(0).await?;
    for friend in friends {
        remote_provider_channel.send(UIImageCacheLoaderCommand::ProvideRemote(
            crate::ui_image::UIImageType::Avatar(friend.id().to_string()),
            friend.avatar().as_ref().unwrap().medium().path().to_string(),
        ))?;
        let friend_info = UIFriend {
            name: friend.display_name().to_string(),
            id: friend.id().to_string(),
            online: BasicPresence::Offline,
            game: None,
            game_presence: None,
        };

        let res = MaximaLibResponse::FriendInfoResponse(InteractThreadFriendListResponse {
            friend: friend_info,
        });
        channel.send(res)?;

        ctx.request_repaint();
    }

    Ok(())
}
