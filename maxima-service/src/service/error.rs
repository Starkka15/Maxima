use actix_web::{error, http::header::ContentType, HttpResponse};
use reqwest::StatusCode;
use thiserror::Error;
use windows_service::service;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error(transparent)]
    Inject(#[from] dll_syringe::error::InjectError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Native(#[from] maxima::util::native::NativeError),
    #[error(transparent)]
    Service(#[from] windows_service::Error),

    #[error("attempted to inject into invalid process")]
    InvalidInjectionTarget,
}

impl error::ResponseError for ServerError {
    fn status_code(&self) -> StatusCode {
        match self {
            ServerError::InvalidInjectionTarget => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(self.to_string())
    }
}
