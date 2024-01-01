use anyhow::{bail, Result};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::StatusCode;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use ureq::OrAnyStatus;

use crate::core::clients::JUNO_PC_CLIENT_ID;
use crate::core::endpoints::API_PROXY_NOVAFUSION_LICENSES;

use super::context::AuthContext;

lazy_static! {
    static ref EMAIL_PATTERN: Regex = Regex::new(
        r"^([a-z0-9_+]([a-z0-9_+.]*[a-z0-9_+])?)@([a-z0-9]+([\-\.]{1}[a-z0-9]+)*\.[a-z]{2,6})"
    )
    .unwrap();
    
    static ref HTTP_PATTERN: Regex = Regex::new(
        r"^([A-Za-z]+) +(.*) +(HTTP/[0-9][.][0-9])"
    )
    .unwrap();
}

pub async fn begin_oauth_login_flow<'a>(context: &mut AuthContext<'a>) -> Result<()> {
    open::that(context.nucleus_auth_url(JUNO_PC_CLIENT_ID, "code")?)?;
    let listener = TcpListener::bind("127.0.0.1:31033").await?;

    loop {
        let (mut socket, _) = listener.accept().await?;

        let (read, _) = socket.split();
        let mut reader = BufReader::new(read);

        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let captures = HTTP_PATTERN.captures(&line);
        if captures.is_none() {
            continue;
        }

        let path_and_query = captures.unwrap().get(2).unwrap().as_str();
        if path_and_query.starts_with("/auth") {
            let query = path_and_query
                .split_once("?")
                .map(|(_, qs)| qs.trim())
                .map(querystring::querify)
                .unwrap();

            for query in query {
                if query.0 == "code" {
                    context.set_code(query.1);
                    return Ok(());
                }
            }

            bail!("Failed to find auth code");
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NovaLoginValue {
    #[serde(rename = "@value")]
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NovaLoginErrorCode {
    InvalidPassword,
    ValidationFailed,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NovaLoginError {
    #[serde(rename = "@code")]
    pub code: NovaLoginErrorCode,
    pub auth_token: Option<NovaLoginValue>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NovaLoginResponse {
    pub error: NovaLoginError,
}

// Use the OOA API to retrieve an access token without a captcha
pub async fn manual_login(persona: &str, password: &str) -> Result<String> {
    let mut query = Vec::new();
    query.push(("contentId", "1"));

    if EMAIL_PATTERN.is_match(persona) {
        query.push(("ea_email", persona));
    } else {
        query.push(("ea_persona", persona));
    }

    query.push(("ea_password", password));

    let res = ureq::get(API_PROXY_NOVAFUSION_LICENSES)
        .query_pairs(query)
        .call()
        .or_any_status()?;
    if res.status() != StatusCode::CONFLICT {
        bail!("License API did not acknowledge login request properly");
    }

    let error: NovaLoginError = quick_xml::de::from_str(&res.into_string()?).unwrap();
    if error.code != NovaLoginErrorCode::ValidationFailed {
        bail!("{:?}", error.code);
    }

    Ok(error.auth_token.unwrap().value)
}
