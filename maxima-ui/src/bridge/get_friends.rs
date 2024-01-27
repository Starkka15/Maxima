use anyhow::{Ok, Result, bail};
use egui::Context;
use log::debug;
use maxima::core::{service_layer::{ServiceFriends, SERVICE_REQUEST_GETMYFRIENDS, ServiceGetMyFriendsRequestBuilder}, LockedMaxima};
use std::sync::mpsc::Sender;

use crate::{
    interact_thread::{MaximaLibResponse, InteractThreadFriendListResponse},
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
        bail!("Ignoring request to load games, not logged in.");
    }

    let response: ServiceFriends = maxima.service_layer().request(
        SERVICE_REQUEST_GETMYFRIENDS,
        ServiceGetMyFriendsRequestBuilder::default()
        .limit(100)
        .offset(0)
        .is_mutual_friends_enabled(false)
        .build()?,
    ).await?;

    for bitchass in response.friends().items() {

        let friend_info = UIFriend {
            name: bitchass.player().display_name().to_string(),
            id: bitchass.player().id().to_string(),
            online: true,
            game: Some("your mom".to_owned()),
            game_presence: None,
            avatar: UIFriendImageWrapper::Unloaded(bitchass.player().avatar().medium().path().to_string()),
        };

        let res = MaximaLibResponse::FriendInfoResponse(InteractThreadFriendListResponse {
            friend: friend_info,
        });
        channel.send(res)?;

        ctx.request_repaint();   
    }

    Ok(())
}