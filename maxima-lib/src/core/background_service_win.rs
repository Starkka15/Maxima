use dll_syringe::{process::OwnedProcess, Syringe};
use log::debug;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::core::error::BackgroundServiceClientError;
use crate::util::native::NativeError;
use crate::util::registry::{set_up_registry, RegistryError};
use is_elevated::is_elevated;

pub const BACKGROUND_SERVICE_PORT: u16 = 13021;

#[derive(Default, Serialize, Deserialize)]
pub struct ServiceLibraryInjectionRequest {
    pub pid: u32,
    pub path: String,
}

pub async fn request_library_injection(
    pid: u32,
    path: &str,
) -> Result<(), BackgroundServiceClientError> {
    debug!("Injecting {}", path);

    if is_elevated() {
        let process = OwnedProcess::from_pid(pid)?;
        let syringe = Syringe::for_process(process);
        syringe.inject(path)?;
        return Ok(());
    }

    let request = &ServiceLibraryInjectionRequest {
        pid,
        path: path.to_owned(),
    };

    let client = reqwest::Client::new();
    let res = client
        .post(format!(
            "http://127.0.0.1:{}/inject_library",
            BACKGROUND_SERVICE_PORT
        ))
        .body(serde_json::to_string(request)?)
        .send()
        .await?;
    if res.status() != StatusCode::OK {
        return Err(BackgroundServiceClientError::Request(res.text().await?));
    }

    Ok(())
}

pub async fn request_registry_setup() -> Result<(), BackgroundServiceClientError> {
    if is_elevated() {
        set_up_registry()?;
        return Ok(());
    }

    reqwest::get(format!(
        "http://127.0.0.1:{}/set_up_registry",
        BACKGROUND_SERVICE_PORT
    ))
    .await?;
    Ok(())
}
