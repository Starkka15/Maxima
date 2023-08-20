pub mod login;

use anyhow::{bail, Result};
use reqwest::{redirect, Client};

use super::endpoints::API_NUCLEUS;

pub async fn get_auth_code(access_token: &str, client_id: &str) -> Result<String> {
    let query = vec![
        ("client_id", client_id),
        ("response_type", "code"),
        ("access_token", access_token),
    ];

    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()?;
    let res = client
        .get(API_NUCLEUS)
        .query(&query)
        .send()
        .await?
        .error_for_status()?;

    if !res.status().is_redirection() {
        bail!("Failed to get auth code");
    }

    let redirect_url = res.headers().get("location").unwrap().to_str().unwrap();
    let parts = redirect_url.split("?code=").collect::<Vec<&str>>();
    Ok(parts[1].to_owned())
}
