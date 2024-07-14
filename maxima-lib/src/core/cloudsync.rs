use std::{
    collections::HashMap,
    env,
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use derive_getters::Getters;
use futures::StreamExt;
use log::debug;
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

use super::{auth::storage::LockedAuthStorage, endpoints::API_CLOUDSYNC, library::OwnedOffer};

const AUTH_HEADER: &str = "X-Origin-AuthToken";
const LOCK_HEADER: &str = "X-Origin-Sync-Lock";

pub enum CloudSyncLockMode {
    Read,
    Write,
}

impl CloudSyncLockMode {
    pub fn key(&self) -> &'static str {
        match self {
            CloudSyncLockMode::Read => "readlock",
            CloudSyncLockMode::Write => "writelock",
        }
    }
}

async fn acquire_auth(auth: &LockedAuthStorage) -> Result<(String, String)> {
    let mut auth = auth.lock().await;

    let token = auth.access_token().await?;
    let user_id = auth.user_id().await?;
    if token.is_none() || user_id.is_none() {
        bail!("You are not signed in");
    }

    let token = token.unwrap();
    let user_id = user_id.unwrap();

    Ok((token, user_id))
}

#[cfg(windows)]
fn home_dir() -> PathBuf {
    PathBuf::from(match env::var_os("USERPROFILE") {
        Some(user_profile) => user_profile,
        None => "C:\\Users\\Public".into(),
    })
}

#[cfg(unix)]
fn home_dir() -> PathBuf {
    use crate::unix::wine::wine_prefix_dir;

    let user = match env::var_os("USER") {
        Some(user) => user,
        None => "".into(),
    };

    wine_prefix_dir().unwrap().join("drive_c/users").join(user)
}

fn substitute_paths<P: AsRef<str>>(path: P) -> PathBuf {
    let mut result = PathBuf::new();
    let path_str = path.as_ref();

    if path_str.contains("%Documents%") {
        let path = home_dir().join("Documents");
        result.push(path_str.replace("%Documents%", path.to_str().unwrap_or_default()));
    } else if path_str.contains("%SavedGames%") {
        let path = home_dir().join("Saved Games");
        result.push(path_str.replace("%SavedGames%", path.to_str().unwrap_or_default()));
    } else {
        result.push(path_str);
    }

    result
}

#[derive(Getters)]
pub struct CloudSyncLock<'a> {
    auth: &'a LockedAuthStorage,
    client: &'a Client,
    lock: String,
    manifest: CloudSyncManifest,
    mode: CloudSyncLockMode,
}

impl<'a> CloudSyncLock<'a> {
    pub async fn new(
        auth: &'a LockedAuthStorage,
        client: &'a Client,
        manifest_url: String,
        lock: String,
        mode: CloudSyncLockMode,
    ) -> Result<Self> {
        let res = client.get(manifest_url).send().await?;
        let text = res.text().await?;
        let manifest: CloudSyncManifest = quick_xml::de::from_str(&text)?;
        Ok(Self {
            auth,
            client,
            lock,
            manifest,
            mode,
        })
    }

    pub async fn release(&self) -> Result<()> {
        let (token, user_id) = acquire_auth(self.auth).await?;

        let res = self
            .client
            .put(format!("{}/lock/{}?status=commit", API_CLOUDSYNC, user_id))
            .header(AUTH_HEADER, token)
            .header(LOCK_HEADER, &self.lock)
            .send()
            .await?;

        res.text().await?;

        debug!("Released CloudSync {} {}", self.mode.key(), self.lock);
        Ok(())
    }

    pub async fn sync_files(&self) -> Result<()> {
        // This sucks.
        #[derive(Serialize)]
        struct requests {
            request: Vec<CloudSyncRequest>,
        }

        let mut value = requests {
            request: Vec::new(),
        };

        let mut paths = HashMap::new();
        for i in 0..self.manifest.file.len() {
            value.request.push(CloudSyncRequest {
                attr_id: i.to_string(),
                verb: "GET".to_owned(),
                resource: self.manifest.file[i].attr_href.to_owned(),
            });

            paths.insert(i, self.manifest.file[i].local_name.to_owned());
        }

        let (token, user_id) = acquire_auth(self.auth).await?;
        let body = quick_xml::se::to_string(&value)?;

        let res = self
            .client
            .put(format!("{}/authorize/{}", API_CLOUDSYNC, user_id))
            .header(AUTH_HEADER, token)
            .header(LOCK_HEADER, &self.lock)
            .header("Content-Type", "application/xml")
            .body(body)
            .send()
            .await?;

        let text = res.text().await?;
        let authorizations: CloudSyncAuthorizatedRequests = quick_xml::de::from_str(&text)?;

        for i in 0..authorizations.request.len() {
            let auth_req = &authorizations.request[i];
            let mut req = self.client.get(&auth_req.url);
            for ele in &auth_req.headers {
                let header = &ele.header;
                req = req.header(&header.attr_key, &header.attr_value);
            }

            let res = req.send().await?;

            let path = paths.get(&i).unwrap();
            let path = substitute_paths(path);

            debug!(
                "Downloaded CloudSync file [{:?}, {} bytes]",
                path, self.manifest.file[i].attr_size
            );

            tokio::fs::create_dir_all(path.parent().unwrap()).await?;
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
}

pub struct CloudSyncClient {
    auth: LockedAuthStorage,
    client: Client,
}

impl CloudSyncClient {
    pub fn new(auth: LockedAuthStorage) -> Self {
        Self {
            auth,
            client: ClientBuilder::default()
                .gzip(true)
                .build()
                .context("Failed to build CloudSync HTTP client")
                .unwrap(),
        }
    }

    pub async fn obtain_lock<'a>(
        &self,
        offer: &OwnedOffer,
        mode: CloudSyncLockMode,
    ) -> Result<CloudSyncLock> {
        let id = format!(
            "{}_{}",
            offer.offer().primary_master_title_id(),
            offer.offer().multiplayer_id()
        );

        Ok(self.obtain_lock_raw(&id, mode).await?)
    }

    pub async fn obtain_lock_raw<'a>(
        &self,
        id: &str,
        mode: CloudSyncLockMode,
    ) -> Result<CloudSyncLock> {
        let (token, user_id) = acquire_auth(&self.auth).await?;

        let res = self
            .client
            .post(format!(
                "{}/{}/{}/{}",
                API_CLOUDSYNC,
                mode.key(),
                user_id,
                id
            ))
            .header(AUTH_HEADER, token)
            .send()
            .await?;
        let lock = res.headers().get("x-origin-sync-lock");
        if lock.is_none() {
            bail!("Failed to acquire {}", mode.key());
        }

        let lock = lock.unwrap().to_str()?.to_owned();
        debug!("Obtained CloudSync {}: {}", mode.key(), lock);

        let text = res.text().await?;
        let sync: CloudSyncSync = quick_xml::de::from_str(&text)?;
        Ok(CloudSyncLock::new(&self.auth, &self.client, sync.manifest, lock, mode).await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::core::{auth::storage::AuthStorage, library::GameLibrary};

    use super::*;

    #[tokio::test]
    async fn read_files() -> Result<()> {
        let auth = AuthStorage::load()?;
        if !auth.lock().await.logged_in().await? {
            bail!("Test cannot run when logged out");
        }

        let mut library = GameLibrary::new(auth.clone()).await;
        let offer = library
            .game_by_base_slug("star-wars-jedi-survivor")
            .await
            .unwrap();

        let client = CloudSyncClient::new(auth);

        let lock = client.obtain_lock(offer, CloudSyncLockMode::Read).await?;
        lock.sync_files().await?;
        lock.release().await?;
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

cloudsync_type!(
    Sync;
    attr {},
    data {
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
        header: CloudSyncHeader
    }
);

cloudsync_type!(
    AuthorizatedRequest;
    attr {
        id: String,
    },
    data {
        url: String,
        headers: Vec<CloudSyncHeaderWrapper>,
    }
);

cloudsync_type!(
    AuthorizatedRequests;
    attr {},
    data {
        request: Vec<CloudSyncAuthorizatedRequest>,
    }
);
