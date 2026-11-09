// SPDX-License-Identifier: AGPL-3.0-or-later

//! Connecting like any application: a button, the browser, and nothing to copy. GitHub uses the
//! device flow; Todoist uses a public client with PKCE, so no secret ships with the application.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::source::{SourceError, check_status};

const GITHUB_BASE: &str = "https://github.com";
const GITHUB_SCOPES: &str = "repo read:org read:project";
/// Set when an OAuth App is declared for Pigeonnier; a client id is public by nature.
const GITHUB_CLIENT_ID: Option<&str> = option_env!("PIGEONNIER_GITHUB_CLIENT_ID");
const TODOIST_METADATA: &str = "https://api.todoist.com/.well-known/oauth-authorization-server";
const TODOIST_SCOPE: &str = "data:read_write";
const CLIENT_NAME: &str = "Pigeonnier";
/// Todoist compares redirect addresses exactly, so the port cannot be chosen at run time.
pub const CALLBACK_PORT: u16 = 8745;
const CALLBACK_WAIT: Duration = Duration::from_secs(300);
/// A token this close to its end is refreshed rather than used.
const EXPIRY_MARGIN_SECONDS: i64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum OauthError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("the authorization was refused")]
    Denied,
    #[error("the authorization took too long")]
    Expired,
    #[error("the browser came back with a different state: the request is ignored")]
    StateMismatch,
    #[error("unexpected answer: {0}")]
    Shape(String),
}

/// The device flow of this build, or nothing if no OAuth App is declared for it.
#[must_use]
pub fn github_device_flow() -> Option<GithubDeviceFlow> {
    let client_id = std::env::var("PIGEONNIER_GITHUB_CLIENT_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .or_else(|| GITHUB_CLIENT_ID.map(str::to_owned))?;
    Some(match std::env::var("PIGEONNIER_GITHUB_LOGIN") {
        Ok(base) => GithubDeviceFlow::with_base(client_id, base),
        Err(_) => GithubDeviceFlow::new(client_id),
    })
}

/// What the person needs to see to authorize GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub struct GithubDeviceFlow {
    http: reqwest::Client,
    base: String,
    client_id: String,
}

impl GithubDeviceFlow {
    #[must_use]
    pub fn new(client_id: String) -> Self {
        Self::with_base(client_id, GITHUB_BASE.to_owned())
    }

    #[must_use]
    pub fn with_base(client_id: String, base: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base,
            client_id,
        }
    }

    /// # Errors
    /// Any HTTP failure.
    pub async fn start(&self) -> Result<DeviceCode, OauthError> {
        let response = self
            .http
            .post(format!("{}/login/device/code", self.base))
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", GITHUB_SCOPES),
            ])
            .send()
            .await?;
        Ok(check_status(response)
            .await
            .map_err(OauthError::Source)?
            .json()
            .await?)
    }

    /// Asks GitHub until the person has answered, at the pace GitHub sets.
    ///
    /// # Errors
    /// The person refuses, the code expires, or an HTTP failure.
    pub async fn wait_for_token(&self, code: &DeviceCode) -> Result<String, OauthError> {
        let mut interval = code.interval.max(1);
        let mut waited = 0;
        while waited < code.expires_in {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            waited += interval;
            let response = self
                .http
                .post(format!("{}/login/oauth/access_token", self.base))
                .header(reqwest::header::ACCEPT, "application/json")
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("device_code", code.device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await?;
            let answer: Value = check_status(response)
                .await
                .map_err(OauthError::Source)?
                .json()
                .await?;
            if let Some(token) = answer["access_token"].as_str() {
                return Ok(token.to_owned());
            }
            match answer["error"].as_str() {
                Some("authorization_pending") => {}
                Some("slow_down") => {
                    interval = answer["interval"].as_u64().unwrap_or(interval + 5);
                }
                Some("access_denied") => return Err(OauthError::Denied),
                Some("expired_token") => return Err(OauthError::Expired),
                other => return Err(OauthError::Shape(format!("{other:?}"))),
            }
        }
        Err(OauthError::Expired)
    }
}

/// What keeps Todoist connected. Access tokens last an hour and each refresh token works once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoistSession {
    pub client_id: String,
    pub token_endpoint: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl TodoistSession {
    #[must_use]
    pub fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        self.refresh_token.is_some()
            && self
                .expires_at
                .is_some_and(|expires| (expires - now).num_seconds() < EXPIRY_MARGIN_SECONDS)
    }

    /// Exchanges the refresh token. Reusing a consumed one revokes every token of the application,
    /// so the caller must store the result before anything else can read the old session.
    ///
    /// # Errors
    /// Any HTTP failure, or an answer with no token.
    pub async fn refresh(&self, http: &reqwest::Client) -> Result<Self, OauthError> {
        let refresh_token = self
            .refresh_token
            .as_deref()
            .ok_or_else(|| OauthError::Shape("no refresh token".to_owned()))?;
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", self.client_id.as_str()),
        ];
        let answer = post_token(http, &self.token_endpoint, &form).await?;
        session_from(
            &answer,
            &self.client_id,
            &self.token_endpoint,
            self.refresh_token.as_deref(),
        )
    }
}

async fn post_token(
    http: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
) -> Result<Value, OauthError> {
    let response = http.post(endpoint).form(form).send().await?;
    Ok(check_status(response)
        .await
        .map_err(OauthError::Source)?
        .json()
        .await?)
}

fn session_from(
    answer: &Value,
    client_id: &str,
    token_endpoint: &str,
    previous_refresh: Option<&str>,
) -> Result<TodoistSession, OauthError> {
    let access_token = answer["access_token"]
        .as_str()
        .ok_or_else(|| OauthError::Shape("no access token".to_owned()))?
        .to_owned();
    let lifetime = answer["expires_in"]
        .as_i64()
        .and_then(chrono::Duration::try_seconds);
    Ok(TodoistSession {
        client_id: client_id.to_owned(),
        token_endpoint: token_endpoint.to_owned(),
        access_token,
        refresh_token: answer["refresh_token"]
            .as_str()
            .or(previous_refresh)
            .map(str::to_owned),
        expires_at: lifetime.map(|lifetime| Utc::now() + lifetime),
    })
}

/// A Todoist authorization under way: open `url`, then call [`TodoistLogin::finish`].
pub struct TodoistLogin {
    http: reqwest::Client,
    pub url: String,
    client_id: String,
    token_endpoint: String,
    verifier: String,
    state: String,
    redirect_uri: String,
    listener: TcpListener,
}

/// Registers this installation as a public client and prepares the authorization.
///
/// # Errors
/// The port is taken, or Todoist refuses the registration.
pub async fn start_todoist_login(
    metadata_url: Option<&str>,
    port: u16,
) -> Result<TodoistLogin, OauthError> {
    let http = reqwest::Client::new();
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://localhost:{port}/callback");

    let response = http
        .get(metadata_url.unwrap_or(TODOIST_METADATA))
        .send()
        .await?;
    let metadata: Value = check_status(response)
        .await
        .map_err(OauthError::Source)?
        .json()
        .await?;
    let endpoint = |name: &str| {
        metadata[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| OauthError::Shape(format!("no {name}")))
    };
    let (authorize, token_endpoint, register) = (
        endpoint("authorization_endpoint")?,
        endpoint("token_endpoint")?,
        endpoint("registration_endpoint")?,
    );

    let response = http
        .post(register)
        .json(&json!({
            "client_name": CLIENT_NAME,
            "redirect_uris": [redirect_uri],
            "scope": TODOIST_SCOPE,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .await?;
    let client: Value = check_status(response)
        .await
        .map_err(OauthError::Source)?
        .json()
        .await?;
    let client_id = client["client_id"]
        .as_str()
        .ok_or_else(|| OauthError::Shape("no client id".to_owned()))?
        .to_owned();

    let verifier = random_secret();
    let state = random_secret();
    let url = reqwest::Url::parse_with_params(
        &authorize,
        &[
            ("client_id", client_id.as_str()),
            ("scope", TODOIST_SCOPE),
            ("state", state.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_challenge", challenge_of(&verifier).as_str()),
            ("code_challenge_method", "S256"),
        ],
    )
    .map_err(|error| OauthError::Shape(error.to_string()))?
    .to_string();

    Ok(TodoistLogin {
        http,
        url,
        client_id,
        token_endpoint,
        verifier,
        state,
        redirect_uri,
        listener,
    })
}

impl TodoistLogin {
    /// Waits for the browser to come back, then exchanges the code.
    ///
    /// # Errors
    /// Nobody comes back in time, a state that does not match, a refusal, or an HTTP failure.
    pub async fn finish(self) -> Result<TodoistSession, OauthError> {
        let code = tokio::time::timeout(CALLBACK_WAIT, wait_for_code(&self.listener, &self.state))
            .await
            .map_err(|_| OauthError::Expired)??;
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", self.client_id.as_str()),
            ("code_verifier", self.verifier.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
        ];
        let answer = post_token(&self.http, &self.token_endpoint, &form).await?;
        session_from(&answer, &self.client_id, &self.token_endpoint, None)
    }
}

/// Answers one browser request. A request for anything else, such as a favicon, is not the answer
/// we wait for and does not end the wait.
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, OauthError> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buffer = vec![0; 8192];
        let read = stream.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request.split_whitespace().nth(1).unwrap_or_default();
        let Some(query) = target.strip_prefix("/callback?") else {
            stream
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .await?;
            continue;
        };
        let value = |name: &str| {
            url_pairs(query)
                .into_iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
        };
        let result = if value("state").as_deref() != Some(state) {
            Err(OauthError::StateMismatch)
        } else if let Some(code) = value("code") {
            Ok(code)
        } else {
            Err(OauthError::Denied)
        };
        let message = if result.is_ok() {
            "Pigeonnier est connecté. Tu peux fermer cette page."
        } else {
            "La connexion a échoué. Retourne dans Pigeonnier pour réessayer."
        };
        let page =
            format!("<!doctype html><meta charset=utf-8><title>Pigeonnier</title><p>{message}</p>");
        let answer = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        stream.write_all(answer.as_bytes()).await?;
        return result;
    }
}

fn url_pairs(query: &str) -> Vec<(String, String)> {
    reqwest::Url::parse(&format!("http://localhost/?{query}"))
        .map(|url| url.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

fn random_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// The S256 challenge of PKCE: unpadded URL-safe base64 of the SHA-256 of the verifier.
fn challenge_of(verifier: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let digest = Sha256::digest(verifier.as_bytes());
    let mut text = String::with_capacity(43);
    for chunk in digest.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let number = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for index in 0..=chunk.len() {
            let position = (number >> (18 - 6 * index)) & 63;
            text.push(char::from(ALPHABET[position as usize]));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_the_example_of_the_pkce_rfc() {
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_session_refreshes_just_before_it_ends() {
        let now = Utc::now();
        let mut session = TodoistSession {
            client_id: "c".to_owned(),
            token_endpoint: "t".to_owned(),
            access_token: "a".to_owned(),
            refresh_token: Some("r".to_owned()),
            expires_at: Some(now + chrono::Duration::minutes(30)),
        };
        assert!(!session.needs_refresh(now));
        session.expires_at = Some(now + chrono::Duration::seconds(10));
        assert!(session.needs_refresh(now));
        session.refresh_token = None;
        assert!(!session.needs_refresh(now));
    }
}
