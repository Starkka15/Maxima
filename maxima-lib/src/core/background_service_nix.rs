use anyhow::{bail, Result};
use base64::{engine::general_purpose, Engine};
use log::debug;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{
    unix::wine::run_wine_command,
    util::{native::module_path, registry::set_up_registry},
};

#[derive(Default, Serialize)]
pub struct InjectArgs {
    pub pid: u32,
    pub path: String,
}

pub async fn request_library_injection(pid: u32, path: &str) -> Result<()> {
    debug!("Injecting {}", path);

    let launch_args = InjectArgs {
        pid,
        path: path.to_owned(),
    };

    let b64 = general_purpose::STANDARD.encode(serde_json::to_string(&launch_args).unwrap());
    run_wine_command(
        "wine",
        module_path().parent().join("wine-injector.exe"),
        Some(vec![b64]),
    )?;

    Ok(())
}
