use log::{debug, info};

use crate::core::service_layer::{
    ServiceFriends, ServiceGetMyFriendsRequestBuilder, ServiceLayerError,
    SERVICE_REQUEST_GETMYFRIENDS,
};
use crate::{
    lsx::{
        connection::LockedConnectionState,
        request::LSXRequestError,
        types::{
            LSXBlockedUser, LSXErrorSuccess, LSXFriend, LSXFriendState, LSXGetBlockList,
            LSXGetBlockListResponse, LSXGetPresence, LSXGetPresenceResponse, LSXGetProfile,
            LSXGetProfileResponse, LSXImage, LSXPresence, LSXQueryFriends, LSXQueryFriendsResponse,
            LSXQueryImage, LSXQueryImageResponse, LSXQueryPresence, LSXQueryPresenceResponse,
            LSXResponseType, LSXSetPresence,
        },
    },
    make_lsx_handler_response,
    rtm::client::{BasicPresence, RichPresenceBuilder},
    util::native::{platform_path, NativeError, SafeStr},
};

pub async fn handle_profile_request(
    state: LockedConnectionState,
    _: LSXGetProfile,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let arc = state.write().await.maxima_arc();
    let maxima = arc.lock().await;

    let user = maxima.local_user().await?;
    let path = platform_path(maxima.avatar_image(&user.id(), 208, 208).await?);

    let player = user
        .player()
        .as_ref()
        .ok_or(ServiceLayerError::MissingField)?;
    let name = player.unique_name();
    debug!("Got profile for {} {:?}", &name, path);

    make_lsx_handler_response!(Response, GetProfileResponse, {
       attr_Persona: name.to_owned(),
       attr_SubscriberLevel: 0,
       attr_CommerceCurrency: "USD".to_string(),
       attr_IsTrialSubscriber: false,
       attr_Country: "US".to_string(),
       attr_UserId: user.id().parse::<u64>()?,
       attr_GeoCountry: "US".to_string(),
       attr_AvatarId: path.safe_str()?.to_string(),
       attr_IsSubscriber: false,
       attr_IsSteamSubscriber: false,
       attr_PersonaId: player.psd().parse::<u64>()?,
       attr_IsUnderAge: false,
       attr_UserIndex: 0,
    })
}

pub async fn handle_presence_request(
    _: LockedConnectionState,
    _: LSXGetPresence,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
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
    state: LockedConnectionState,
    request: LSXSetPresence,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    info!(
        "Setting Presence to {:?}: {}",
        request.attr_Presence,
        request
            .attr_RichPresence
            .to_owned()
            .or(Some(String::new()))
            .unwrap()
    );

    let arc = state.write().await.maxima_arc();
    let mut maxima = arc.lock().await;

    let playing = maxima.playing().as_ref().unwrap();
    if playing.mode().is_online_offline() {
        return make_lsx_handler_response!(Response, ErrorSuccess, { attr_Code: 0, attr_Description: String::new() });
    }

    let offer = playing.offer().as_ref().unwrap().offer();
    let offer_id = offer.offer_id().to_owned();
    let name = offer.display_name().to_owned();

    if let Some(presence) = request.attr_RichPresence {
        maxima
            .rtm()
            .set_presence(
                BasicPresence::Online,
                &format!("{}: {}", name, presence),
                &offer_id,
            )
            .await?;
    }

    make_lsx_handler_response!(Response, ErrorSuccess, { attr_Code: 0, attr_Description: String::new() })
}

pub async fn handle_query_presence_request(
    state: LockedConnectionState,
    request: LSXQueryPresence,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let mut friends = Vec::new();

    let mut state = state.write().await;
    let mut maxima = state.maxima().await;
    let presence_store = maxima.rtm().presence_store().lock().await;

    for user in request.Users {
        let presence = match presence_store.get(&user.to_string()) {
            Some(p) => p,
            None => continue,
        };

        let game = if let Some(game) = presence.game() {
            game.to_owned()
        } else {
            String::new()
        };

        friends.push(LSXFriend {
            attr_TitleId: "".to_string(),
            attr_MultiplayerId: "".to_string(),
            attr_Persona: "------".to_string(),
            attr_RichPresence: presence.status().to_string(),
            attr_GamePresence: game,
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
    state: LockedConnectionState,
    _: LSXQueryFriends,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let mut state = state.write().await;
    let mut maxima = state.maxima().await;

    let friends = maxima.friends(0).await?;
    let presence_store = maxima.rtm().presence_store().lock().await;

    let mut lsx_friends = Vec::new();
    for ele in friends {
        if ele.relationship() != "FRIEND" {
            continue;
        }

        let mut presence = presence_store.get(ele.id()).unwrap_or_else(|| {
            RichPresenceBuilder::default()
                .basic(BasicPresence::Offline)
                .status(String::new())
                .game(None)
                .build()
                .unwrap()
        });

        let mut lsx_presence = match presence.basic() {
            BasicPresence::Unknown => LSXPresence::Unknown,
            BasicPresence::Offline => LSXPresence::Offline,
            BasicPresence::Dnd => LSXPresence::Busy,
            BasicPresence::Away => LSXPresence::Idle,
            BasicPresence::Online => LSXPresence::Online,
        };

        let game = if let Some(game) = presence.game() {
            game.to_owned()
        } else {
            String::new()
        };

        if !game.is_empty() {
            lsx_presence = LSXPresence::Ingame;
        }

        lsx_friends.push(LSXFriend {
            attr_TitleId: "".to_string(),
            attr_MultiplayerId: "".to_string(),
            attr_Persona: ele.unique_name().to_string(),
            attr_RichPresence: presence.status().to_string(),
            attr_GamePresence: game,
            attr_Title: "".to_string(),
            attr_UserId: ele.id().parse()?,
            attr_PersonaId: ele.pd().parse()?,
            attr_AvatarId: format!("user:{}", ele.id()).to_string(),
            attr_Group: "".to_string(),
            attr_GroupId: "".to_string(),
            attr_Presence: lsx_presence,
            attr_State: LSXFriendState::Mutual,
        });
    }

    make_lsx_handler_response!(Response, QueryFriendsResponse, { friend: lsx_friends })
}

pub async fn handle_get_block_list_request(
    state: LockedConnectionState,
    _: LSXGetBlockList,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let mut list: Vec<LSXBlockedUser> = Vec::new();

    let mut maxima = state.write().await;
    let maxima = maxima.maxima().await;
    let friends: ServiceFriends = maxima
        .service_layer()
        .request(
            SERVICE_REQUEST_GETMYFRIENDS,
            ServiceGetMyFriendsRequestBuilder::default()
                .limit(100)
                .offset(0)
                .is_mutual_friends_enabled(false)
                .build()
                .unwrap(),
        )
        .await?;

    for blocked in friends.blocked_players().items() {
        list.push(LSXBlockedUser {
            attr_UserId: blocked.pd().to_string(),
            attr_EAID: "".to_string(),
            attr_PersonaId: blocked.player_v2().psd().to_string(),
        });
    }

    make_lsx_handler_response!(Response, GetBlockListResponse, { attr_Return: "Success".to_string(), User: list})
}

pub async fn handle_query_image_request(
    state: LockedConnectionState,
    request: LSXQueryImage,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let parts = request.attr_ImageId.split(":").collect::<Vec<_>>();

    let arc = state.write().await.maxima_arc();
    let maxima = arc.lock().await;

    let path = platform_path(
        maxima
            .avatar_image(parts[1], request.attr_Width, request.attr_Height)
            .await?,
    );

    let mut images = Vec::new();
    images.push(LSXImage {
        attr_ImageId: request.attr_ImageId,
        attr_Width: request.attr_Width,
        attr_Height: request.attr_Height,
        attr_ResourcePath: path.safe_str()?.to_string(),
    });

    make_lsx_handler_response!(Response, QueryImageResponse, { attr_Result: 1, image: images, })
}
