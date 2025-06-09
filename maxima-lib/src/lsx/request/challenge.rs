use log::{debug, info};

use crate::{
    lsx::{
        connection::LockedConnectionState,
        request::LSXRequestError,
        types::{LSXChallengeAccepted, LSXChallengeResponse, LSXResponseType},
    },
    make_lsx_handler_response,
    util::simple_crypto::{check_challenge_response, make_challenge_response, make_lsx_key},
};

pub async fn handle_challenge_response(
    state: LockedConnectionState,
    message: LSXChallengeResponse,
) -> Result<Option<LSXResponseType>, LSXRequestError> {
    let valid = check_challenge_response(&message.attr_response, &state.read().await.challenge());
    if !valid {
        return Err(LSXRequestError::InvalidChallengeResponse);
    }

    let accept_key = make_challenge_response(&message.attr_key);
    let accept_key_bytes = accept_key.as_bytes();
    let seed = match message.attr_version.as_str() {
        "2" => 0,
        "3" => ((accept_key_bytes[0] as u16) << 8) | (accept_key_bytes[1]) as u16,
        _ => return Err(LSXRequestError::UnknownEncryption(message.attr_version)),
    };

    info!(
        "Game Connected - Name: {}, Offer ID: {}, Multiplayer Id: {}",
        message.title, message.content_id, message.multiplayer_id
    );

    let encryption_key = make_lsx_key(seed);
    state.write().await.enable_encryption(encryption_key);

    debug!(
        "Encryption key: {}, version: {}",
        hex::encode(encryption_key),
        message.attr_version
    );
    make_lsx_handler_response!(Response, ChallengeAccepted, { attr_response: accept_key })
}
