use std::path::PathBuf;

use std::io::Read;

use crate::util::native::DownloadError;
use log::info;
use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
}

#[derive(Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub assets: Vec<GithubAsset>,
}

pub fn fetch_github_releases(
    author: &str,
    repository: &str,
) -> Result<Vec<GithubRelease>, DownloadError> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases",
        author, repository
    );

    let res = ureq::get(&url)
        .set("User-Agent", "ArmchairDevelopers/Maxima")
        .call()?;
    if res.status() != StatusCode::OK {
        return Err(DownloadError::Http(res.into_string()?));
    }

    let text = res.into_string()?;
    let result = serde_json::from_str(text.as_str())?;
    Ok(result)
}

pub fn fetch_github_release(
    author: &str,
    repository: &str,
    version: &str,
) -> Result<GithubRelease, DownloadError> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/{}",
        author, repository, version
    );

    let res = ureq::get(&url)
        .set("User-Agent", "ArmchairDevelopers/Maxima")
        .call()?;
    if res.status() != StatusCode::OK {
        return Err(DownloadError::Http(res.into_string()?));
    }

    let text = res.into_string()?;
    let result = serde_json::from_str(text.as_str())?;
    Ok(result)
}

pub fn github_download_asset(asset: &GithubAsset, path: &PathBuf) -> Result<(), DownloadError> {
    info!("Downloading {}...", asset.name);

    let res = ureq::get(&asset.browser_download_url).call()?;
    if res.status() != StatusCode::OK {
        return Err(DownloadError::Http(res.into_string()?));
    }

    let mut body: Vec<u8> = vec![];
    res.into_reader().take(asset.size).read_to_end(&mut body)?;

    std::fs::write(path, body)?;
    Ok(())
}
