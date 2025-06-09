use crate::{
    core::settings::MaximaSetting,
    lsx::{
        connection::LockedConnectionState,
        request::LSXRequestError,
        types::{
            LSXGetInternetConnectedState, LSXGetSetting, LSXGetSettingResponse,
            LSXInternetConnectedState, LSXResponseType, LSXSetDownloaderUtilization,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_settings_request(
    _: LockedConnectionState,
    request: LSXGetSetting,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let setting = match request.attr_SettingId {
        MaximaSetting::IsIgoEnabled => "false".to_string(),
        MaximaSetting::IsIgoAvailable => "false".to_string(),
        MaximaSetting::Environment => "production".to_string(),
    };

    make_lsx_handler_response!(Response, GetSettingResponse, { attr_Setting: setting })
}

pub async fn handle_connectivity_request(
    _: LockedConnectionState,
    _: LSXGetInternetConnectedState,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    // TODO Actually check this
    make_lsx_handler_response!(Response, InternetConnectedState, { attr_connected: 1 })
}

pub async fn handle_set_downloader_util_request(
    _: LockedConnectionState,
    _: LSXSetDownloaderUtilization,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    // TODO Actually set this
    Ok(None)
}
