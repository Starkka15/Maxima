pub mod context;
pub mod hardware;
pub mod login;
pub mod pc_sign;
pub mod storage;
pub mod token_info;

use super::{
    clients::{JUNO_PC_CLIENT_ID, JUNO_PC_CLIENT_SECRET},
    endpoints::API_NUCLEUS_TOKEN,
};
use crate::core::auth::storage::{AuthError, TokenError};
use context::AuthContext;
use derive_getters::Getters;
use reqwest::{redirect, Client, Url};
use serde::Deserialize;
use thiserror::Error;

pub async fn nucleus_auth_exchange<'a>(
    auth_context: &AuthContext<'a>,
    client_id: &str,
    mut response_type: &str,
) -> Result<String, AuthError> {
    if auth_context.access_token().is_none() {
        return Err(AuthError::NoToken);
    }

    let url: String = auth_context.nucleus_auth_url(client_id, response_type)?;

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client.get(url).send().await?.error_for_status()?;

    if !res.status().is_redirection() {
        return Err(AuthError::InvalidRedirect(None));
    }

    let mut redirect_url = res
        .headers()
        .get("location")
        .ok_or(AuthError::Header("location".to_string()))?
        .to_str()?
        .to_owned();

    // Failed, the user either has 2fa enabled or something went wrong
    if redirect_url.starts_with("https://signin.ea.com") {
        return Err(AuthError::InvalidRedirect(Some(redirect_url.to_string())));
    }

    // The Url crate doesn't like custom protocols :(
    let use_fragment = redirect_url.starts_with("qrc");
    if use_fragment {
        redirect_url = redirect_url.replace("qrc:/html", "http://127.0.0.1");
    }

    let url = Url::parse(&redirect_url)?;
    let query = if use_fragment {
        url.fragment().or(url.query())
    } else {
        url.query()
    };

    let query = querystring::querify(query.ok_or(AuthError::Query)?);

    if response_type == "token" {
        response_type = "access_token";
    }

    let token = query
        .iter()
        .find(|(x, _)| *x == response_type)
        .ok_or(AuthError::Query)?
        .1;
    Ok(token.to_owned())
}

#[derive(Error, Debug)]
pub enum TokenRefreshError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Deserialization(#[from] serde_json::Error),

    #[error("token `{refresh_token}` could not be refreshed: {error}")]
    API {
        error: String,
        refresh_token: String,
    },
}

#[derive(Debug, Deserialize, Getters)]
pub struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

pub async fn nucleus_token_exchange(
    auth_context: &AuthContext<'_>,
) -> Result<TokenResponse, TokenError> {
    assert!(auth_context.code().is_some());

    let query = vec![
        ("grant_type", "authorization_code"),
        ("code", &auth_context.code().ok_or(TokenError::Absent)?),
        ("code_verifier", &auth_context.code_verifier()),
        ("client_id", JUNO_PC_CLIENT_ID),
        ("client_secret", JUNO_PC_CLIENT_SECRET),
        ("redirect_uri", "qrc:///html/login_successful.html"),
        ("token_format", "JWS"), // Force JWT for Kyber
    ];

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client.post(API_NUCLEUS_TOKEN).form(&query).send().await?;

    let status = res.status();
    let text = res.text().await?;
    if status.is_client_error() || status.is_server_error() {
        return Err(TokenError::Exchange(text));
    }

    let response: TokenResponse = serde_json::from_str(&text)?;
    Ok(response)
}

pub async fn nucleus_connect_token_refresh(
    refresh_token: &str,
) -> Result<TokenResponse, TokenRefreshError> {
    let query = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", JUNO_PC_CLIENT_ID),
        ("client_secret", JUNO_PC_CLIENT_SECRET),
    ];

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client.post(API_NUCLEUS_TOKEN).form(&query).send().await?;

    let status = res.status();
    let text = res.text().await?;
    if status.is_client_error() || status.is_server_error() {
        return Err(TokenRefreshError::API {
            error: text,
            refresh_token: refresh_token.to_owned(),
        });
    }

    let response: TokenResponse = serde_json::from_str(&text)?;
    Ok(response)
}
