use anyhow::{bail, Result};
use lazy_static::lazy_static;
use regex::Regex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;

use crate::core::clients::JUNO_PC_CLIENT_ID;

use super::context::AuthContext;

lazy_static! {
    static ref HTTP_PATTERN: Regex =
        Regex::new(r"^([A-Za-z]+) +(.*) +(HTTP/[0-9][.][0-9])").unwrap();
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

// Use the OOA API to retrieve an access token without a captcha
#[deprecated(note = "This method of login was patched and this function will be removed soon")]
pub async fn manual_login(_persona: &str, _password: &str) -> Result<String> {
    unimplemented!();
}
