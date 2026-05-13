#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//extern crate windows_service;

use std::env::current_exe;
use std::path::{Path, PathBuf};
use std::error::Error;
use std::string::FromUtf8Error;
use thiserror::Error;
use tokio::process::Command;

use base64::{engine::general_purpose, Engine};
use maxima::core::launch::BootstrapLaunchArgs;
use maxima::util::native::NativeError;
#[cfg(windows)]
use maxima::util::service::{is_service_valid, register_service};
use maxima::util::BackgroundServiceControlError;
use url::Url;

#[cfg(target_os = "macos")]
mod macos;

#[derive(Error, Debug)]
pub(crate) enum RunError {
    #[error(transparent)]
    BackgroundService(#[from] BackgroundServiceControlError),
    #[error(transparent)]
    Base64(#[from] base64::DecodeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    ParseUrl(#[from] url::ParseError),
    #[error(transparent)]
    ParseUtf8(#[from] FromUtf8Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

#[cfg(not(target_os = "macos"))]
#[tokio::main]
async fn main() -> Result<(), RunError> {
    // Immediate entry log
    if let Ok(temp_dir) = std::env::var("TEMP").map(PathBuf::from).or_else(|_| Ok::<PathBuf, RunError>(std::env::temp_dir())) {
        let debug_log = temp_dir.join("maxima_execution.log");
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&debug_log) {
            use std::io::Write;
            let _ = writeln!(file, "BOOTSTRAP MAIN START at {:?} | Raw Args: {:?}", std::time::SystemTime::now(), std::env::args().collect::<Vec<_>>());
        }
    }

    let _ = handle_launch_args().await?;

    Ok(())
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<()> {
    use cacao::appkit::App;

    use crate::macos::MaximaBootstrapApp;

    let handle = tokio::runtime::Handle::current();
    App::new(
        "dev.armchairdevelopers.MaximaBootstrap",
        MaximaBootstrapApp::new(handle),
    )
    .run();

    Ok(())
}

async fn handle_launch_args() -> Result<bool, RunError> {
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0);

    let result = run(&args).await;
    let str_result = result
        .as_ref()
        .map_err(|e| {
            let source = e.source();
            let error_str = if source.is_some() {
                source.unwrap().to_string()
            } else {
                e.to_string()
            };

            error_str
        })
        .err()
        .unwrap_or("Success".to_string());
        
    // Unconditional debug log to verify execution (APPEND)
    let temp_dir = std::env::temp_dir();
    let debug_log = temp_dir.join("maxima_execution.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&debug_log) {
        use std::io::Write;
        let _ = writeln!(file, "Maxima Bootstrap executed at {:?}\nArgs: {:?}\nResult: {}\n---", std::time::SystemTime::now(), args, str_result);
    }

    if str_result != "Success" {
        let log_path = temp_dir.join("maxima_bootstrap_error.log");
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write;
            let _ = writeln!(file, "Maxima Bootstrap Error at {:?}: {}", std::time::SystemTime::now(), str_result);
        }
        
        // Try a very simple path as well
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("C:\\maxima_debug_error.log") {
            use std::io::Write;
            let _ = writeln!(file, "Maxima Bootstrap Error at {:?}: {}", std::time::SystemTime::now(), str_result);
        }
    }

    if cfg!(debug_assertions) || std::env::var("MAXIMA_DEBUG").is_ok() {
        println!("Args: {:?}", &args);
        println!("Result: {}", str_result);

        // Pause terminal
        //std::io::Read::read(&mut std::io::stdin(), &mut [0]).unwrap();
    }

    result
}

#[cfg(windows)]
fn service_setup() -> Result<(), BackgroundServiceControlError> {
    if is_service_valid()? {
        return Ok(());
    }

    register_service()?;

    Ok(())
}

#[cfg(not(windows))]
fn service_setup() -> Result<(), BackgroundServiceControlError> {
    Ok(())
}

#[cfg(windows)]
async fn platform_launch(args: BootstrapLaunchArgs) -> Result<(), NativeError> {
    let mut binding = Command::new(&args.path);
    let child = binding.args(&args.args);

    let temp_dir = std::env::temp_dir();
    let debug_log = temp_dir.join("maxima_execution.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&debug_log) {
        use std::io::Write;
        let _ = writeln!(file, "PLATFORM_LAUNCH: Executing {:?} with args {:?}", args.path, args.args);
    }

    let status = child.spawn()?.wait().await?;
    if !status.success() {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Game exited with code: {:?}", status.code())).into());
    }
    Ok(())
}

#[cfg(unix)]
async fn platform_launch(args: BootstrapLaunchArgs) -> Result<(), NativeError> {
    use maxima::unix::wine::run_wine_command;
    use maxima::unix::wine::CommandType;

    run_wine_command(
        args.path,
        Some(args.args),
        None,
        false,
        CommandType::WaitForExitAndRun,
    )
    .await?;

    Ok(())
}

async fn run(args: &[String]) -> Result<bool, RunError> {
    let len = args.len();
    if len == 1 {
        let arg = &args[0];

        if arg == "--noop" {
            return Ok(true);
        }

        if arg.starts_with("link2ea") {
            // link2ea://launchgame/<offer-id>?platform=<p>&theme=<t>
            // link2ea://resume/<offer-id>?...
            let url = Url::parse(arg)?;

            // The offer ID is the first path segment after the host/action
            let segments: Vec<&str> = url
                .path_segments()
                .map(|c| c.collect())
                .unwrap_or_default();

            if segments.is_empty() {
                return Ok(false);
            }

            // segments[0] is the offer ID (e.g. "Origin.OFR.50.0002694")
            let offer_id = segments[0];

            let mut child = Command::new(current_exe()?.with_file_name("maxima-cli.exe"));

            // Forward environment variables from parent process
            if let Ok(port) = std::env::var("KYBER_INTERFACE_PORT") {
                child.env("KYBER_INTERFACE_PORT", port);
            }

            // Extract any command params from the query string
            if let Some(query) = url.query() {
                let params = querystring::querify(query);
                if let Some((_, cmd_params)) = params.iter().find(|(k, _)| *k == "cmdParams") {
                    child.env(
                        "MAXIMA_LAUNCH_ARGS",
                        urlencoding::decode(cmd_params)
                            .unwrap_or_default()
                            .into_owned()
                            .replace("\\\"", "\""),
                    );
                }
            }

            child.args(["launch", offer_id]);
            child.spawn()?.wait().await?;

            return Ok(true);
        }

        if arg.starts_with("origin2") {
            let url = Url::parse(arg)?;
            let query = querystring::querify(url.query().unwrap());
            let _offer_id = query.iter().find(|(x, _)| *x == "offerIds").unwrap().1;
            let cmd_params = query.iter().find(|(x, _)| *x == "cmdParams").unwrap().1;

            let mut child = Command::new(current_exe()?.with_file_name("maxima-cli.exe"));
            child.env(
                "MAXIMA_LAUNCH_ARGS",
                urlencoding::decode(cmd_params)?
                    .into_owned()
                    .replace("\\\"", "\""),
            );
            println!(
                "{}",
                urlencoding::decode(cmd_params)?
                    .into_owned()
                    .replace("\\\"", "\"")
            );
            child.env("KYBER_INTERFACE_PORT", "3005");
            child.args(["--mode", "launch", "--offer-id", "Origin.OFR.50.0002148"]);
            child.spawn()?.wait().await?;

            return Ok(true);
        }

        if arg.starts_with("qrc") {
            let query = arg.split("login_successful.html?").collect::<Vec<&str>>()[1];
            reqwest::get(format!("http://127.0.0.1:31033/auth?{}", query)).await?;

            return Ok(true);
        }

        return Ok(false);
    }

    if len > 1 {
        let command = &args[0];
        let handled = match command.as_str() {
            "launch" => {
                let decoded = general_purpose::STANDARD.decode(&args[1])?;
                let launch_args: BootstrapLaunchArgs = serde_json::from_slice(&decoded)?;
                platform_launch(launch_args).await?;

                true
            }
            _ => false,
        };
        return Ok(handled);
    }

    service_setup()?;

    Ok(false)
}
