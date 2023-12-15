use anyhow::{bail, Result};
use log::debug;

use crate::{
    lsx::{
        connection::Connection,
        types::{LSXChallengeAccepted, LSXChallengeResponse, LSXResponseType},
    },
    make_lsx_handler_response,
    util::simple_crypto::{check_challenge_response, make_lsx_key, make_challenge_response},
};

pub async fn handle_challenge_response(
    connection: &mut Connection,
    message: LSXChallengeResponse,
) -> Result<Option<LSXResponseType>> {
    let valid = check_challenge_response(&message.attr_response, &connection.challenge());
    if !valid {
        bail!("Invalid challenge response");
    }

    let accept_key = make_challenge_response(&message.attr_key);
    let accept_key_bytes = accept_key.as_bytes();
    let seed = match message.attr_version.as_str() {
        "2" => 0,
        "3" => ((accept_key_bytes[0] as u16) << 8) | (accept_key_bytes[1]) as u16,
        _ => bail!("Unknown LSX encryption version!"),
    };

    let encryption_key = make_lsx_key(seed);
    connection.enable_encryption(encryption_key);

    debug!(
        "Encryption key: {}, version: {}",
        hex::encode(encryption_key),
        message.attr_version
    );
    make_lsx_handler_response!(Response, ChallengeAccepted, { attr_response: accept_key })
}
