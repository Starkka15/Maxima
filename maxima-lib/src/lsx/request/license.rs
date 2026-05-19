use log::{debug, info};
use std::env;

use crate::{
    core::{auth::hardware::HardwareInfo, launch::LaunchMode},
    lsx::{
        connection::LockedConnectionState,
        request::LSXRequestError,
        types::{LSXRequestLicense, LSXRequestLicenseResponse, LSXResponseType},
    },
    make_lsx_handler_response,
    ooa::{request_license, LicenseAuth},
};

pub async fn handle_license_request(
    state: LockedConnectionState,
    request: LSXRequestLicense,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    info!("Requesting OOA License and Denuvo Token");

    if let Ok(token) = env::var("MAXIMA_DENUVO_TOKEN") {
        return make_lsx_handler_response!(Response, RequestLicenseResponse, { attr_License: token.to_owned() });
    }

    let arc = state.write().await.maxima_arc();
    let mut maxima = arc.lock().await;

    // When the game wasn't launched through Maxima (e.g. the user opened
    // Maxima UI / `maxima-cli serve` and then started TF2 via Steam or
    // Northstar mode), `maxima.playing()` is None — there is no
    // ActiveGameContext to consult for content_id or mode. Upstream this
    // unwrap-panics, killing the spawned LSX-request task and leaving the
    // game waiting forever for a response. Mirror the same defensive
    // pattern `handle_set_presence_request` already uses below: return an
    // empty `attr_License` so TF2 falls back to its on-disk `.dlf` (which
    // `request_and_save_license` deposited at `…/EA Services/License/`
    // during the prior `maxima-cli launch` run, if there was one) rather
    // than crashing the connection.
    let Some(playing) = maxima.playing().as_ref() else {
        info!("RequestLicense from external LSX (playing=None); returning empty token so the game falls back to its cached .dlf");
        return make_lsx_handler_response!(Response, RequestLicenseResponse, { attr_License: String::new() });
    };
    let content_id = playing.content_id().to_owned();
    let mode = playing.mode();

    let auth = match mode {
        LaunchMode::Offline(_) => {
            return make_lsx_handler_response!(Response, RequestLicenseResponse, { attr_License: String::new() });
        }
        LaunchMode::Online(_) => LicenseAuth::AccessToken(maxima.access_token().await?),
        LaunchMode::OnlineOffline(_, persona, password) => {
            LicenseAuth::Direct(persona.to_owned(), password.to_owned())
        }
    };

    // TODO: how to get version
    let hw_info = HardwareInfo::new(2);
    let license = request_license(
        &content_id,
        &hw_info.generate_hardware_hash(),
        &auth,
        Some(request.attr_RequestTicket.as_str()),
        Some(request.attr_TicketEngine.as_str()),
    )
    .await?;

    if license.game_token.is_none() {
        return Err(LSXRequestError::Denuvo);
    }

    info!("Successfully retrieved license tokens");

    let token = license.game_token.as_ref().unwrap();

    debug!("Got Denuvo Token: {}", token);

    make_lsx_handler_response!(Response, RequestLicenseResponse, { attr_License: token.to_owned() })
}
