use std::{
    fs::create_dir_all,
    io::{Read, Write},
    path::PathBuf,
};

use aes::cipher::{
    block_padding::Pkcs7, generic_array::GenericArray, BlockDecryptMut, BlockEncryptMut, KeyIvInit,
};
use anyhow::{bail, Result};
use chrono::{DateTime, Duration, Utc};
use log::warn;
use tokio::fs::{self, File};

use base64::{engine::general_purpose, Engine};

use lazy_static::lazy_static;
use regex::Regex;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::core::{auth::hardware::HardwareInfo, endpoints::API_PROXY_NOVAFUSION_LICENSES};

pub const OOA_CRYPTO_KEY: [u8; 16] = [
    65, 50, 114, 45, 208, 130, 239, 176, 220, 100, 87, 197, 118, 104, 202, 9,
];

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

lazy_static! {
    static ref EMAIL_PATTERN: Regex = Regex::new(
        r"^([a-z0-9_+]([a-z0-9_+.]*[a-z0-9_+])?)@([a-z0-9]+([\-\.]{1}[a-z0-9]+)*\.[a-z]{2,6})"
    )
    .unwrap();
}

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

#[derive(PartialEq, Clone, Copy)]
pub enum OOAState {
    /// We don't need to request a license for this game
    Disabled,
    /// This game expects the license to have its signature base64 encoded
    SignatureEncoded,
    /// This game expects the license to have its signature decoded
    SignatureDecoded,
}

pub fn detect_ooa_state(game_path: PathBuf) -> OOAState {
    let core_dir = game_path.join("Core");
    if !core_dir.exists() {
        return OOAState::Disabled;
    }

    if core_dir.join("activation.exe").exists() {
        return OOAState::SignatureEncoded;
    }

    OOAState::SignatureDecoded
}

pub async fn detect_ooa_version(game_path: PathBuf) -> Result<u32> {
    #[cfg(unix)]
    use crate::unix::fs::case_insensitive_path;
    let activation_dll = game_path.join("Core/activation.dll");
    let awc = game_path.join("Core/awc.dll");

    #[cfg(unix)]
    let activation_dll = case_insensitive_path(activation_dll).await;
    #[cfg(unix)]
    let awc = case_insensitive_path(awc).await;

    let dest_dll = if activation_dll.exists() {
        activation_dll
    } else {
        awc
    };

    if !dest_dll.exists() {
        return Ok(1);
    }

    let image = tokio::fs::read(dest_dll).await?;
    let pe = pelite::PeFile::from_bytes(&image)?;

    let version = pe.resources()?.version_info()?;
    let version = if let Some(v) = version.fixed() {
        v.dwProductVersion
    } else {
        return Ok(1);
    };
    let major = version.Major;
    let minor = version.Minor;
    let patch = version.Patch;

    if major >= 5 {
        return Ok(4);
    } else if major == 4 && (minor > 7 || minor == 7 && patch >= 6) {
        return Ok(3);
    } else if major == 4 && minor >= 7 {
        return Ok(2);
    } else if major >= 4 {
        return Ok(1);
    } else {
        return Ok(0);
    }
}

pub enum LicenseAuth {
    AccessToken(String),
    /// Persona/Email, Password
    Direct(String, String),
}

pub async fn needs_license_update(content_id: &str) -> Result<bool> {
    let path = get_license_dir()?.join(format!("{}.dlf", content_id));
    if !path.exists() {
        return Ok(true);
    }

    let bytes = tokio::fs::read(path).await;
    if bytes.is_err() {
        return Ok(true);
    }

    let license = decrypt_license(&bytes.unwrap()[65..]);
    if license.is_err() {
        warn!("Failed to decrypt game license when checking for update");
        return Ok(true);
    }

    let license = license.unwrap();

    // Not actually sure how long licenses last, two weeks is a guesstimate
    let date: DateTime<Utc> = license.start_time.parse()?;
    Ok(Utc::now() - date > Duration::weeks(2))
}

pub async fn request_and_save_license(
    auth: &LicenseAuth,
    content_id: &str,
    mut game_path: PathBuf,
) -> Result<()> {
    if game_path.is_file() {
        game_path = game_path.parent().unwrap().to_path_buf();
    }

    let state = detect_ooa_state(game_path.clone());
    if state == OOAState::Disabled {
        return Ok(());
    }

    let version = detect_ooa_version(game_path).await.unwrap_or(1);

    let hw_info = HardwareInfo::new(version)?;
    let license = request_license(
        content_id,
        &hw_info.generate_hardware_hash(),
        auth,
        None,
        None,
    )
    .await?;
    save_licenses(&license, state).await?;

    Ok(())
}

pub async fn request_license(
    content_id: &str,
    machine_hash: &str,
    auth: &LicenseAuth,
    request_token: Option<&str>,
    request_type: Option<&str>,
) -> Result<License> {
    let mut query = Vec::new();
    query.push(("contentId", content_id));
    query.push(("machineHash", machine_hash));
    log::debug!("Using hash {}", machine_hash);

    match auth {
        LicenseAuth::AccessToken(access_token) => {
            query.push(("ea_eadmtoken", access_token));
        }
        LicenseAuth::Direct(persona, password) => {
            if EMAIL_PATTERN.is_match(persona) {
                query.push(("ea_email", persona));
            } else {
                query.push(("ea_persona", persona));
            }

            query.push(("ea_password", password));
        }
    }

    if request_token.is_some() {
        query.push(("requestToken", request_token.unwrap()));
        query.push(("requestType", request_type.unwrap()));
    }

    let res = Client::new()
        .get(API_PROXY_NOVAFUSION_LICENSES)
        .query(&query)
        .header("X-Requester-Id", "Origin Online Activation")
        .header("User-Agent", "EACTransaction")
        .send()
        .await?;
    if res.status() != StatusCode::OK {
        bail!("License request failed: {}", res.text().await?);
    }

    let signature = res.headers().get("x-signature").unwrap().to_owned();
    let body: Vec<u8> = res.bytes().await?.to_vec();

    let mut license = decrypt_license(body.as_slice())?;
    license.signature = signature.to_str()?.to_owned();
    Ok(license)
}

pub fn decrypt_license(data: &[u8]) -> Result<License> {
    let key = GenericArray::from_slice(&OOA_CRYPTO_KEY);
    let iv = GenericArray::from_slice(&[0u8; 16]);
    let cipher = Aes128CbcDec::new(key, iv);

    let decrypted_data = cipher.decrypt_padded_vec_mut::<Pkcs7>(data)?;
    let data_str = String::from_utf8(decrypted_data)?;

    Ok(quick_xml::de::from_str(&data_str)?)
}

pub fn encrypt_license(data: &str) -> Result<Vec<u8>> {
    let key = GenericArray::from_slice(&OOA_CRYPTO_KEY);
    let iv = GenericArray::from_slice(&[0u8; 16]);

    let cipher = Aes128CbcEnc::new(key, iv);
    Ok(cipher.encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes()))
}

pub async fn save_license(license: &License, state: OOAState, path: PathBuf) -> Result<()> {
    let mut data = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>".to_string();
    data.push_str(quick_xml::se::to_string(license)?.as_str());

    if !data.contains("<GameToken>") {
        data.remove_matches("<GameToken/>");
    }

    let encrypted_data = encrypt_license(&data)?;

    let mut signature = license.signature.as_bytes().to_vec();
    if state == OOAState::SignatureDecoded {
        signature = general_purpose::STANDARD.decode(&signature)?;
    }

    let signature_len = signature.len();
    let license_blob: Vec<u8> = vec![signature, vec![0; 65 - signature_len], encrypted_data]
        .into_iter()
        .flatten()
        .collect();

    fs::write(path, license_blob).await?;
    Ok(())
}

pub async fn save_licenses(license: &License, state: OOAState) -> Result<()> {
    let path = get_license_dir()?;

    save_license(
        &license,
        state,
        path.join(format!("{}.dlf", license.content_id)),
    )
    .await?;

    save_license(
        &license,
        state,
        path.join(format!("{}_cached.dlf", license.content_id)),
    )
    .await?;

    Ok(())
}

#[cfg(windows)]
pub fn get_license_dir() -> Result<PathBuf> {
    let path = format!("C:/{}", LICENSE_PATH.to_string());
    create_dir_all(&path)?;
    Ok(PathBuf::from(path))
}

#[cfg(unix)]
pub fn get_license_dir() -> Result<PathBuf> {
    use crate::unix::wine::wine_prefix_dir;

    let path = format!(
        "{}/drive_c/{}",
        wine_prefix_dir()?.to_str().unwrap(),
        LICENSE_PATH.to_string()
    );
    create_dir_all(&path)?;

    Ok(PathBuf::from(path))
}
