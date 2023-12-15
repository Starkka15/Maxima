use std::{path::PathBuf, fs::{create_dir_all, File, self, remove_dir_all}, io::Read, process::Command};

use anyhow::{Result, bail};
use flate2::read::GzDecoder;
use lazy_static::lazy_static;
use log::info;
use regex::Regex;
use serde::{Serialize, Deserialize};
use tar::Archive;
use xz2::read::XzDecoder;

use crate::util::{native::maxima_dir, github::{fetch_github_release, github_download_asset, fetch_github_releases, GithubRelease}};

lazy_static! {
    static ref DXVK_PATTERN: Regex = Regex::new(r"dxvk-([0-9]\.[0-9])\.tar\.gz").unwrap();
    static ref VKD3D_PATTERN: Regex = Regex::new(r"vkd3d-proton-(.*)\.tar\.zst").unwrap();
    static ref PROTON_PATTERN: Regex = Regex::new(r"wine-lutris-GE-Proton.*\.tar\.xz").unwrap();
}

const VERSION_FILE: &str = "dependency-versions.toml";
const REG_FILE: &str = include_str!("wine.reg");

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

pub fn check_wine_validity() -> Result<bool> {
    let release = get_wine_release()?;
    Ok(versions()?.wine == release.tag_name)
}

pub fn check_dxvk_validity() -> Result<bool> {
    let release = fetch_github_release("doitsujin", "dxvk", "latest")?;
    Ok(versions()?.dxvk == release.tag_name)
}

pub fn check_vkd3d_validity() -> Result<bool> {
    let release = fetch_github_release("HansKristian-Work", "vkd3d-proton", "latest")?;
    Ok(versions()?.vkd3d == release.tag_name)
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

pub async fn install_wine() -> Result<()> {
    let release = get_wine_release()?;
    let asset = release.assets.iter().find(|x| PROTON_PATTERN.captures(&x.name).is_some());
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
        
        let destination_path = dir.join(entry_path.strip_prefix(entry_path.components().next().unwrap())?);
        if let Some(parent_dir) = destination_path.parent() {
            std::fs::create_dir_all(parent_dir)?;
        }

        entry.unpack(destination_path)?;
    }

    Ok(())
}

pub async fn wine_install_dxvk() -> Result<()> {
    let release = fetch_github_release("doitsujin", "dxvk", "latest")?;
    let asset = release.assets.iter().find(|x| DXVK_PATTERN.captures(&x.name).is_some());
    if asset.is_none() {
        bail!("Failed to find DXVK asset! the name pattern might be outdated, please make an issue at https://github.com/ArmchairDevelopers/Maxima/issues.");
    }

    let asset = asset.unwrap();

    let dir = maxima_dir()?.join("downloads");
    create_dir_all(&dir)?;

    let path = dir.join(&asset.name);
    github_download_asset(asset, &path)?;

    let version = DXVK_PATTERN.captures(&asset.name).unwrap().get(1).unwrap().as_str();
    extract_dynamic_archive(GzDecoder::new(File::open(path)?), "dxvk", version)?;

    let mut versions = versions()?;
    versions.dxvk = release.tag_name;
    set_versions(versions)?;

    Ok(())
}

pub async fn wine_install_vkd3d() -> Result<()> {
    let release = fetch_github_release("HansKristian-Work", "vkd3d-proton", "latest")?;
    let asset = release.assets.iter().find(|x| VKD3D_PATTERN.captures(&x.name).is_some());
    if asset.is_none() {
        bail!("Failed to find VKD3D asset! the name pattern might be outdated, please make an issue at https://github.com/ArmchairDevelopers/Maxima/issues.");
    }

    let dir = maxima_dir()?.join("downloads");
    create_dir_all(&dir)?;

    let asset = &release.assets[0];
    let path = dir.join(&asset.name);
    github_download_asset(asset, &path)?;

    let version = VKD3D_PATTERN.captures(&asset.name).unwrap().get(1).unwrap().as_str();
    extract_dynamic_archive(zstd::Decoder::new(File::open(path)?)?, "vkd3d-proton", version)?;

    let mut versions = versions()?;
    versions.vkd3d = release.tag_name;
    set_versions(versions)?;

    Ok(())
}

fn extract_dynamic_archive<R>(reader: R, label: &str, version: &str) -> Result<()>
    where R: Read,
{
    let windows_dir = wine_prefix_dir()?.join("drive_c/windows");
    let strip_prefix = format!("{}-{}/", label, version);

    let mut archive = Archive::new(reader);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?;
        
        let destination_path: PathBuf;
        if entry_path.starts_with(strip_prefix.clone() + "x64/") {
            destination_path = windows_dir.join("system32").join(entry_path.strip_prefix(strip_prefix.clone() + "x64/")?);
        } else if entry_path.starts_with(strip_prefix.clone() + "x32/") {
            destination_path = windows_dir.join("syswow64").join(entry_path.strip_prefix(strip_prefix.clone() + "x32/")?);
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
    let dir = wine_prefix_dir()?.join("drive_c");
    create_dir_all(&dir)?;
    fs::write(dir.join("wine.reg"), REG_FILE)?;

    let output = Command::new(maxima_dir()?.join("wine/bin/regedit"))
        .env("WINEPREFIX", wine_prefix_dir()?)
        //.arg("C:/Windows/syswow64/regedit.exe")
        .arg("/S")
        .arg("C:/wine.reg")
        .output()
        .expect("Failed to execute wine while modifying the registry");

    let output_str = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && !output_str.is_empty() {
        bail!("Failed to modify the wine registry: {}", output_str);
    }

    Ok(())
}