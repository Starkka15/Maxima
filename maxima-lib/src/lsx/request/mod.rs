pub mod account;
pub mod auth;
pub mod challenge;
pub mod config;
pub mod core;
pub mod game;
pub mod igo;
pub mod license;
pub mod offer;
pub mod profile;
pub mod progressive_install;
pub mod voip;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LSXRequestError {
    #[error(transparent)]
    Auth(#[from] crate::core::auth::storage::AuthError),
    #[error(transparent)]
    ServiceLayer(#[from] crate::core::service_layer::ServiceLayerError),
    #[error(transparent)]
    Native(#[from] crate::util::native::NativeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),
    #[error(transparent)]
    Cache(#[from] crate::core::error::CacheRetrievalError),
    #[error(transparent)]
    AuthToken(#[from] crate::core::auth::storage::TokenError),
    #[error(transparent)]
    License(#[from] crate::ooa::LicenseError),
    #[error(transparent)]
    Rtm(#[from] crate::rtm::RtmError),
    #[error(transparent)]
    Infallible(#[from] std::convert::Infallible),
    #[error(transparent)]
    ECommerce(#[from] crate::core::ecommerce::ECommerceError),

    #[error("invalid LSX challenge response")]
    InvalidChallengeResponse,
    #[error("unknown LSX encryption version ({0})")]
    UnknownEncryption(String),
    #[error("failed to retrieve Denuvo token")]
    Denuvo,
}
