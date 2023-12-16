#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use strum_macros::IntoStaticStr;

use crate::core::{settings::MaximaSetting, ecommerce::{CommerceEntitlementType, CommerceEntitlementSource}};

macro_rules! lsx_message {
    (
        $(#[$message_attr:meta])*
        $message_name:ident;
        attr {
            $(
                $(#[$attr_field_attr:meta])*
                $attr_field:ident: $attr_field_type:ty
            ),* $(,)?
        },
        data {
            $(
                $(#[$field_attr:meta])*
                $field:ident: $field_type:ty
            ),* $(,)?
        }
    ) => {
        paste::paste! {
            // Main struct definition
            $(#[$message_attr])*
            #[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
            #[serde(rename_all = "PascalCase")]
            pub struct [<LSX $message_name>] {
                $(
                    $(#[$attr_field_attr])*
                    #[serde(rename = "@" $attr_field)]
                    pub [<attr_ $attr_field>]: $attr_field_type,
                )*
                $(
                    $(#[$field_attr])*
                    pub $field: $field_type,
                )*
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LSX {
    #[serde(rename = "$value")]
    pub value: LSXMessageType,
}

// Types of LSX messages
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum LSXMessageType {
    Event(LSXEvent),
    Request(LSXRequest),
    Response(LSXResponse),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LSXEvent {
    #[serde(rename = "@sender")]
    pub sender: String,
    #[serde(rename = "$value")]
    pub value: LSXEventType,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LSXRequest {
    #[serde(rename = "@recipient")]
    pub recipient: String,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "$value")]
    pub value: LSXRequestType,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LSXResponse {
    #[serde(rename = "@sender")]
    pub sender: String,
    #[serde(rename = "@id")]
    pub id: String,
    #[serde(rename = "$value")]
    pub value: LSXResponseType,
}

// All LSX messages per type
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum LSXEventType {
    Challenge(LSXChallenge),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, IntoStaticStr)]
pub enum LSXRequestType {
    ChallengeResponse(LSXChallengeResponse),
    GetConfig(LSXGetConfig),
    GetProfile(LSXGetProfile),
    GetSetting(LSXGetSetting),
    RequestLicense(LSXRequestLicense),
    GetGameInfo(LSXGetGameInfo),
    GetAllGameInfo(LSXGetAllGameInfo),
    GetInternetConnectedState(LSXGetInternetConnectedState),
    IsProgressiveInstallationAvailable(LSXIsProgressiveInstallationAvailable),
    AreChunksInstalled(LSXAreChunksInstalled),
    GetAuthCode(LSXGetAuthCode),
    GetPresence(LSXGetPresence),
    SetPresence(LSXSetPresence),
    QueryPresence(LSXQueryPresence),
    QueryFriends(LSXQueryFriends),
    QueryEntitlements(LSXQueryEntitlements),
    QueryImage(LSXQueryImage),
    GetVoipStatus(LSXGetVoipStatus),
    ShowIGOWindow(LSXShowIGOWindow),
    SetDownloaderUtilization(LSXSetDownloaderUtilization)
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum LSXResponseType {
    ErrorSuccess(LSXErrorSuccess),
    ChallengeAccepted(LSXChallengeAccepted),
    GetConfigResponse(LSXGetConfigResponse),
    GetProfileResponse(LSXGetProfileResponse),
    GetSettingResponse(LSXGetSettingResponse),
    RequestLicenseResponse(LSXRequestLicenseResponse),
    GetGameInfoResponse(LSXGetGameInfoResponse),
    GetAllGameInfoResponse(LSXGetAllGameInfoResponse),
    InternetConnectedState(LSXInternetConnectedState),
    IsProgressiveInstallationAvailableResponse(LSXIsProgressiveInstallationAvailableResponse),
    AreChunksInstalledResponse(LSXAreChunksInstalledResponse),
    AuthCode(LSXAuthCode),
    GetPresenceResponse(LSXGetPresenceResponse),
    QueryPresenceResponse(LSXQueryPresenceResponse),
    QueryFriendsResponse(LSXQueryFriendsResponse),
    QueryEntitlementsResponse(LSXQueryEntitlementsResponse),
    QueryImageResponse(LSXQueryImageResponse),
    GetVoipStatusResponse(LSXGetVoipStatusResponse),
}

pub fn create_lsx_message(r#type: LSXMessageType) -> LSX {
    LSX { value: r#type }
}

#[macro_export]
macro_rules! make_lsx_handler_response {
    ($reply_type:ty, $reply_name:ident, $reply_initializer:tt) => {
        paste::paste! {
            anyhow::Ok(Some([<LSX $reply_type Type>]::$reply_name(
                [<LSX $reply_name>] $reply_initializer
            )))
        }
    };
}

// Event Messages

lsx_message! {
    Challenge;
    attr {
        build: String,
        key: String,
        version: String,
    },
    data {}
}

// Request Messages

lsx_message! {
    ChallengeResponse;
    attr {
        response: String,
        key: String,
        version: String,
    },
    data {
        content_id: String,
        title: String,
        multiplayer_id: String,
        language: String,
        version: String,
    }
}

lsx_message! {
    GetConfig;
    attr {
        version: String,
    },
    data {}
}

lsx_message! {
    GetProfile;
    attr {
        index: u8,
        version: String,
    },
    data {}
}

lsx_message! {
    GetSetting;
    attr {
        SettingId: MaximaSetting,
    },
    data {}
}

lsx_message! {
    GetInternetConnectedState;
    attr {
        version: String,
    },
    data {}
}

lsx_message! {
    RequestLicense;
    attr {
        UserId: u64,
        RequestTicket: String,
        TicketEngine: String,
        version: String,
    },
    data {}
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum LSXGameInfoId {
    #[default]
    FreeTrial,
    Languages,
    #[serde(rename = "INSTALLED_LANGUAGE")]
    InstalledLanguage,
}

lsx_message! {
    GetGameInfo;
    attr {
        GameInfoId: LSXGameInfoId,
        version: String,
    },
    data {}
}

lsx_message! {
    GetAllGameInfo;
    attr {
        version: String,
    },
    data {}
}

lsx_message! {
    IsProgressiveInstallationAvailable;
    attr {
        ItemId: String,
        version: String,
    },
    data {}
}

lsx_message! {
    AreChunksInstalled;
    attr {
        ItemId: String,
        version: String,
    },
    data {
        chunk_ids: Vec<u32>,
    }
}

lsx_message! {
    GetAuthCode;
    attr {
        UserId: Option<String>,
        ClientId: String,
        Scope: Option<String>,
        AppendAuthSource: Option<String>,
        version: String,
    },
    data {}
}

lsx_message! {
    GetPresence;
    attr {
        UserId: u64,
    },
    data {}
}

lsx_message! {
    SetPresence;
    attr {
        UserId: u64,
        Presence: LSXPresence,
        RichPresence: Option<String>,
        GamePresence: Option<String>,
        SessionId: Option<String>,
    },
    data {}
}

lsx_message! {
    QueryPresence;
    attr {
        UserId: u64,
    },
    data {
        Users: Vec<u64>,
    }
}

lsx_message! {
    QueryFriends;
    attr {
        UserId: u64,
    },
    data {}
}

lsx_message!(
    QueryEntitlements;
    attr {
        UserId: u64,
        Group: String,
    },
    data {}
);

lsx_message! {
    QueryImage;
    attr {
        ImageId: String,
        Width: u16,
        Height: u16,
    },
    data {}
}

lsx_message! {
    GetVoipStatus;
    attr {},
    data {}
}

lsx_message! {
    SetDownloaderUtilization;
    attr {
        Utilization: f32
    },
    data {}
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LSXIGOWindow {
    #[default]
    Login,
    Profile,
    Recent,
    Feedback,
    Friends,
    FriendRequest,
    Chat,
    ComposeChat,
    Invite,
    Achievements,
    Store,
    CodeRedemption,
    Checkout,
    Blocked,
    Browser,
    FindFriends,
    ChangeAvatar,
    Gamedetails,
    Broadcast,
    Upsell
}

lsx_message! {
    ShowIGOWindow;
    attr {
        WindowId: LSXIGOWindow,
        Show: Option<bool>,
        Flags: Option<i32>,
        ContentId: String,
    },
    data {
        target_id: u64,
    }
}

// Response Messages

lsx_message! {
    ErrorSuccess;
    attr {
        Code: i64,
        Description: String,
    },
    data {}
}

lsx_message! {
    ChallengeAccepted;
    attr {
        response: String,
    },
    data {}
}

lsx_message! {
    Service;
    attr {
        Name: String,
        Facility: String,
    },
    data {}
}

impl LSXService {
    pub fn new(name: &str, facility: &str) -> Self {
        Self {
            attr_Name: name.to_string(),
            attr_Facility: facility.to_string(),
        }
    }
}

lsx_message! {
    GetConfigResponse;
    attr {},
    data {
        service: Vec<LSXService>,
    }
}

lsx_message! {
    GetSettingResponse;
    attr {
        Setting: String,
    },
    data {}
}

lsx_message! {
    GetProfileResponse;
    attr {
        Persona: String,
        SubscriberLevel: u8,
        CommerceCurrency: String,
        IsTrialSubscriber: bool,
        Country: String,
        UserId: u64,
        GeoCountry: String,
        AvatarId: String,
        IsSubscriber: bool,
        IsSteamSubscriber: bool,
        PersonaId: u64,
        IsUnderAge: bool,
        UserIndex: u8,
    },
    data {}
}

lsx_message! {
    RequestLicenseResponse;
    attr {
        License: String
    },
    data {}
}

lsx_message! {
    GetGameInfoResponse;
    attr {
        GameInfo: String
    },
    data {}
}

lsx_message! {
    GetAllGameInfoResponse;
    attr {
        FullGamePurchased: bool,
        FullGameReleased: bool,
        InstalledVersion: String,
        Languages: String,
        Expiration: String,
        UpToDate: bool,
        HasExpiration: bool,
        EntitlementSource: String,
        AvailableVersion: String,
        MaxGroupSize: u32,
        DisplayName: String,
        FreeTrial: bool,
        InstalledLanguage: String,
        FullGameReleaseDate: String,
        SystemTime: String,
    },
    data {}
}

lsx_message! {
    InternetConnectedState;
    attr {
        connected: u8,
    },
    data {}
}

lsx_message! {
    IsProgressiveInstallationAvailableResponse;
    attr {
        ItemId: String,
        Available: bool,
    },
    data {}
}

lsx_message! {
    AreChunksInstalledResponse;
    attr {
        ItemId: String,
        Installed: bool,
    },
    data {
        chunk_ids: Vec<u32>,
    }
}

lsx_message! {
    AuthCode;
    attr {
        value: String,
    },
    data {}
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LSXPresence {
    #[default]
    Unknown,
    Offline,
    Online,
    Ingame,
    Busy,
    Idle,
    Joinable,
    JoinableInviteOnly,
}

lsx_message! {
    GetPresenceResponse;
    attr {
        UserId: u64,
        Presence: LSXPresence,
        Title: Option<String>,
        TitleId: Option<String>,
        MultiplayerId: Option<String>,
        RichPresence: Option<String>,
        GamePresence: Option<String>,
        SessionId: Option<String>,
        Group: Option<String>,
        GroupId: Option<String>,
    },
    data {}
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LSXFriendState {
    #[default]
    Unknown,
    None,
    Mutual,
    Invited,
    Declined,
    Request,
}

lsx_message! {
    Friend;
    attr {
        TitleId: String,
        MultiplayerId: String,
        Persona: String,
        RichPresence: String,
        GamePresence: String,
        Title: String,
        UserId: u64,
        PersonaId: String,
        AvatarId: String,
        Group: String,
        GroupId: String,
        Presence: LSXPresence,
        State: LSXFriendState,
    },
    data {}
}

lsx_message! {
    QueryPresenceResponse;
    attr {},
    data {
        friend: Vec<LSXFriend>
    }
}

lsx_message! {
    QueryFriendsResponse;
    attr {},
    data {
        friend: Vec<LSXFriend>
    }
}

lsx_message!(
    Entitlement;
    attr {
        LastModifiedDate: String,
        EntitlementId: u64,
        UseCount: u32,
        Version: u16,
        ItemId: String,
        ResourceId: String,
        GrantDate: String,
        Group: String,
        EntitlementTag: String,
        Type: CommerceEntitlementType,
        Expiration: String,
        Source: String,
    },
    data {}
);

lsx_message! {
    QueryEntitlementsResponse;
    attr {},
    data {
        entitlement: Vec<LSXEntitlement>,
    }
}

lsx_message! {
    Image;
    attr {
        ImageId: String,
        Width: u16,
        Height: u16,
        ResourcePath: String,
    },
    data {}
}

lsx_message! {
    QueryImageResponse;
    attr {
        Result: i32,
    },
    data {
        image: Vec<LSXImage>,
    }
}

lsx_message! {
    GetVoipStatusResponse;
    attr {
        Available: bool,
        Active: bool,
    },
    data {}
}
