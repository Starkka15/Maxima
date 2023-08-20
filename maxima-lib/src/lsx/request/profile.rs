use anyhow::Result;
use log::{debug, info};

use crate::{
    core::background_service::request_library_injection,
    lsx::{
        connection::Connection,
        types::{
            LSXErrorSuccess, LSXFriend, LSXFriendState, LSXGetPresence, LSXGetPresenceResponse,
            LSXGetProfile, LSXGetProfileResponse, LSXImage, LSXPresence, LSXQueryFriends,
            LSXQueryFriendsResponse, LSXQueryImage, LSXQueryImageResponse, LSXQueryPresence,
            LSXQueryPresenceResponse, LSXResponseType, LSXSetPresence,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_profile_request(
    connection: &mut Connection,
    _: LSXGetProfile,
) -> Result<Option<LSXResponseType>> {
    let maxima = connection.get_maxima().await;
    let user = maxima.get_local_user().await?;

    let path = maxima.get_avatar_image(&user.id, 208, 208).await?;

    let player = user.player.unwrap();
    let name = player.unique_name;
    debug!("Got profile for {}", &name);

    make_lsx_handler_response!(Response, GetProfileResponse, {
       attr_Persona: name,
       attr_SubscriberLevel: 0,
       attr_CommerceCurrency: "USD".to_string(),
       attr_IsTrialSubscriber: false,
       attr_Country: "US".to_string(),
       attr_UserId: user.id.parse::<u64>()?,
       attr_GeoCountry: "US".to_string(),
       attr_AvatarId: path.to_str().unwrap().to_string(),
       attr_IsSubscriber: false,
       attr_IsSteamSubscriber: false,
       attr_PersonaId: player.psd.parse::<u64>()?,
       attr_IsUnderAge: false,
       attr_UserIndex: 0,
    })
}

pub async fn handle_presence_request(
    _: &mut Connection,
    _: LSXGetPresence,
) -> Result<Option<LSXResponseType>> {
    make_lsx_handler_response!(Response, GetPresenceResponse, {
       attr_UserId: 1005663144213,
       attr_Presence: LSXPresence::Ingame,
       attr_Title: None,
       attr_TitleId: None,
       attr_MultiplayerId: None,
       attr_RichPresence: None,
       attr_GamePresence: None,
       attr_SessionId: None,
       attr_Group: None,
       attr_GroupId: None,
    })
}

pub async fn handle_set_presence_request(
    connection: &mut Connection,
    request: LSXSetPresence,
) -> Result<Option<LSXResponseType>> {
    info!(
        "Setting Presence to {:?}: {}",
        request.attr_Presence,
        request
            .attr_RichPresence
            .or(Some("Unknown".to_string()))
            .unwrap()
    );

    if let Err(_) = std::env::var("MAXIMA_DISABLE_KYBER") {
        ureq::get(&format!("http://127.0.0.1:{}/initialize_renderer", std::env::var("KYBER_INTERFACE_PORT")?)).call()?;
    }

    make_lsx_handler_response!(Response, ErrorSuccess, { attr_Code: 0, attr_Description: String::new() })
}

pub async fn handle_query_presence_request(
    _: &mut Connection,
    request: LSXQueryPresence,
) -> Result<Option<LSXResponseType>> {
    let mut friends = Vec::new();

    for user in request.Users {
        friends.push(LSXFriend {
            attr_TitleId: "".to_string(),
            attr_MultiplayerId: "".to_string(),
            attr_Persona: "------".to_string(),
            attr_RichPresence: "".to_string(),
            attr_GamePresence: "".to_string(),
            attr_Title: "".to_string(),
            attr_UserId: user,
            attr_PersonaId: "0".to_string(),
            attr_AvatarId: "".to_string(),
            attr_Group: "".to_string(),
            attr_GroupId: "".to_string(),
            attr_Presence: LSXPresence::Ingame,
            attr_State: LSXFriendState::None,
        });
    }

    make_lsx_handler_response!(Response, QueryPresenceResponse, { friend: friends })
}

pub async fn handle_query_friends_request(
    _: &mut Connection,
    _: LSXQueryFriends,
) -> Result<Option<LSXResponseType>> {
    let friends = Vec::new();
    // TODO Populate friends with API
    make_lsx_handler_response!(Response, QueryFriendsResponse, { friend: friends })
}

pub async fn handle_query_image_request(
    connection: &mut Connection,
    request: LSXQueryImage,
) -> Result<Option<LSXResponseType>> {
    let parts = request.attr_ImageId.split(":").collect::<Vec<_>>();

    let maxima = connection.get_maxima().await;
    let path = maxima
        .get_avatar_image(parts[1], request.attr_Width, request.attr_Height)
        .await?;

    let mut images = Vec::new();

    // TODO Download and populate images
    images.push(LSXImage {
        attr_ImageId: request.attr_ImageId,
        attr_Width: request.attr_Width,
        attr_Height: request.attr_Height,
        attr_ResourcePath: path.to_str().unwrap().to_string(),
    });

    make_lsx_handler_response!(Response, QueryImageResponse, { attr_Result: 1, image: images, })
}
