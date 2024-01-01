pub mod context;
pub mod hardware;
pub mod login;
pub mod pc_sign;
pub mod storage;
pub mod token_info;

use anyhow::{bail, Result};
use context::AuthContext;
use derive_getters::Getters;
use reqwest::{redirect, Client, Url};
use serde::Deserialize;

use super::{
    clients::{JUNO_PC_CLIENT_ID, JUNO_PC_CLIENT_SECRET},
    endpoints::API_NUCLEUS_TOKEN,
};

pub async fn execute_auth_exchange<'a>(
    auth_context: &AuthContext<'a>,
    client_id: &str,
    mut response_type: &str,
) -> Result<String> {
    if auth_context.access_token().is_none() {
        bail!("To execute an auth exchange you must provide an access token in the auth context");
    }

    let url: String = auth_context.nucleus_auth_url(client_id, response_type)?;

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client.get(url).send().await?.error_for_status()?;

    if !res.status().is_redirection() {
        bail!("Failed to get auth code");
    }

    let mut redirect_url = res
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Failed, the user either has 2fa enabled or something went wrong
    if redirect_url.starts_with("https://signin.ea.com") {
        bail!("Auth exchange failed because 2FA is enabled");
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

    let query = querystring::querify(query.unwrap());

    if response_type == "token" {
        response_type = "access_token";
    }

    let token = query.iter().find(|(x, _)| *x == response_type).unwrap().1;
    Ok(token.to_owned())
}

#[derive(Debug, Deserialize, Getters)]
pub struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: String,
}

pub async fn execute_connect_token(auth_context: &AuthContext<'_>) -> Result<TokenResponse> {
    assert!(auth_context.code().is_some());

    let query = vec![
        ("grant_type", "authorization_code"),
        ("code", &auth_context.code().unwrap()),
        ("code_verifier", &auth_context.code_verifier()),
        ("client_id", JUNO_PC_CLIENT_ID),
        ("client_secret", JUNO_PC_CLIENT_SECRET),
        ("redirect_uri", "qrc:///html/login_successful.html"),
    ];

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client.post(API_NUCLEUS_TOKEN).form(&query).send().await?;

    let status = res.status();
    if status.is_client_error() || status.is_server_error() {
        bail!(
            "Token exchange failed with code {}: {}",
            auth_context.code().unwrap(),
            res.text().await?
        );
    }

    let response: TokenResponse = serde_json::from_str(&res.text().await?)?;
    Ok(response)
}
