const LANGUAGES: &str =
    "ar_SA,de_DE,en_US,es_ES,es_MX,fr_FR,it_IT,ja_JP,ko_KR,pl_PL,pt_BR,ru_RU,zh_CN,zh_TW";
//const LANGUAGES: &str = "de_DE,en_US,es_ES,es_MX,fr_FR,it_IT,ja_JP,pl_PL,pt_BR,ru_RU,zh_TW";
//const LANGUAGES: &str = "en_US,es_ES,fr_FR,pt_BR";

use crate::{
    lsx::{
        connection::LockedConnectionState,
        request::LSXRequestError,
        types::{
            LSXGameInfoId, LSXGetAllGameInfo, LSXGetAllGameInfoResponse, LSXGetGameInfo,
            LSXGetGameInfoResponse, LSXResponseType,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_game_info_request(
    _: LockedConnectionState,
    request: LSXGetGameInfo,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let game_info = match request.attr_GameInfoId {
        LSXGameInfoId::FreeTrial => "false".to_string(),
        LSXGameInfoId::Languages => LANGUAGES.to_string(),
        LSXGameInfoId::InstalledLanguage => "en_US".to_string(),
    };

    make_lsx_handler_response!(Response, GetGameInfoResponse, { attr_GameInfo: game_info })
}

// Sample EA Desktop response (from a real Battlefield V LSX trace) for
// reference of the field shape — DO NOT use these values, they're stale:
// <GetAllGameInfoResponse FullGamePurchased="true" FullGameReleased="true"
//   InstalledVersion="0" MaxGroupSize="16" Languages="..."
//   Expiration="0000-00-00T00:00:00" UpToDate="true" HasExpiration="false"
//   InstalledLanguage="" EntitlementSource="STEAM"
//   FullGameReleaseDate="2020-10-22T09:00:00" AvailableVersion="1.0.64.43203"
//   DisplayName="Battlefield V Definitive Edition" FreeTrial="false"
//   SystemTime="2023-06-23T04:22:10"/>

/// Handles `GetAllGameInfo` — the LSX request the game uses to verify that
/// the auth server's view of "what's installed" matches what's on disk.
///
/// CRITICAL: `InstalledVersion` and `AvailableVersion` must match the client's
/// own version, or TF2 (and similar Source games) raises an "Engine Error:
/// File corruption detected" dialog and exits. The version arrives in the
/// `<Version>` element of the LSX challenge handshake, which we capture into
/// connection state (`set_game_metadata`) so we can echo it back here.
///
/// Older upstream code hardcoded `InstalledVersion="0"` / `AvailableVersion="1.0.1.3"`
/// which worked with old client builds but breaks current TF2 (9.12.1.3).
pub async fn handle_all_game_info_request(
    state: LockedConnectionState,
    _: LSXGetAllGameInfo,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let (version, title) = {
        let s = state.read().await;
        (
            s.game_version()
                .clone()
                .unwrap_or_else(|| "1.0.1.3".to_string()),
            s.game_title()
                .clone()
                .unwrap_or_else(|| "Titanfall® 2 Deluxe Edition".to_string()),
        )
    };

    // EntitlementSource must agree with `IsSteamSubscriber` in
    // `GetProfileResponse` — TF2's DRM stub treats any contradiction
    // (e.g. "STEAM" + IsSteamSubscriber=false) as a tamper signal and
    // shows "Engine Error: File corruption detected". Both are now
    // sourced from `ActiveGameContext.steam_app_id` (the original
    // Steam App ID that triggered this launch, if any).
    let entitlement_source: String = {
        let arc = state.write().await.maxima_arc();
        let maxima = arc.lock().await;
        let is_steam = maxima
            .playing()
            .as_ref()
            .and_then(|p| p.steam_app_id().as_ref())
            .is_some();
        if is_steam {
            "STEAM".to_string()
        } else {
            "EA".to_string()
        }
    };

    make_lsx_handler_response!(Response, GetAllGameInfoResponse, {
        attr_FullGamePurchased: true,
        attr_FullGameReleased: true,
        attr_InstalledVersion: version.clone(),
        attr_MaxGroupSize: 16,
        attr_Languages: LANGUAGES.to_string(),
        attr_Expiration: "0000-00-00T00:00:00".to_string(),
        attr_UpToDate: true,
        attr_HasExpiration: false,
        attr_EntitlementSource: entitlement_source,
        attr_AvailableVersion: version,
        attr_DisplayName: title,
        attr_FreeTrial: false,
        attr_InstalledLanguage: "en_US".to_string(),
        attr_FullGameReleaseDate: "2016-10-28T04:00:00".to_string(),
        attr_SystemTime: "2023-06-22T04:00:00".to_string()
    })
}
