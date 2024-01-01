use anyhow::Result;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::random;
use ring::hmac::HMAC_SHA256;
use sha2_const::Sha256;

use crate::core::clients::JUNO_PC_CLIENT_ID;
use crate::core::endpoints::API_NUCLEUS_AUTH;

use super::pc_sign::PCSign;

/// Context with utilities for auth flow
pub struct AuthContext<'a> {
    code_verifier: String,
    code_challenge: String,
    code: Option<String>,
    access_token: Option<String>,
    pc_sign: PCSign<'a>,
}

impl AuthContext<'_> {
    pub fn new() -> anyhow::Result<Self> {
        let verifier = Self::generate_code_verifier();
        let challenge = Self::generate_challenge(&verifier);
        let signature = PCSign::new()?;
        Ok(Self {
            code_verifier: verifier,
            code: None,
            access_token: None,
            code_challenge: challenge,
            pc_sign: signature,
        })
    }

    /// Generates 32 byte long buffer used for code_verifier
    fn generate_code_verifier() -> String {
        let rand_bytes: [u8; 32] = random();

        URL_SAFE_NO_PAD.encode(&rand_bytes)
    }

    fn generate_challenge(code_verifier: &String) -> String {
        let hash = Sha256::new().update(code_verifier.as_bytes()).finalize();

        URL_SAFE_NO_PAD.encode(hash)
    }

    pub fn generate_pc_sign(&self) -> String {
        let json_formatted_sign = serde_json::to_string(&self.pc_sign).unwrap();
        let payload = URL_SAFE_NO_PAD.encode(json_formatted_sign.as_bytes());

        let key = ring::hmac::Key::new(HMAC_SHA256, self.pc_sign.sign_key());
        let value = ring::hmac::sign(&key, payload.as_bytes());

        payload.to_string() + "." + URL_SAFE_NO_PAD.encode(value).as_ref()
    }

    /// Returns String representation of code_verifier
    pub fn code_verifier(&self) -> &str {
        &self.code_verifier
    }

    pub fn code(&self) -> Option<&str> {
        match &self.code {
            Some(code) => Some(&code),
            None => None,
        }
    }

    pub fn set_code(&mut self, code: &str) {
        self.code = Some(code.to_owned())
    }

    pub fn access_token(&self) -> Option<&str> {
        match &self.access_token {
            Some(access_token) => Some(&access_token),
            None => None,
        }
    }

    pub fn set_access_token(&mut self, token: &str) {
        self.access_token = Some(token.to_owned())
    }

    pub fn nucleus_auth_url(&self, client_id: &str, response_type: &str) -> Result<String> {
        let signature = self.generate_pc_sign();
        let nonce = random::<i32>().to_string();

        let mut query = vec![
            ("client_id", client_id),
            ("sbiod_enabled", "false"),
            ("response_type", response_type),
            ("locale", "en_US"),
            ("pc_sign", &signature),
            ("nonce", &nonce),
        ];

        if client_id == JUNO_PC_CLIENT_ID {
            query.push(("code_challenge_method", "S256"));
            query.push(("code_challenge", &self.code_challenge));
        }

        if let Some(access_token) = &self.access_token {
            query.push(("access_token", access_token));
        }

        let url = reqwest::Url::parse_with_params(API_NUCLEUS_AUTH, query)?;
        Ok(url.to_string())
    }
}
