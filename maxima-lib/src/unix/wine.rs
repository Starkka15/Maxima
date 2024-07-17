use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{File, self, create_dir_all, remove_dir_all},
    io::Read,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tar::Archive;
use tokio::{io::{AsyncBufReadExt, BufReader}, sync::Mutex};
use xz2::read::XzDecoder;

use crate::util::{
    github::{fetch_github_release, fetch_github_releases, github_download_asset, GithubRelease},
    native::maxima_dir,
};

lazy_static! {
    static ref DXVK_PATTERN: Regex = Regex::new(r"dxvk-(.*)\.tar\.gz").unwrap();
    static ref VKD3D_PATTERN: Regex = Regex::new(r"vkd3d-proton-(.*)\.tar\.zst").unwrap();
    static ref PROTON_PATTERN: Regex = Regex::new(r"wine-lutris-GE-Proton.*\.tar\.xz").unwrap();
}

const VERSION_FILE: &str = "dependency-versions.toml";

#[derive(Serialize, Deserialize, Default)]
struct Versions {
    wine: String,
    dxvk: String,
    vkd3d: String,
}

pub fn wine_prefix_dir() -> Result<PathBuf> {
    Ok(maxima_dir()?.join("pfx"))
}

fn versions() -> Result<Versions> {
    let file = maxima_dir()?.join(VERSION_FILE);
    if !file.exists() {
        return Ok(Versions::default());
    }

    let data = fs::read_to_string(file)?;
    Ok(toml::from_str(&data)?)
}

fn set_versions(versions: Versions) -> Result<()> {
    let file = maxima_dir()?.join(VERSION_FILE);
    fs::write(file, toml::to_string(&versions)?)?;
    Ok(())
}

pub(crate) fn check_wine_validity() -> Result<bool> {
    let version = versions()?.wine;

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

pub(crate) fn check_dxvk_validity() -> Result<bool> {
    let version = versions()?.dxvk;

    let release = fetch_github_release("doitsujin", "dxvk", "latest");
    if release.is_err() {
        if !version.is_empty() {
            warn!("Failed to check dxvk release, rate limited?");
            return Ok(true);
        }

        bail!("Failed to check dxvk release: {}", release.err().unwrap());
    }
    Ok(version == release.unwrap().tag_name)
}

pub(crate) fn check_vkd3d_validity() -> Result<bool> {
    let version = versions()?.vkd3d;

    let release = fetch_github_release("HansKristian-Work", "vkd3d-proton", "latest");
    if release.is_err() {
        if !version.is_empty() {
            warn!("Failed to check vkd3d release, rate limited?");
            return Ok(true);
        }

        bail!("Failed to check vkd3d release: {}", release.err().unwrap());
    }

    Ok(version == release.unwrap().tag_name)
}

fn get_wine_release() -> Result<GithubRelease> {
    let releases = fetch_github_releases("GloriousEggroll", "wine-ge-custom")?;

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

pub fn run_wine_command<I: IntoIterator<Item = T>, T: AsRef<OsStr>>(
    program: &str,
    arg: T,
    args: Option<I>,
    cwd: Option<PathBuf>,
    want_output: bool,
) -> Result<String> {
    let path = maxima_dir()?.join(format!("wine/bin/{}", program));

    // Create command with all necessary wine env variables
    let mut binding = Command::new(path);
    let mut child = binding
        .env("WINEPREFIX", wine_prefix_dir()?)
        .env(
            "WINEDLLOVERRIDES",
            "CryptBase,bcrypt,dxgi,d3d11,d3d12,d3d12core=n,b;winemenubuilder.exe=d",
        ) // Disable winemenubuilder so it doesnt mess with file associations
        .env(
            "WINEDLLPATH",
            format!(
                "{}:{}",
                maxima_dir()?.join("wine/lib64/wine").display(),
                maxima_dir()?.join("wine/lib/wine").display()
            ),
        )
        // These should probably be settings for the user to enable/disable
        .env("WINE_FULLSCREEN_FSR", "0")
        .env("WINEESYNC", "1")
        .env("WINEFSYNC", "1")
        .env("WINEDEBUG", "fixme-all")
        .env("LD_PRELOAD", "") // Fixes some log errors for some games
        .env(
            "LD_LIBRARY_PATH",
            format!(
                "{}:{}",
                maxima_dir()?.join("wine/lib64").display(),
                maxima_dir()?.join("wine/lib").display()
            ),
        )
        .env(
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            format!(
                "{}:{}",
                maxima_dir()?.join("wine/lib64/gstreamer-1.0").display(),
                maxima_dir()?.join("wine/lib/gstreamer-1.0").display()
            ),
        )
        .arg(arg);

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
            .wait_with_output()?;
        output_str = String::from_utf8_lossy(&output.stdout).to_string();
        status = output.status;
    } else {
        status = child
            .spawn()
            .context("Failed to run wine command")?
            .wait()?;

        // Start wineserver to wait for the process to exit
        // Disabled because this is causing hangs

        // let wine_server_path = maxima_dir()?.join("wine/bin/wineserver");
        // let mut wine_server_binding = Command::new(wine_server_path);
        // let wine_server = wine_server_binding
        //     .env("WINEPREFIX", wine_prefix_dir()?)
        //     .arg("--wait");
        // wine_server.spawn()?.wait()?;
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
    versions.wine = release.tag_name;
    set_versions(versions)?;

    run_wine_command("wine64", "wineboot", Some(vec![" --init"]), None, false)?;

    Ok(())
}

fn extract_wine(archive_path: &PathBuf) -> Result<()> {
    info!("Extracting wine...");

    let dir = maxima_dir()?.join("wine");
    if dir.exists() {
        remove_dir_all(&dir)?;
    }

    create_dir_all(&dir)?;

    let archive_file = File::open(archive_path)?;
    let archive_decoder = XzDecoder::new(archive_file);
    let mut archive = Archive::new(archive_decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?;

        let destination_path =
            dir.join(entry_path.strip_prefix(entry_path.components().next().unwrap())?);
        if let Some(parent_dir) = destination_path.parent() {
            std::fs::create_dir_all(parent_dir)?;
        }

        entry.unpack(destination_path)?;
    }

    Ok(())
}

fn add_dll_override(dll_name: &str) -> Result<()> {
    run_wine_command(
        "wine",
        "reg",
        Some(vec![
            "add",
            "HKEY_CURRENT_USER\\Software\\Wine\\DllOverrides",
            "/v",
            dll_name,
            "/d",
            "native,builtin",
            "/f",
        ]),
        None,
        false,
    )?;

    Ok(())
}

pub(crate) async fn wine_install_dxvk() -> Result<()> {
    let release = fetch_github_release("doitsujin", "dxvk", "latest")?;
    let asset = release
        .assets
        .iter()
        .find(|x| DXVK_PATTERN.captures(&x.name).is_some());
    if asset.is_none() {
        bail!("Failed to find DXVK asset! the name pattern might be outdated, please make an issue at https://github.com/ArmchairDevelopers/Maxima/issues.");
    }

    let asset = asset.unwrap();

    let dir = maxima_dir()?.join("downloads");
    create_dir_all(&dir)?;

    let path = dir.join(&asset.name);
    github_download_asset(asset, &path)?;

    let version = DXVK_PATTERN
        .captures(&asset.name)
        .unwrap()
        .get(1)
        .unwrap()
        .as_str();
    extract_dynamic_archive(GzDecoder::new(File::open(path)?), "dxvk", version)?;

    let mut versions = versions()?;
    versions.dxvk = release.tag_name;
    set_versions(versions)?;

    add_dll_override("d3d10core")?;
    add_dll_override("d3d11")?;
    add_dll_override("d3d9")?;
    add_dll_override("dxgi")?;

    Ok(())
}

pub async fn wine_install_vkd3d() -> Result<()> {
    let release = fetch_github_release("HansKristian-Work", "vkd3d-proton", "latest")?;
    let asset = release
        .assets
        .iter()
        .find(|x| VKD3D_PATTERN.captures(&x.name).is_some());
    if asset.is_none() {
        bail!("Failed to find VKD3D asset! the name pattern might be outdated, please make an issue at https://github.com/ArmchairDevelopers/Maxima/issues.");
    }

    let dir = maxima_dir()?.join("downloads");
    create_dir_all(&dir)?;

    let asset = &release.assets[0];
    let path = dir.join(&asset.name);
    github_download_asset(asset, &path)?;

    let version = VKD3D_PATTERN
        .captures(&asset.name)
        .unwrap()
        .get(1)
        .unwrap()
        .as_str();
    extract_dynamic_archive(
        zstd::Decoder::new(File::open(path)?)?,
        "vkd3d-proton",
        version,
    )?;

    let mut versions = versions()?;
    versions.vkd3d = release.tag_name;
    set_versions(versions)?;

    add_dll_override("d3d12core")?;
    add_dll_override("d3d12")?;

    Ok(())
}

fn extract_dynamic_archive<R>(reader: R, label: &str, version: &str) -> Result<()>
where
    R: Read,
{
    let windows_dir = wine_prefix_dir()?.join("drive_c/windows");
    let strip_prefix = format!("{}-{}/", label, version);

    let mut archive = Archive::new(reader);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?;

        let destination_path: PathBuf;
        if entry_path.starts_with(strip_prefix.clone() + "x64/") {
            destination_path = windows_dir
                .join("system32")
                .join(entry_path.strip_prefix(strip_prefix.clone() + "x64/")?);
        } else if entry_path.starts_with(strip_prefix.clone() + "x32/") {
            destination_path = windows_dir
                .join("syswow64")
                .join(entry_path.strip_prefix(strip_prefix.clone() + "x32/")?);
        } else {
            continue;
        }

        if let Some(parent_dir) = destination_path.parent() {
            create_dir_all(parent_dir)?;
        }

        entry.unpack(destination_path)?;
    }

    Ok(())
}

pub fn setup_wine_registry() -> Result<()> {
    run_wine_command(
        "wine64",
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
    )?;
    run_wine_command(
        "wine64",
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
    )?;

    Ok(())
}

pub type WineRegistry = HashMap<String, String>;

lazy_static!{
    static ref MX_WINE_REGISTRY: Mutex<WineRegistry> = Mutex::new(WineRegistry::new());
}

async fn parse_wine_registry(file_path: &str) -> WineRegistry {
    let mut registry_map = MX_WINE_REGISTRY.lock().await;
    if !registry_map.is_empty() {
        return registry_map.clone();
    }

    let file = tokio::fs::File::open(file_path).await.expect("Could not open file");
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
        lower_key.trim_start_matches("hkey_local_machine\\").to_string()
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
        let wow6432_query_key = normalized_query_key.replace("software\\", "software\\wow6432node\\");
        registry_map.get(&wow6432_query_key).cloned()
    };

    value.map(|x| x.replace("Z:", "").replace("\\", "/"))
}
