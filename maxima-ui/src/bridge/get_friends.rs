use anyhow::{Ok, Result, bail};
use egui::Context;
use log::{debug, info};
use maxima::{core::LockedMaxima, rtm::client::BasicPresence};
use std::sync::mpsc::Sender;

use crate::{
    bridge_thread::{MaximaLibResponse, InteractThreadFriendListResponse},
    views::friends_view::{UIFriend, UIFriendImageWrapper},
};

pub async fn get_friends_request(
    maxima_arc: LockedMaxima,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
) -> Result<()> {
    debug!("recieved request to load friends");
    let maxima = maxima_arc.lock().await;
    let logged_in = maxima.auth_storage().lock().await.current().is_some();
    if !logged_in {
        bail!("Ignoring request to load friends, not logged in.");
    }

    let friends = maxima.friends(0).await?;
    for bitchass in friends {

        let friend_info = UIFriend {
            name: bitchass.display_name().to_string(),
            id: bitchass.id().to_string(),
            online: BasicPresence::Offline,
            game: None,
            game_presence: None,
            avatar: UIFriendImageWrapper::Unloaded(bitchass.avatar().medium().path().to_string()),
        };

        let res = MaximaLibResponse::FriendInfoResponse(InteractThreadFriendListResponse {
            friend: friend_info,
        });
        channel.send(res)?;

        ctx.request_repaint();   
    }

    Ok(())
}