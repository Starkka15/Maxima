use log::info;

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

    let arc = state.write().await.maxima_arc();
    let mut maxima = arc.lock().await;

    let playing = maxima.playing().as_ref().unwrap();
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

    make_lsx_handler_response!(Response, RequestLicenseResponse, { attr_License: license.game_token.unwrap() })
}
