use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{create_dir_all, remove_dir_all, remove_file, File},
    io::Read,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    env,
};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use xz2::read::XzDecoder;
use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tar::Archive;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::Mutex,
    process::Command
};

use crate::util::{
    github::{fetch_github_release, fetch_github_releases, github_download_asset, GithubRelease},
    native::maxima_dir,
};

lazy_static! {
    static ref PROTON_PATTERN: Regex = Regex::new(r"GE-Proton\d+-\d+\.tar\.gz").unwrap();
}

// A Proton verb to use
pub enum CommandType {
    // Set the prefix up and runs the command
    Run,
    // Waits for any hanging wineserver instances and runs the command
    WaitForExitAndRun,
    // Directly calls the command, doesn't setup the prefix (use with caution)
    RunInPrefix,
}

impl std::fmt::Display for CommandType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            Self::RunInPrefix => "runinprefix",
            Self::Run => "run",
            Self::WaitForExitAndRun => "waitforexitandrun",
        };
        f.write_str(display)
    }
}

const VERSION_FILE: &str = "dependency-versions.toml";

#[derive(Deserialize, Default)]
struct LutrisRuntime {
    name: String,
    created_at: String,
    url: String
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Versions {
    proton: String,
    eac_runtime: String,
}

/// Returns internal prtoton pfx path
pub fn wine_prefix_dir() -> Result<PathBuf> {
    Ok(maxima_dir()?.join("wine/prefix"))
}

pub fn proton_dir() -> Result<PathBuf> {
    Ok(maxima_dir()?.join("wine/proton"))
}

pub fn eac_dir() -> Result<PathBuf> {
    Ok(maxima_dir()?.join("wine/eac_runtime"))
}


fn versions() -> Result<Versions> {
    let file = maxima_dir()?.join(VERSION_FILE);
    if !file.exists() {
        return Ok(Versions::default());
    }

    let data = std::fs::read_to_string(file)?;
    Ok(toml::from_str(&data).unwrap_or_default())
}

fn set_versions(versions: Versions) -> Result<()> {
    let file = maxima_dir()?.join(VERSION_FILE);
    std::fs::write(file, toml::to_string(&versions)?)?;
    Ok(())
}

pub(crate) async fn check_wine_validity() -> Result<bool> {
    if !proton_dir()?.exists() {
        return Ok(false);
    }

    let version = versions()?.proton;

    let release = get_wine_release();
    if release.is_err() {
        if !version.is_empty() {
            warn!("Failed to check wine release, rate limited?");
            return Ok(true);
        }

        bail!("Failed to check wine release: {}", release.err().unwrap());
    }

    Ok(version == release.unwrap().tag_name)
}

async fn get_lutris_runtimes() -> Result<Vec<LutrisRuntime>> {
    let client = reqwest::Client::builder().user_agent("ArmchairDevelopers/Maxima").build().unwrap();
    let res = client.get("https://lutris.net/api/runtimes").send().await?;
    let res = res.error_for_status()?; 
    let data = res.json().await?;
    Ok(data)
}

pub(crate) async fn check_eac_runtime_validity() -> Result<bool> {
    let eac_path = eac_dir()?;
    if !eac_path.exists() {
        return Ok(false);
    }
    let version = versions()?.eac_runtime;
    let runtimes = get_lutris_runtimes().await?;
    let eac_runtime = runtimes.iter().find(|r| r.name == "eac_runtime");

    Ok(eac_runtime.is_some_and(|r| r.created_at == version))
}

pub(crate) async fn install_eac() -> Result<()> {
    info!("Downloading EasyAntiCheat runtime");
    let runtimes = get_lutris_runtimes().await?;
    let eac_runtime = runtimes.iter().find(|r| r.name == "eac_runtime").unwrap();
    let path = eac_dir()?;

    let res = ureq::get(&eac_runtime.url)
        .set("User-Agent", "ArmchairDevelopers/Maxima")
        .call()?;

    if res.status() != StatusCode::OK {
        bail!("Failed to download eac runtime");
    }

    let mut body: Vec<u8> = vec![];
    res.into_reader().read_to_end(&mut body)?;

    if path.exists() {
        remove_dir_all(&path)?;
    }

    create_dir_all(&path)?;
    
    let xz = XzDecoder::new(&body[..]);
    let archive = Archive::new(xz);
    extract_archive(path, archive)?;

    let mut versions = versions()?;
    versions.eac_runtime = eac_runtime.created_at.clone();
    set_versions(versions)
}


fn get_wine_release() -> Result<GithubRelease> {
    let releases = fetch_github_releases("GloriousEggroll", "proton-ge-custom")?;

    let mut release = None;
    for r in releases {
        if r.tag_name.ends_with("LoL") {
            continue;
        }

        release = Some(r);
        break;
    }

    if release.is_none() {
        bail!("Couldn't find suitable wine release");
    }

    Ok(release.unwrap())
}

pub async fn run_wine_command<I: IntoIterator<Item = T>, T: AsRef<OsStr>>(
    arg: T,
    args: Option<I>,
    cwd: Option<PathBuf>,
    want_output: bool,
    command_type: CommandType,
) -> Result<String> {
    let proton_path = proton_dir()?;
    let proton_prefix_path = wine_prefix_dir()?;
    let eac_path = eac_dir()?;

    let wine_path = env::var("MAXIMA_WINE_COMMAND").unwrap_or_else(|_| "umu-run".to_owned());

    // Create command with all necessary wine env variables
    let mut binding = Command::new(wine_path.clone());
    let mut child = binding
        .env("WINEPREFIX", proton_prefix_path)
        .env("GAMEID", "umu-0")
        .env("PROTON_VERB", &command_type.to_string())
        .env("PROTONPATH", proton_path)
        .env("STORE", "ea")
        .env("PROTON_EAC_RUNTIME", eac_path)
        .env("WINEDEBUG", "fixme-all")
        .env("LD_PRELOAD", "") // Fixes some log errors for some games
        .arg(arg);

    if wine_path != "umu-run" {
        child = child.env("WINEDLLOVERRIDES", "CryptBase,wsock32,bcrypt,dxgi,d3d11,d3d12,d3d12core=n,b;winemenubuilder.exe=d");
    }

    if let Some(arguments) = args {
        child = child.args(arguments);
    }

    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }

    let status: ExitStatus;
    let mut output_str = String::new();

    if want_output {
        let output = child
            .stdout(Stdio::piped())
            .spawn()
            .context("Failed to run wine command")?
            .wait_with_output()
            .await?;
        output_str = String::from_utf8_lossy(&output.stdout).to_string();
        status = output.status;
    } else {
        status = child
            .spawn()
            .context("Failed to run wine command")?
            .wait()
            .await?;
    };

    if !status.success() {
        bail!(
            "Failed to run wine command: {} ({})",
            output_str,
            status.code().unwrap()
        );
    }

    Ok(output_str.to_string())
}

pub(crate) async fn install_wine() -> Result<()> {
    let release = get_wine_release()?;
    let asset = release
        .assets
        .iter()
        .find(|x| PROTON_PATTERN.captures(&x.name).is_some());
    if asset.is_none() {
        bail!("Failed to find proton asset! the name pattern might be outdated, please make an issue at https://github.com/ArmchairDevelopers/Maxima/issues.");
    }

    let asset = asset.unwrap();

    let dir = maxima_dir()?.join("downloads");
    create_dir_all(&dir)?;

    let path = dir.join(&asset.name);
    github_download_asset(asset, &path)?;
    extract_wine(&path)?;

    let mut versions = versions()?;
    versions.proton = release.tag_name;
    set_versions(versions)?;

    if let Err(err) = remove_file(&path) {
        log::warn!("Failed to delete {:?} - {:?}", path, err);
    }

    let _ = run_wine_command(
        "",
        None::<[&str; 0]>,
        None,
        false,
        CommandType::Run,
    ).await;

    Ok(())
}

fn extract_wine(archive_path: &PathBuf) -> Result<()> {
    info!("Extracting proton...");

    let dir = proton_dir()?;
    if dir.exists() {
        remove_dir_all(&dir)?;
    }

    create_dir_all(&dir)?;

    let archive_file = File::open(archive_path)?;
    let archive_decoder = GzDecoder::new(archive_file);
    let archive = Archive::new(archive_decoder);
    extract_archive(dir, archive)
}

fn extract_archive<R: Read + Sized>(dir: PathBuf, mut archive: Archive<R>) -> Result<()> {
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?;

        let destination_path =
            dir.join(entry_path.strip_prefix(entry_path.components().next().unwrap())?);
        if let Some(parent_dir) = destination_path.parent() {
            create_dir_all(parent_dir)?;
        }

        entry.unpack(destination_path)?;
    }

    Ok(())
}

pub async fn setup_wine_registry() -> Result<()> {
    run_wine_command(
        "reg",
        Some(vec![
            "add",
            "HKLM\\Software\\Electronic Arts\\EA Desktop",
            "/v",
            "InstallSuccessful",
            "/d",
            "true",
            "/f",
            "/reg:64",
        ]),
        None,
        false,
        CommandType::Run,
    ).await?;
    run_wine_command(
        "reg",
        Some(vec![
            "add",
            "HKLM\\Software\\Origin",
            "/v",
            "ClientPath",
            "/d",
            "C:/Windows/System32/conhost.exe",
            "/f",
            "/reg:32",
        ]),
        None,
        false,
        CommandType::Run,
    ).await?;

    Ok(())
}

pub type WineRegistry = HashMap<String, String>;

lazy_static! {
    static ref MX_WINE_REGISTRY: Mutex<WineRegistry> = Mutex::new(WineRegistry::new());
}

async fn parse_wine_registry(file_path: &str) -> WineRegistry {
    let mut registry_map = MX_WINE_REGISTRY.lock().await;
    if !registry_map.is_empty() {
        return registry_map.clone();
    }

    let file = tokio::fs::File::open(file_path)
        .await
        .expect("Could not open file");
    let reader = BufReader::new(file);
    let mut current_section = String::new();

    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await.expect("Failed to read file") {
        let trimmed_line = line.trim();

        if trimmed_line.starts_with('[') && trimmed_line.contains(']') {
            if let Some(end) = trimmed_line.find(']') {
                current_section = trimmed_line[1..end].to_string();
            }
        } else if trimmed_line.contains('=') && trimmed_line.starts_with('"') {
            let parts: Vec<&str> = trimmed_line.splitn(2, '=').collect();
            if parts.len() == 2 {
                let key = parts[0].trim_matches('"').to_string();
                let value = parts[1].trim_matches('"').to_string();
                let full_key = format!("{}\\{}", current_section, key).replace("\\\\", "\\");
                registry_map.insert(full_key.to_lowercase(), value);
            }
        }
    }

    registry_map.clone()
}

pub async fn parse_mx_wine_registry() -> WineRegistry {
    let path = wine_prefix_dir().unwrap().join("system.reg");
    if !path.exists() {
        return HashMap::new();
    }

    parse_wine_registry(path.to_str().unwrap()).await
}

pub async fn invalidate_mx_wine_registry() {
    MX_WINE_REGISTRY.lock().await.clear();
}

fn normalize_key(key: &str) -> String {
    let lower_key = key.to_lowercase();
    if lower_key.starts_with("hkey_local_machine\\") {
        lower_key
            .trim_start_matches("hkey_local_machine\\")
            .to_string()
    } else {
        lower_key
    }
}

pub async fn get_mx_wine_registry_value(query_key: &str) -> Option<String> {
    let registry_map = parse_mx_wine_registry().await;
    let normalized_query_key = normalize_key(query_key);

    let value = if let Some(value) = registry_map.get(&normalized_query_key) {
        Some(value.clone())
    } else {
        let wow6432_query_key =
            normalized_query_key.replace("software\\", "software\\wow6432node\\");
        registry_map.get(&wow6432_query_key).cloned()
    };

    value.map(|x| x.replace("Z:", "").replace("\\", "/"))
}
