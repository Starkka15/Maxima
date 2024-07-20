#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//extern crate windows_service;

use std::env::current_exe;
use tokio::process::Command;

use anyhow::{bail, Result};

use base64::{engine::general_purpose, Engine};
use maxima::core::launch::BootstrapLaunchArgs;
use url::Url;

#[cfg(windows)]
use maxima::util::service::{is_service_valid, register_service};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
#[tokio::main]
async fn main() -> Result<()> {
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

async fn handle_launch_args() -> Result<bool> {
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0);

    let result = run(&args).await;
    if cfg!(debug_assertions) || std::env::var("MAXIMA_DEBUG").is_ok() {
        println!("Args: {:?}", &args);

        let str_result = result
            .as_ref()
            .map_err(|e| {
                let source = e.source();
                let error_str = if source.is_some() {
                    source.unwrap().to_string()
                } else {
                    e.to_string()
                };

                error_str + "\n" + &e.backtrace().to_string()
            })
            .err()
            .unwrap_or("Success".to_string());
        println!("Result: {}", str_result);

        // Pause terminal
        //std::io::Read::read(&mut std::io::stdin(), &mut [0]).unwrap();
    }

    result
}

#[cfg(windows)]
fn service_setup() -> Result<()> {
    if is_service_valid()? {
        return Ok(());
    }

    register_service()?;

    Ok(())
}

#[cfg(not(windows))]
fn service_setup() -> Result<()> {
    Ok(())
}

#[cfg(windows)]
async fn platform_launch(args: BootstrapLaunchArgs) -> Result<()> {
    let mut binding = Command::new(args.path);
    let child = binding.args(args.args);

    let status = child.spawn()?.wait().await?;
    bail!("{}", status.code().unwrap());
}

#[cfg(unix)]
async fn platform_launch(args: BootstrapLaunchArgs) -> Result<()> {
    use maxima::unix::wine::run_wine_command;
    use maxima::unix::wine::CommandType;

    run_wine_command(
        args.path,
        Some(args.args),
        None,
        false,
        CommandType::WaitForExitAndRun,
    ).await?;

    Ok(())
}

async fn run(args: &[String]) -> Result<bool> {
    let len = args.len();
    if len == 1 {
        let arg = &args[0];

        if arg == "--noop" {
            return Ok(true);
        }

        if arg.starts_with("link2ea") {
            // TODO
            bail!("link2ea not yet implemented!");
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
