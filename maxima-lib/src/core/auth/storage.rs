use std::{
    collections::HashMap,
    fs,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::util::native::maxima_dir;

use super::{token_info::NucleusTokenInfo, TokenResponse};

const FILE: &str = "auth.toml";

#[derive(Default, Serialize, Deserialize)]
pub struct AuthAccount {
    #[serde(skip_serializing, skip_deserializing)]
    client: Client,
    access_token: String,
    refresh_token: String,
    /// Expiry time in seconds since epoch
    expires_at: u64,
    user_id: String,
}

impl AuthAccount {
    async fn from_token_response(response: &TokenResponse) -> Result<Self> {
        let client = Client::new();

        let secs_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let expires_at = secs_since_epoch + response.expires_in();

        let access_token = response.access_token().to_owned();
        let token_info = NucleusTokenInfo::fetch(&client, &access_token).await?;

        Ok(Self {
            client: Client::new(),
            access_token,
            refresh_token: response.refresh_token().to_owned(),
            expires_at,
            user_id: token_info.user_id().to_owned(),
        })
    }

    pub async fn access_token(&mut self) -> Result<&str> {
        // If the key is expired (or is about to be), refresh
        let secs_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if secs_since_epoch >= self.expires_at - 10 {
            self.refresh().await;
        }

        Ok(&self.access_token)
    }

    pub async fn validate(&mut self) -> Result<bool> {
        let access_token = self.access_token().await?.to_owned();
        let token_info = NucleusTokenInfo::fetch(&self.client, &access_token).await;
        if token_info.is_err() {
            return Ok(false);
        }

        if self.user_id != *token_info.unwrap().user_id() {
            return Ok(false);
        }

        Ok(true)
    }

    async fn refresh(&mut self) {
        todo!();
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct AuthStorage {
    accounts: HashMap<String, AuthAccount>,
    selected: Option<String>,
}

pub type LockedAuthStorage = Arc<Mutex<AuthStorage>>;

impl AuthStorage {
    pub(crate) fn load() -> Result<LockedAuthStorage> {
        let file = maxima_dir()?.join(FILE);
        if !file.exists() {
            return Ok(Arc::new(Mutex::new(Self::default())));
        }

        let data = fs::read_to_string(file)?;
        Ok(Arc::new(Mutex::new(toml::from_str(&data)?)))
    }

    pub(crate) fn save(&self) -> Result<()> {
        let file = maxima_dir()?.join(FILE);
        fs::write(file, toml::to_string(&self)?)?;
        Ok(())
    }

    pub async fn logged_in(&mut self) -> Result<bool> {
        Ok(match self.current() {
            Some(account) => account.validate().await?,
            None => false,
        })
    }

    pub fn current(&mut self) -> Option<&mut AuthAccount> {
        match &self.selected {
            Some(selected) => self.accounts.get_mut(selected),
            None => None,
        }
    }

    /// Add an account from a token response and set it as the currently selected one
    pub async fn add_account(&mut self, response: &TokenResponse) -> Result<()> {
        let account = AuthAccount::from_token_response(response).await?;
        let user_id = account.user_id.to_owned();

        self.accounts.insert(user_id.to_owned(), account);
        self.selected = Some(user_id);

        self.save()?;
        Ok(())
    }
}
