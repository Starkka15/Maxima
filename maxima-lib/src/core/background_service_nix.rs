use anyhow::{bail, Result};
use base64::{engine::general_purpose, Engine};
use lazy_static::lazy_static;
use log::debug;
use regex::Regex;
use serde::Serialize;

use crate::unix::wine::CommandType;
use crate::{unix::wine::run_wine_command, util::native::module_path};

lazy_static! {
    static ref PID_PATTERN: Regex = Regex::new(r"wine-helper: PID (.*)").unwrap();
}

#[derive(Default, Serialize)]
pub struct WineGetPidArgs {
    pub launch_id: String,
    pub name: String,
}

#[derive(Default, Serialize)]
pub struct WineInjectArgs {
    pub pid: u32,
    pub path: String,
}

pub async fn wine_get_pid(launch_id: &str, name: &str) -> Result<u32> {
    debug!("Seaching for wine PID for {}", name);

    let launch_args = WineGetPidArgs {
        launch_id: launch_id.to_owned(),
        name: name.to_owned(),
    };

    let b64 = general_purpose::STANDARD.encode(serde_json::to_string(&launch_args).unwrap());
    let output = run_wine_command(
        module_path()
            .parent()
            .unwrap()
            .join("wine-helper.exe")
            .to_str()
            .unwrap(),
        Some(vec!["get_pid", b64.as_str()]),
        None,
        true,
        CommandType::RunInPrefix,
    ).await?;

    if output.contains("Failed to find PID") {
        bail!("Failed to find PID");
    }

    let pid = PID_PATTERN.captures(&output);
    if pid.is_none() {
        bail!("No PID pattern");
    }

    let pid = pid.unwrap().get(1).unwrap().as_str();
    Ok(pid.parse()?)
}

pub async fn request_library_injection(pid: u32, path: &str) -> Result<()> {
    debug!("Injecting {}", path);

    let launch_args = WineInjectArgs {
        pid,
        path: path.to_owned(),
    };

    let b64 = general_purpose::STANDARD.encode(serde_json::to_string(&launch_args).unwrap());
    run_wine_command(
        module_path()
            .parent()
            .unwrap()
            .join("wine-helper.exe")
            .to_str()
            .unwrap(),
        Some(vec!["inject", b64.as_str()]),
        None,
        false,
        CommandType::RunInPrefix,
    ).await?;

    Ok(())
}
