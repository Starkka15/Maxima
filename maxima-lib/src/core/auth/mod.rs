pub mod hardware;
pub mod login;

use anyhow::{bail, Result};
use reqwest::{redirect, Client, Url};
use serde::Deserialize;

use super::endpoints::{API_NUCLEUS_AUTH, API_NUCLEUS_TOKEN};

pub async fn execute_auth_exchange(
    access_token: &str,
    client_id: &str,
    mut response_type: &str,
) -> Result<String> {
    let query = vec![
        ("client_id", client_id),
        ("response_type", response_type),
        // Need to figure out how this is generated, see login.rs for more info
        ("pc_sign", "eyJhdiI6InYxIiwiYnNuIjoiRGVmYXVsdCBzdHJpbmciLCJnaWQiOjc5NDQsImhzbiI6IkU4MjNfOEZBNl9CRjUzXzAwMDFfMDAxQl80NDhCXzRBODVfOUM5Qi4iLCJtYWMiOiIkMDAxNTVkZDMxNzc0IiwibWlkIjoiNzg3NTE0NTgxMTIwNzEzMDY1NiIsIm1zbiI6IkRlZmF1bHQgc3RyaW5nIiwic3YiOiJ2MSIsInRzIjoiMjAyMy05LTE5IDA6Mjc6MDozNjEifQ.ZTJhxK5bcX_2ApICzT3RKJspUnfl44q0CeVky0_MPGw"),
        ("access_token", access_token),
    ];

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client
        .get(API_NUCLEUS_AUTH)
        .query(&query)
        .send()
        .await?
        .error_for_status()?;

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
        url.fragment()
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

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u32,
    refresh_token: String,
    refresh_token_expires_in: u32,
}

// Unfinished
pub async fn execute_connect_token(_code: &str) -> Result<TokenResponse> {
    let query = vec![("grant_type", "authorization_code")];

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client
        .get(API_NUCLEUS_TOKEN)
        .form(&query)
        .send()
        .await?
        .error_for_status()?;

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
    let _query = if use_fragment {
        url.fragment()
    } else {
        url.query()
    };

    let response: TokenResponse = serde_json::from_str(&res.text().await?)?;
    Ok(response)
}
