use super::{
    nucleus_connect_token_refresh, token_info::NucleusTokenInfo, TokenRefreshError, TokenResponse,
};
use crate::core::auth::hardware::HardwareHashError;
use crate::ooa::LicenseError;
use crate::util::native::{maxima_dir, NativeError};
use log::info;
use reqwest::header::ToStrError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    sync::Arc,
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::sync::Mutex;

const FILE: &str = "auth.toml";

#[derive(Error, Debug)]
pub enum TokenError {
    #[error(transparent)]
    Time(#[from] SystemTimeError),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    TomlSerialization(#[from] toml::ser::Error),
    #[error(transparent)]
    JsonSerialization(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Refresh(#[from] TokenRefreshError),

    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("a refresh token was not provided")]
    NoRefresh,
    #[error("an access token was not provided")]
    Absent,
}

#[derive(Error, Debug)]
pub enum AuthError {
    #[error(transparent)]
    Token(#[from] TokenError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    UrlParse(#[from] url::ParseError),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    PCSign(#[from] HardwareHashError),
    #[error(transparent)]
    HeaderStr(#[from] http::header::ToStrError),

    #[error("no token was provided")]
    NoToken,
    #[error("failed to find auth code")]
    NoAuthCode,
    #[error("could not find header `{0}`")]
    Header(String),
    #[error("missing query")]
    Query,
    #[error("invalid redirect or chain `{0:?}`")]
    InvalidRedirect(Option<String>),
}

#[derive(Default, Serialize, Deserialize)]
pub struct AuthAccount {
    #[serde(skip_serializing, skip_deserializing)]
    client: Client,
    #[serde(skip_serializing, skip_deserializing)]
    dirty: bool,

    access_token: String,
    refresh_token: String,
    /// Expiry time in seconds since epoch
    expires_at: u64,
    user_id: String,
}

impl AuthAccount {
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    fn from_token(token: &str) -> Self {
        Self {
            access_token: token.to_owned(),
            expires_at: u64::MAX,
            ..Default::default()
        }
    }

    async fn from_token_response(response: &TokenResponse) -> Result<Self, TokenError> {
        let mut account = Self::default();
        account.parse_token_response(response).await?;
        Ok(account)
    }

    async fn parse_token_response(&mut self, response: &TokenResponse) -> Result<(), TokenError> {
        let secs_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let expires_at = secs_since_epoch + response.expires_in();

        self.access_token = response.access_token().to_owned();
        if let Some(refresh_token) = response.refresh_token() {
            self.refresh_token = refresh_token.to_owned();
        } else {
            return Err(TokenError::NoRefresh);
        }

        self.expires_at = expires_at;

        if self.user_id.is_empty() {
            let token_info = NucleusTokenInfo::fetch(&self.client, &self.access_token).await?;
            self.user_id = token_info.user_id().to_owned();
        }

        self.dirty = true;
        Ok(())
    }

    async fn access_token(&mut self) -> Result<&str, TokenError> {
        // If the key is expired (or is about to be), refresh
        let secs_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if secs_since_epoch >= self.expires_at - 10 {
            self.refresh().await?;
        }

        Ok(&self.access_token)
    }

    async fn validate(&mut self) -> Result<bool, TokenError> {
        let access_token = self.access_token().await;
        if access_token.is_err() {
            return Ok(false);
        }

        let access_token = access_token?.to_owned();
        let token_info = NucleusTokenInfo::fetch(&self.client, &access_token).await;
        if token_info.is_err() {
            return Ok(false);
        }

        if self.user_id != *token_info?.user_id() {
            return Ok(false);
        }

        Ok(true)
    }

    async fn refresh(&mut self) -> Result<(), TokenError> {
        info!("Attempting token refresh...");

        let token_res = nucleus_connect_token_refresh(&self.refresh_token).await?;
        self.parse_token_response(&token_res).await?;
        Ok(())
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

#[derive(Serialize, Deserialize)]
pub struct AuthStorage {
    accounts: HashMap<String, AuthAccount>,
    selected: Option<String>,

    #[serde(skip_serializing, skip_deserializing)]
    can_save: bool,
}

pub type LockedAuthStorage = Arc<Mutex<AuthStorage>>;

impl Default for AuthStorage {
    fn default() -> Self {
        Self {
            accounts: HashMap::new(),
            selected: None,
            can_save: true,
        }
    }
}

impl AuthStorage {
    /// Directly create AuthStorage from a token response
    pub async fn from_token_response(
        response: &TokenResponse,
    ) -> Result<LockedAuthStorage, AuthError> {
        let account = AuthAccount::from_token_response(response).await?;

        let storage = Self {
            accounts: HashMap::from([("direct".to_owned(), account)]),
            selected: Some("direct".to_owned()),
            can_save: false,
        };

        Ok(Arc::new(Mutex::new(storage)))
    }

    /// This is to be used only in circumstances where you want
    /// to make a single request to a single system with a
    /// single account. This will not be persisted, and
    /// saving and refreshing is disabled.
    pub fn from_token(token: &str) -> LockedAuthStorage {
        let account = AuthAccount::from_token(token);

        let storage = Self {
            accounts: HashMap::from([("direct".to_owned(), account)]),
            selected: Some("direct".to_owned()),
            can_save: false,
        };

        Arc::new(Mutex::new(storage))
    }

    pub fn new() -> LockedAuthStorage {
        Arc::new(Mutex::new(Self {
            accounts: HashMap::new(),
            selected: None,
            can_save: false,
        }))
    }

    pub fn load() -> Result<LockedAuthStorage, AuthError> {
        let file = maxima_dir()?.join(FILE);
        if !file.exists() {
            return Ok(Arc::new(Mutex::new(Self::default())));
        }

        let data = fs::read_to_string(file)?;
        let mut storage: AuthStorage = toml::from_str::<AuthStorage>(&data).unwrap_or_else(|err| {
            log::error!("Failed to parse auth storage file: `{:?}`", err);
            Self::default()
        });

        storage.can_save = true;
        Ok(Arc::new(Mutex::new(storage)))
    }

    pub fn save(&self) -> Result<(), TokenError> {
        let file = maxima_dir()?.join(FILE);
        fs::write(file, toml::to_string(&self)?)?;
        Ok(())
    }

    pub async fn logged_in(&mut self) -> Result<bool, AuthError> {
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

    pub async fn user_id(&mut self) -> Result<Option<String>, AuthError> {
        let current = match self.current() {
            Some(current) => current,
            None => return Ok(None),
        };

        let user_id = current.user_id().to_owned();
        self.save_if_dirty()?;

        Ok(Some(user_id))
    }

    pub async fn access_token(&mut self) -> Result<Option<String>, TokenError> {
        let current = match self.current() {
            Some(current) => current,
            None => return Ok(None),
        };

        let access_token = current.access_token().await?.to_owned();
        self.save_if_dirty()?;

        Ok(Some(access_token))
    }

    /// Add an account from a token response and set it as the currently selected one
    pub async fn add_account(&mut self, response: &TokenResponse) -> Result<(), AuthError> {
        let mut account = AuthAccount::from_token_response(response).await?;
        let user_id = account.user_id.to_owned();

        if self.accounts.contains_key(&user_id) {
            info!("Marking account dirty");
            account.mark_dirty();
        }

        self.accounts.insert(user_id.to_owned(), account);
        self.selected = Some(user_id);

        self.save_if_dirty()?;
        Ok(())
    }

    fn save_if_dirty(&self) -> Result<(), TokenError> {
        if !self.can_save {
            return Ok(());
        }

        let needed = self.accounts.iter().any(|a| a.1.dirty);
        if needed {
            self.save()?;
        }

        Ok(())
    }
}
