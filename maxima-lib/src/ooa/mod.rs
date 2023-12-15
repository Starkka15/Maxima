use std::{
    fs::{create_dir_all, File},
    io::Write,
};

use anyhow::{bail, Result};

use base64::{engine::general_purpose, Engine};
use openssl::symm::{decrypt, encrypt, Cipher};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use std::io::Read;

use crate::core::endpoints::API_PROXY_NOVAFUSION_LICENSES;

const CRYPTO_KEY: [u8; 16] = [
    65, 50, 114, 45, 208, 130, 239, 176, 220, 100, 87, 197, 118, 104, 202, 9,
];

const LICENSE_PATH: &str = "ProgramData/Electronic Arts/EA Services/License";

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct License {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub signature: String,
    pub cipher_key: String,
    pub machine_hash: String,
    pub content_id: String,
    pub user_id: String,
    pub game_token: Option<String>,
    pub grant_time: String,
    pub start_time: String,
}

pub async fn request_license(
    content_id: &str,
    machine_hash: &str,
    access_token: &str,
    request_token: Option<&str>,
    request_type: Option<&str>,
) -> Result<License> {
    let mut query = Vec::new();
    query.push(("contentId", content_id));
    query.push(("machineHash", machine_hash));
    query.push(("ea_eadmtoken", access_token));

    if request_token.is_some() {
        query.push(("requestToken", request_token.unwrap()));
        query.push(("requestType", request_type.unwrap()));
    }

    let res = ureq::get(API_PROXY_NOVAFUSION_LICENSES)
        .query_pairs(query)
        .set("X-Requester-Id", "Origin Online Activation")
        .set("User-Agent", "EACTransaction")
        .call()?;
    if res.status() != StatusCode::OK {
        bail!("License request failed");
    }

    let signature = res.header("x-signature").unwrap().to_owned();
    let mut body: Vec<u8> = vec![];
    res.into_reader()
        .take((4096 + 1) as u64)
        .read_to_end(&mut body)?;

    let cipher = Cipher::aes_128_cbc();
    let decrypted_data = decrypt(cipher, &CRYPTO_KEY, Some(&[0; 16]), body.as_slice())?;
    let data = String::from_utf8(decrypted_data)?;

    let mut license: License = quick_xml::de::from_str(data.as_str())?;
    license.signature = signature;
    Ok(license)
}

pub fn save_license(license: &License, path: String) -> Result<()> {
    let mut data = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>".to_string();
    data.push_str(quick_xml::se::to_string(license)?.as_str());
    data.remove_matches("<GameToken/>");

    let cipher = Cipher::aes_128_cbc();
    let encrypted_data = encrypt(cipher, &CRYPTO_KEY, Some(&[0; 16]), data.as_bytes())?;

    let decoded = general_purpose::STANDARD.decode(&license.signature)?;
    let len = decoded.len();
    let license_blob: Vec<u8> = vec![decoded, vec![0; 65 - len], encrypted_data]
        .into_iter()
        .flatten()
        .collect();

    let mut file = File::create(path).unwrap();
    file.write_all(license_blob.as_slice())?;
    file.flush()?;

    Ok(())
}

pub fn save_licenses(license: &License) -> Result<()> {
    let path = get_license_dir()?;

    save_license(&license, format!("{}/{}.dlf", path, license.content_id))?;
    save_license(
        &license,
        format!("{}/{}_cached.dlf", path, license.content_id),
    )?;

    Ok(())
}

#[cfg(windows)]
fn get_license_dir() -> Result<String> {
    let path = format!("C:/{}", LICENSE_PATH.to_string());
    create_dir_all(&path)?;
    Ok(path)
}

#[cfg(unix)]
fn get_license_dir() -> Result<String> {
    use crate::unix::wine::wine_prefix_dir;

    let path = format!("{}/drive_c/{}", wine_prefix_dir()?.to_str().unwrap(), LICENSE_PATH.to_string());
    create_dir_all(&path)?;

    Ok(path)
}