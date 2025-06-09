pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/eadp.rtm.rs"));
}

pub mod client;
pub mod connection;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RtmError {
    #[error(transparent)]
    Auth(#[from] crate::core::auth::storage::AuthError),
    #[error(transparent)]
    Decode(#[from] prost::DecodeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Send(#[from] tokio::sync::mpsc::error::SendError<connection::RtmRequest>),
    #[error(transparent)]
    Token(#[from] crate::core::auth::storage::TokenError),

    #[error("RTM error ({code}/{msg}: {body:?})", code = .0.error_message, msg = .0.error_message, body = .0.body)]
    V1(proto::ErrorV1),
    #[error("RTM login failed")]
    Login,
    #[error("RTM response had no body")]
    NoBody,
    #[error("invalid or missing RTM client version")]
    InvalidClientVersion,
    #[error("invalid RTM message variant `{0:?}`")]
    InvalidVariant(proto::communication_v1::Body),
    #[error("invalid RTM response variant `{0:?}`")]
    InvalidResponse(proto::success_v1::Body),
    #[error("got unhandled RTM update `{0:?}`")]
    UnhandledUpdate(proto::communication_v1::Body),
}
