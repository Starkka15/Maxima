use anyhow::Result;

use crate::{
    lsx::{
        connection::Connection,
        types::{
            LSXAreChunksInstalled, LSXAreChunksInstalledResponse,
            LSXIsProgressiveInstallationAvailable, LSXIsProgressiveInstallationAvailableResponse,
            LSXResponseType,
        },
    },
    make_lsx_handler_response,
};

pub async fn handle_pi_availability_request(
    _: &mut Connection,
    _: LSXIsProgressiveInstallationAvailable,
) -> Result<Option<LSXResponseType>> {
    make_lsx_handler_response!(Response, IsProgressiveInstallationAvailableResponse, {
        attr_Available: false,
        attr_ItemId: "Origin.OFR.50.0001456".to_string(),
    })
}

pub async fn handle_pi_installed_chunks_request(
    _: &mut Connection,
    request: LSXAreChunksInstalled,
) -> Result<Option<LSXResponseType>> {
    make_lsx_handler_response!(Response, AreChunksInstalledResponse, {
        attr_ItemId: "Origin.OFR.50.0001456".to_string(),
        attr_Installed: true,
        chunk_ids: request.chunk_ids,
    })
}
