use crate::core::{auth::storage::TokenError, endpoints::API_NUCLEUS_TOKENINFO};
use derive_getters::Getters;
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize, Getters)]
pub struct NucleusTokenInfo {
    client_id: String,
    scope: String,
    expires_in: u32,
    pid_id: String,
    pid_type: String,
    user_id: String,
    persona_id: Option<u64>,
    console_env: Option<String>,
    is_underage: Option<bool>,
}

impl NucleusTokenInfo {
    pub async fn fetch(client: &Client, access_token: &str) -> Result<Self, TokenError> {
        let res = client
            .get(API_NUCLEUS_TOKENINFO)
            .query(&[("access_token", access_token)])
            .send()
            .await?
            .error_for_status()?;

        let text = &res.text().await?;
        Ok(serde_json::from_str(text)?)
    }
}
