use anyhow::Result;
use lazy_static::lazy_static;

use crate::{
    lsx::{
        connection::Connection,
        types::{LSXGetConfig, LSXGetConfigResponse, LSXResponseType, LSXService},
    },
    make_lsx_handler_response,
};

lazy_static! {
    static ref SERVICES: Vec<LSXService> = vec![
        LSXService::new("EbisuSDK", "SDK"),
        LSXService::new("EbisuSDK", "PROFILE"),
        LSXService::new("XMPP", "PRESENCE"),
        LSXService::new("XMPP", "FRIENDS"),
        LSXService::new("Commerce", "COMMERCE"),
        LSXService::new("EbisuSDK", "RECENTPLAYER"),
        LSXService::new("EbisuSDK", "IGO"),
        LSXService::new("EbisuSDK", "MISC"),
        LSXService::new("EALS", "LOGIN"),
        LSXService::new("Utility", "UTILITY"),
        LSXService::new("XMPP", "XMPP"),
        LSXService::new("XMPP", "CHAT"),
        LSXService::new("EbisuSDK", "IGO_EVENT"),
        LSXService::new("EALS", "EALS_EVENTS"),
        LSXService::new("EbisuSDK", "LOGIN_EVENT"),
        LSXService::new("XMPP", "INVITE_EVENT"),
        LSXService::new("EbisuSDK", "PROFILE_EVENT"),
        LSXService::new("XMPP", "PRESENCE_EVENT"),
        LSXService::new("XMPP", "FRIENDS_EVENT"),
        LSXService::new("Commerce", "COMMERCE_EVENT"),
        LSXService::new("XMPP", "CHAT_EVENT"),
        LSXService::new("EbisuSDK", "DOWNLOAD_EVENT"),
        LSXService::new("EbisuSDK", "PERMISSION"),
        LSXService::new("EbisuSDK", "RESOURCES"),
        LSXService::new("EbisuSDK", "BLOCKED_USERS"),
        LSXService::new("EbisuSDK", "BLOCKED_USER_EVENT"),
        LSXService::new("EbisuSDK", "GET_USERID"),
        LSXService::new("EbisuSDK", "ONLINE_STATUS_EVENT"),
        LSXService::new("EbisuSDK", "ACHIEVEMENT"),
        LSXService::new("EbisuSDK", "ACHIEVEMENT_EVENT"),
        LSXService::new("EbisuSDK", "BROADCAST_EVENT"),
        LSXService::new("PI", "PROGRESSIVE_INSTALLATION"),
        LSXService::new("PI", "PROGRESSIVE_INSTALLATION_EVENT"),
        LSXService::new("EbisuSDK", "CONTENT"),
    ];
}

pub async fn handle_config_request(
    _: &mut Connection,
    _: LSXGetConfig,
) -> Result<Option<LSXResponseType>> {
    let mut services: Vec<LSXService> = Vec::new();
    for service in SERVICES.iter() {
        services.push(service.clone());
    }
 
    make_lsx_handler_response!(Response, GetConfigResponse, { service: services })
}
