/// Cloud sync is a process and a half. Here's a high-level overview of it:
/// # Reading (pre-launch)
/// - Call `/lock/read`, which gives us a link to an XML on S3:
/// a manifest of all files in the cloud, along with their MD5 hash and last modified date
/// - If you feel the need to update the local files:
///   - Call `/lock/authorize` with a `Vec<CloudSyncRequest>` containing the corresponding `href` from the manifest
///   - This will return an XML with S3 targets corresponding to the files. Download them (the response body is the file, verbatim) and write them.
/// - Call `/lock/delete` to release the lock. **This is not optional**, however it will automatically release after 5-10 minutes.
///
/// # Writing (post-game)
/// - Call `/lock/write` which has the same functionality to you, but locks *write* on the backend
/// - If you feel the need to update cloud files:
///   - Call `/lock/authorize` with a `Vec<CloudSyncRequest>`, creating details that match the file and keeping track of them for later
///   - Push the files to the endpoints, along with a manifest outlining the files you uploaded and/or that are already there.
/// - Call `/lock/delete`
use super::{
    auth::storage::LockedAuthStorage, endpoints::API_CLOUDSYNC, launch::LaunchMode,
    library::OwnedOffer,
};
use crate::util::native::{NativeError, SafeParent, SafeStr};
use derive_getters::Getters;
use futures::StreamExt;
use log::{debug, error};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Debug,
    path::{Path, PathBuf},
};
use thiserror::Error;
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
};

const AUTH_HEADER: &str = "X-Origin-AuthToken";
const LOCK_HEADER: &str = "X-Origin-Sync-Lock";

#[derive(Error, Debug)]
pub enum CloudSyncError {
    #[error(transparent)]
    Auth(#[from] crate::core::auth::storage::AuthError),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Token(#[from] crate::core::auth::storage::TokenError),
    #[error(transparent)]
    Xml(#[from] quick_xml::de::DeError),
    #[error(transparent)]
    Glob(#[from] glob::GlobError),
    #[error(transparent)]
    Pattern(#[from] glob::PatternError),
    #[error(transparent)]
    ToStr(#[from] reqwest::header::ToStrError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    Library(#[from] crate::core::library::LibraryError),

    #[error("failed to acquire {0:?}")]
    LockAcquire(CloudSyncLockMode),
    #[error("`{0}` has no cloudsync configuration")]
    NoConfig(String),
    #[error("cannot cloudsync when logged out")]
    NotSignedIn,
}

pub enum CloudSyncLockMode {
    Read,
    Write,
}

impl Debug for CloudSyncLockMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudSyncLockMode::Read => write!(f, "lock/read"),
            CloudSyncLockMode::Write => write!(f, "lock/write"),
        }
    }
}

async fn acquire_auth(auth: &LockedAuthStorage) -> Result<(String, String), CloudSyncError> {
    let mut auth = auth.lock().await;

    let token = auth.access_token().await?;
    let user_id = auth.user_id().await?;
    if token.is_none() || user_id.is_none() {
        return Err(CloudSyncError::NotSignedIn);
    }

    let token = token.unwrap();
    let user_id = user_id.unwrap();

    Ok((token, user_id))
}

#[cfg(windows)]
fn home_dir() -> Result<PathBuf, NativeError> {
    Ok(PathBuf::from(
        std::env::var_os("USERPROFILE").unwrap_or_else(|| "C:\\Users\\Public".into()),
    ))
}

#[cfg(unix)]
fn home_dir() -> Result<PathBuf, NativeError> {
    use crate::unix::wine::wine_prefix_dir;
    Ok(wine_prefix_dir()?.join("drive_c/users/steamuser"))
}

fn substitute_paths<P: AsRef<str>>(path: P) -> Result<PathBuf, NativeError> {
    let mut result = PathBuf::new();
    let path_str = path.as_ref();

    if path_str.contains("%Documents%") {
        let path = home_dir()?.join("Documents");
        result.push(path_str.replace("%Documents%", path.to_str().unwrap_or_default()));
    } else if path_str.contains("%SavedGames%") {
        let path = home_dir()?.join("Saved Games");
        result.push(path_str.replace("%SavedGames%", path.to_str().unwrap_or_default()));
    } else {
        result.push(path_str);
    }

    Ok(result)
}

fn unsubstitute_paths<P: AsRef<Path>>(path: P) -> Result<String, NativeError> {
    let path = path.as_ref();
    let home = home_dir()?;

    let documents_path = home.join("Documents");
    let saved_games_path = home.join("Saved Games");

    let path_str = path.to_str().unwrap_or_default().to_string();

    if path_str.contains(documents_path.to_str().unwrap_or_default()) {
        Ok(path_str.replace(documents_path.to_str().unwrap_or_default(), "%Documents%"))
    } else if path_str.contains(saved_games_path.to_str().unwrap_or_default()) {
        Ok(path_str.replace(
            saved_games_path.to_str().unwrap_or_default(),
            "%SavedGames%",
        ))
    } else {
        Ok(path_str)
    }
}

enum HashMode {
    Hex,
    Base62,
}

async fn calc_file_md5(file: File, mode: HashMode) -> Result<String, CloudSyncError> {
    let len = file.metadata().await?.len();

    let buf_len = len.min(1_000_000) as usize;
    let mut buf = BufReader::with_capacity(buf_len, file);
    let mut context = md5::Context::new();

    loop {
        let part = buf.fill_buf().await?;
        if part.is_empty() {
            break;
        }

        context.consume(part);

        let len = part.len();
        buf.consume(len);
    }

    let digest = context.compute();

    match mode {
        HashMode::Hex => Ok(format!("{:x}", digest)),
        HashMode::Base62 => {
            let mut b = format!("{}", u128::from_le_bytes(digest.0));

            while b.len() < 24 {
                b = format!("{}=", b)
            }

            Ok(b)
        }
    }
}

#[derive(Getters)]
pub struct CloudSyncLock<'a> {
    auth: &'a LockedAuthStorage,
    client: &'a Client,
    lock: String,
    manifest: CloudSyncManifest,
    mode: CloudSyncLockMode,
    allowed_files: Vec<PathBuf>,
}

impl<'a> CloudSyncLock<'a> {
    pub async fn new(
        auth: &'a LockedAuthStorage,
        client: &'a Client,
        manifest_url: String,
        lock: String,
        mode: CloudSyncLockMode,
        allowed_files: Vec<PathBuf>,
    ) -> Result<Self, CloudSyncError> {
        let res = client.get(manifest_url).send().await?;

        let manifest: CloudSyncManifest = {
            let mut manifest = if let Ok(text) = res.text().await {
                let result = quick_xml::de::from_str(&text);
                if let Ok(manifest) = result {
                    manifest
                } else {
                    None
                }
            } else {
                None
            };

            let mut manifest = manifest.unwrap_or_else(|| CloudSyncManifest {
                attr_xmlns: String::new(),
                file: Vec::new(),
            });

            manifest.attr_xmlns = "http://origin.com/cloudsaves/manifest".to_owned();
            manifest
        };

        Ok(Self {
            auth,
            client,
            lock,
            manifest,
            mode,
            allowed_files,
        })
    }

    pub async fn release(&self) -> Result<(), CloudSyncError> {
        let (token, user_id) = acquire_auth(self.auth).await?;

        let res = self
            .client
            .delete(format!("{}/lock/delete/{}", API_CLOUDSYNC, user_id))
            .header(AUTH_HEADER, token)
            .header(LOCK_HEADER, &self.lock)
            .header("Content-Length", 0)
            .send()
            .await?;

        res.error_for_status()?;

        debug!("Released CloudSync {:?} {}", self.mode, self.lock);
        Ok(())
    }

    /// The file syncing functions are some real hastily written code at the moment.
    /// Lots of stuff could be better and merged between them. TODO: Clean it up.
    pub async fn sync_files(&self) -> Result<(), CloudSyncError> {
        Ok(match self.mode {
            CloudSyncLockMode::Read => self.sync_read_files().await,
            CloudSyncLockMode::Write => self.sync_write_files().await,
        }?)
    }

    async fn sync_read_files(&self) -> Result<(), CloudSyncError> {
        let mut value = CloudSyncRequests::default();

        let mut paths = HashMap::new();
        for i in 0..self.manifest.file.len() {
            let local_path = &self.manifest.file[i].local_name;
            let path = substitute_paths(local_path)?;

            let file = OpenOptions::new().read(true).open(path.clone()).await;

            let md5 = if let Ok(file) = file {
                let md5 = calc_file_md5(file, HashMode::Hex).await?;
                if let Some(_) = self.manifest.file_by_md5(&md5) {
                    debug!("Skipping CloudSync read {}", &path.display());
                    continue;
                }
                md5
            } else {
                continue;
            };

            value.request.push(CloudSyncRequest {
                attr_id: i.to_string(),
                verb: "GET".to_owned(),
                resource: self.manifest.file[i].attr_href.to_owned(),
                content_type: None,
                md5: None,
            });

            paths.insert(i.to_string(), path);
        }

        if value.request.is_empty() {
            return Ok(());
        }

        let (token, user_id) = acquire_auth(self.auth).await?;
        let body = quick_xml::se::to_string(&value)?.replace("CloudSyncRequests", "requests");

        let res = self
            .client
            .put(format!("{}/lock/authorize/{}", API_CLOUDSYNC, user_id))
            .header(AUTH_HEADER, token)
            .header(LOCK_HEADER, &self.lock)
            .header("Content-Type", "application/xml")
            .body(body.to_owned())
            .send()
            .await?;

        let text = res.text().await?;
        let authorizations: CloudSyncAuthorizationResponses = quick_xml::de::from_str(&text)?;

        for i in 0..authorizations.request.len() {
            let auth_req = &authorizations.request[i];
            let mut req = self.client.get(&auth_req.url);
            let res = req.send().await?;
            if !res.status().is_success() {
                // If the request is invalid, S3 will return an error *and* include that error in the body.
                // DO NOT save the body to disk if it's an error. It will corrupt your save data.
                // Ask me how I know.
                error!(
                    "Failed to download CloudSync file: {:?}: {}",
                    res.status(),
                    res.text().await?
                );
                continue;
            }

            let path = paths.get(&auth_req.attr_id).unwrap();

            debug!(
                "Downloaded CloudSync file [{:?}, {} bytes]",
                path, self.manifest.file[i].attr_size
            );

            tokio::fs::create_dir_all(path.safe_parent()?).await?;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(path)
                .await?;

            let mut body = res.bytes_stream();
            while let Some(item) = body.next().await {
                let chunk = item?;
                file.write_all(&chunk).await?;
            }
        }

        Ok(())
    }

    async fn sync_write_files(&self) -> Result<(), CloudSyncError> {
        let mut auth_reqs = CloudSyncRequests::default();

        enum WriteData {
            File {
                name: String,
                file: File,
                hex: String,
                base62: String,
            },
            Text {
                name: String,
                text: String,
            },
        }

        impl WriteData {
            pub async fn file_key(&self) -> Result<String, CloudSyncError> {
                Ok(match self {
                    WriteData::File { file, hex, .. } => {
                        format!("{}-{}", file.metadata().await?.len(), hex)
                    }
                    _ => String::new(),
                })
            }
        }

        auth_reqs.request.push(CloudSyncRequest {
            attr_id: "0".to_owned(),
            verb: "PUT".to_owned(),
            resource: "manifest.xml".to_owned(),
            content_type: Some("text/xml".to_owned()),
            md5: None, // this is gonna bite me in the ass
        });

        let mut data = HashMap::new();
        let mut skipped = Vec::new();

        let mut i = 1;
        for path in &self.allowed_files {
            let file = OpenOptions::new().read(true).open(path.clone()).await?;

            let md5 = calc_file_md5(file.try_clone().await?, HashMode::Hex).await?;
            let base62 = calc_file_md5(file.try_clone().await?, HashMode::Base62).await?;
            if let Some(file) = self.manifest.file_by_md5(&md5) {
                debug!("Skipping CloudSync write {}", &path.display());
                skipped.push(file);
                continue;
            }

            let name = unsubstitute_paths(&path)?;
            let write_data = WriteData::File {
                name,
                file,
                hex: md5,
                base62: base62.clone(),
            };

            // TODO(headassbtw): DELETE if already present

            auth_reqs.request.push(CloudSyncRequest {
                attr_id: i.to_string(),
                verb: "PUT".to_owned(),
                resource: write_data.file_key().await?,
                content_type: None,
                md5: Some(base62),
            });

            data.insert(i, write_data);
            i += 1;
        }

        // Don't bother uploading/updating the cloudsave data if there's no changes.
        if data.is_empty() {
            return Ok(());
        }

        // Create a manifest that tells the cloud what files it does and is going to have.
        {
            let mut manifest = self.manifest.clone();
            manifest.file.clear();

            for ele in skipped {
                manifest.file.push(ele.clone());
            }

            for (_, write_data) in &data {
                if let WriteData::File {
                    name, file, base62, ..
                } = write_data
                {
                    let file = CloudSyncFile {
                        attr_href: write_data.file_key().await?,
                        attr_size: file.metadata().await?.len().to_string(),
                        attr_md5: Some(base62.to_owned()),
                        local_name: name.to_owned(),
                    };

                    manifest.file.push(file);
                }
            }

            let manifest =
                quick_xml::se::to_string(&manifest)?.replace("CloudSyncManifest", "manifest");
            data.insert(
                0,
                WriteData::Text {
                    name: "manifest.xml".to_owned(),
                    text: manifest,
                },
            );
        }

        let (token, user_id) = acquire_auth(self.auth).await?;
        let body = quick_xml::se::to_string(&auth_reqs)?.replace("CloudSyncRequests", "requests");

        let res = self
            .client
            .put(format!("{}/lock/authorize/{}", API_CLOUDSYNC, user_id))
            .header(AUTH_HEADER, token)
            .header(LOCK_HEADER, &self.lock)
            .header("Content-Type", "application/xml")
            .body(body)
            .send()
            .await?;

        let text = res.error_for_status()?.text().await?;
        let authorizations: CloudSyncAuthorizationResponses = quick_xml::de::from_str(&text)?;

        for i in 0..authorizations.request.len() {
            let auth_req = &authorizations.request[i];
            let mut req = self.client.put(&auth_req.url);
            for header in &auth_req.headers.header.clone().unwrap_or(Vec::new()) {
                req = req.header(&header.attr_key, &header.attr_value);
            }

            let length = match &data[&i] {
                WriteData::File { file, .. } => {
                    let mut buffer = vec![0; file.metadata().await?.len() as usize];
                    file.try_clone().await?.read_to_end(&mut buffer).await?;

                    let len = buffer.len();
                    req = req.body(buffer);

                    len as u64
                }
                WriteData::Text { text, .. } => {
                    req = req.body(text.to_owned());
                    text.len() as u64
                }
            };

            req = req.header("Content-Length", length);
            let res = req.send().await?;

            if !res.status().is_success() {
                error!(
                    "Failed to upload CloudSync file {}: {} {}",
                    auth_reqs.request[i].resource,
                    res.status(),
                    res.text().await?
                );
                continue;
            }

            debug!(
                "Uploaded CloudSync file [{:?}, {} bytes]",
                auth_reqs.request[i].resource, length
            );
        }

        Ok(())
    }
}

pub struct CloudSyncClient {
    auth: LockedAuthStorage,
    client: Client,
}

impl CloudSyncClient {
    pub fn new(auth: LockedAuthStorage) -> Self {
        Self {
            auth,
            client: ClientBuilder::default().gzip(true).build().unwrap(),
        }
    }

    pub async fn obtain_lock<'a>(
        &self,
        offer: &OwnedOffer,
        mode: CloudSyncLockMode,
    ) -> Result<CloudSyncLock, CloudSyncError> {
        let id = format!(
            "{}_{}",
            offer.offer().primary_master_title_id(),
            offer.offer().multiplayer_id().as_ref().unwrap()
        );

        let mut allowed_files = Vec::new();
        if let Some(config) = offer.offer().cloud_save_configuration_override() {
            let criteria: CloudSyncSaveFileCriteria = quick_xml::de::from_str(config)?;
            for include in criteria.include {
                let path = substitute_paths(include.value)?;
                let paths = glob::glob(path.safe_str()?)?;
                for path in paths {
                    let path = path?;
                    if path.is_dir() {
                        continue;
                    }

                    allowed_files.push(path);
                }
            }
        } else {
            return Err(CloudSyncError::NoConfig(offer.offer_id().clone()));
        }

        Ok(self.obtain_lock_raw(&id, mode, allowed_files).await?)
    }

    pub async fn obtain_lock_raw<'a>(
        &self,
        id: &str,
        mode: CloudSyncLockMode,
        allowed_files: Vec<PathBuf>,
    ) -> Result<CloudSyncLock, CloudSyncError> {
        let (token, user_id) = acquire_auth(&self.auth).await?;

        let res = self
            .client
            .post(format!("{}/{:?}/{}/{}", API_CLOUDSYNC, mode, user_id, id))
            .header(AUTH_HEADER, token)
            .header("Content-Length", 0)
            .send()
            .await?;
        let lock = match res.headers().get("x-origin-sync-lock") {
            Some(lock) => lock.to_str()?.to_owned(),
            None => {
                return Err(CloudSyncError::LockAcquire(mode));
            }
        };
        debug!("Obtained CloudSync {:?}: {}", mode, lock);

        let text = res.text().await?;
        let sync: CloudSyncSync = quick_xml::de::from_str(&text)?;
        Ok(CloudSyncLock::new(
            &self.auth,
            &self.client,
            sync.manifest,
            lock,
            mode,
            allowed_files,
        )
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::core::{auth::storage::AuthStorage, library::GameLibrary};

    use super::*;

    #[tokio::test]
    async fn read_files() -> Result<(), CloudSyncError> {
        let auth = AuthStorage::load()?;
        if !auth.lock().await.logged_in().await? {
            return Err(CloudSyncError::NotSignedIn);
        }

        let mut library = GameLibrary::new(auth.clone()).await;
        let offer = library
            .game_by_base_slug("star-wars-battlefront-2")
            .await?
            .unwrap();

        println!("Got offer");

        let client = CloudSyncClient::new(auth);

        let lock = client.obtain_lock(offer, CloudSyncLockMode::Read).await?;
        //lock.sync_read_files().await?;
        lock.release().await?;
        Ok(())
    }

    #[tokio::test]
    async fn write_files() -> Result<(), CloudSyncError> {
        let auth = AuthStorage::load()?;
        if !auth.lock().await.logged_in().await? {
            return Err(CloudSyncError::NotSignedIn);
        }

        let mut library = GameLibrary::new(auth.clone()).await;
        let offer = library
            .game_by_base_slug("star-wars-battlefront-2")
            .await?
            .unwrap();

        println!("Got offer");

        let client = CloudSyncClient::new(auth);

        let lock = client.obtain_lock(offer, CloudSyncLockMode::Write).await?;
        let res = lock.sync_write_files().await;
        lock.release().await?;
        res?;
        Ok(())
    }
}

macro_rules! cloudsync_type {
    (
        $(#[$message_attr:meta])*
        $message_name:ident;
        attr {
            $(
                $(#[$attr_field_attr:meta])*
                $attr_field:ident: $attr_field_type:ty
            ),* $(,)?
        },
        data {
            $(
                $(#[$field_attr:meta])*
                $field:ident: $field_type:ty
            ),* $(,)?
        }
    ) => {
        paste::paste! {
            // Main struct definition
            $(#[$message_attr])*
            #[derive(Default, Debug, Clone, Serialize, Deserialize, Getters, PartialEq)]
            #[serde(rename_all = "camelCase")]
            struct [<CloudSync $message_name>] {
                $(
                    $(#[$attr_field_attr])*
                    #[serde(rename = "@" $attr_field)]
                    [<attr_ $attr_field>]: $attr_field_type,
                )*
                $(
                    $(#[$field_attr])*
                    $field: $field_type,
                )*
            }
        }
    }
}

cloudsync_type!(
    File;
    attr {
        href: String,
        size: String,
        md5: Option<String>,
    },
    data {
        local_name: String,
    }
);

cloudsync_type!(
    Manifest;
    attr {
        xmlns: String,
    },
    data {
        file: Vec<CloudSyncFile>,
    }
);

impl CloudSyncManifest {
    pub fn file_by_md5(&self, md5: &str) -> Option<&CloudSyncFile> {
        for ele in &self.file {
            if let Some(ele_md5) = &ele.attr_md5 {
                if ele_md5 != md5 {
                    continue;
                }

                return Some(&ele);
            }
        }

        None
    }
}

cloudsync_type!(
    Sync;
    attr {},
    data {
        host: String,
        root: String,
        manifest: String,
    }
);

cloudsync_type!(
    Request;
    attr {
        id: String,
    },
    data {
        verb: String,
        resource: String,
        md5: Option<String>,

        #[serde(rename = "content-type")]
        content_type: Option<String>,
    }
);

cloudsync_type!(
    Requests;
    attr {},
    data {
        request: Vec<CloudSyncRequest>,
    }
);

cloudsync_type!(
    Header;
    attr {
        key: String,
        value: String,
    },
    data {}
);

cloudsync_type!(
    HeaderWrapper;
    attr {},
    data {
        header: Option<Vec<CloudSyncHeader>>,
    }
);

cloudsync_type!(
    AuthorizationResponse;
    attr {
        id: String,
    },
    data {
        url: String,
        headers: CloudSyncHeaderWrapper,
    }
);

cloudsync_type!(
    AuthorizationResponses;
    attr {},
    data {
        request: Vec<CloudSyncAuthorizationResponse>,
    }
);

cloudsync_type!(
    SaveFileInclude;
    attr {
        order: String,
    },
    data {
        #[serde(rename = "$value")]
        value: String,
    }
);

cloudsync_type!(
    SaveFileCriteria;
    attr {},
    data {
        include: Vec<CloudSyncSaveFileInclude>,
    }
);
