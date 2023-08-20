use anyhow::Result;

const LANGUAGES: &str = "ar_SA,de_DE,en_US,es_ES,es_MX,fr_FR,it_IT,ja_JP,ko_KR,pl_PL,pt_BR,ru_RU,zh_CN,zh_TW";
//const LANGUAGES: &str = "de_DE,en_US,es_ES,es_MX,fr_FR,it_IT,ja_JP,pl_PL,pt_BR,ru_RU,zh_TW";
//const LANGUAGES: &str = "en_US,es_ES,fr_FR,pt_BR";

use crate::{
    lsx::{
        connection::Connection,
        types::{
            LSXGameInfoId, LSXGetAllGameInfo, LSXGetAllGameInfoResponse, LSXGetGameInfo,
            LSXGetGameInfoResponse, LSXResponseType,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_game_info_request(
    _: &mut Connection,
    request: LSXGetGameInfo,
) -> Result<Option<LSXResponseType>> {
    let game_info = match request.attr_GameInfoId {
        LSXGameInfoId::FreeTrial => "false".to_string(),
        LSXGameInfoId::Languages => LANGUAGES.to_string(),
        LSXGameInfoId::InstalledLanguage => "en_US".to_string(),
    };

    make_lsx_handler_response!(Response, GetGameInfoResponse, { attr_GameInfo: game_info })
}

// <GetAllGameInfoResponse FullGamePurchased="true" FullGameReleased="true" InstalledVersion="0" MaxGroupSize="16" Languages="ar_SA,de_DE,en_US,es_ES,es_MX,fr_FR,it_IT,ja_JP,ko_KR,pl_PL,pt_BR,ru_RU,zh_CN,zh_TW" Expiration="0000-00-00T00:00:00" UpToDate="true" HasExpiration="false" InstalledLanguage="" EntitlementSource="STEAM" FullGameReleaseDate="2020-10-22T09:00:00" AvailableVersion="1.0.64.43203" DisplayName="Battlefield V Definitive Edition" FreeTrial="false" SystemTime="2023-06-23T04:22:10"/>

pub async fn handle_all_game_info_request(
    _: &mut Connection,
    _: LSXGetAllGameInfo,
) -> Result<Option<LSXResponseType>> {
    make_lsx_handler_response!(Response, GetAllGameInfoResponse, {
        attr_FullGamePurchased: true,
        attr_FullGameReleased: true,
        attr_InstalledVersion: "0".to_string(),
        attr_MaxGroupSize: 16,
        attr_Languages: LANGUAGES.to_string(),
        attr_Expiration: "0000-00-00T00:00:00".to_string(),
        attr_UpToDate: true,
        attr_HasExpiration: false,
        attr_EntitlementSource: "STEAM".to_string(),
        attr_AvailableVersion: "1.0.1.3".to_string(),
        attr_DisplayName: "Titanfall® 2 Deluxe Edition".to_string(),
        attr_FreeTrial: false,
        attr_InstalledLanguage: "en_US".to_string(),
        attr_FullGameReleaseDate: "2016-10-28T04:00:00".to_string(),
        attr_SystemTime: "2023-06-22T04:00:00".to_string()
    })
}
