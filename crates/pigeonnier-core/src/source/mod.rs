// SPDX-License-Identifier: AGPL-3.0-or-later

//! The first step: fetch items from a service, untouched, with where each one came from.

pub mod github;
pub mod todoist;

use serde::Serialize;
use serde_json::Value;

/// Where an item came from: enough to find it again and to notice it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    pub source: &'static str,
    pub id: String,
    /// The service's update time; none of the sources gives a real version number.
    pub version: Option<String>,
}

/// An item as the service returned it.
#[derive(Debug, Clone, PartialEq)]
pub struct RawItem {
    pub provenance: Provenance,
    pub payload: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("no token: {0}")]
    MissingToken(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("{}", status_text(*status, body))]
    Status { status: u16, body: String },
    #[error("GraphQL: {0}")]
    GraphQl(String),
    #[error("unexpected response: {0}")]
    Shape(String),
}

/// A failed answer in a few words: a service that does not respond says so, and a body that is an
/// HTML page, as GitHub sends when it times out, is left out.
fn status_text(status: u16, body: &str) -> String {
    if (500..600).contains(&status) {
        return format!(
            "le service ne répond pas (HTTP {status}) ; nouvel essai au prochain rafraîchissement"
        );
    }
    if body.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body}")
    }
}

/// What to keep of an error body: nothing of an HTML page, the first few hundred characters of the
/// rest.
fn summarize(body: &str) -> String {
    let body = body.trim();
    let start: String = body.chars().take(15).collect::<String>().to_lowercase();
    if start.starts_with("<!doctype") || start.starts_with("<html") {
        return String::new();
    }
    const LIMIT: usize = 300;
    if body.chars().count() > LIMIT {
        let cut: String = body.chars().take(LIMIT).collect();
        format!("{cut}…")
    } else {
        body.to_owned()
    }
}

impl SourceError {
    /// A failure that may not happen again: the service is overloaded, timed out or limits the rate.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Status { status, .. } => *status == 429 || (500..600).contains(status),
            Self::Http(error) => error.is_timeout() || error.is_connect(),
            _ => false,
        }
    }

    /// The token was refused, so the UI must offer to reconnect rather than just report a failure.
    #[must_use]
    pub fn needs_reconnect(&self) -> bool {
        matches!(
            self,
            Self::MissingToken(_)
                | Self::Status {
                    status: 401 | 403,
                    ..
                }
        )
    }
}

/// Turns a non-success response into [`SourceError::Status`], keeping the body the service explains
/// itself in.
pub(crate) async fn check_status(
    response: reqwest::Response,
) -> Result<reqwest::Response, SourceError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(SourceError::Status {
        status: status.as_u16(),
        body: summarize(&body),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_html_error_page_is_left_out_and_a_timeout_says_so() {
        let page = "<!DOCTYPE html>\n<!--\nHello future GitHubber!";
        assert_eq!(summarize(page), "");
        let error = SourceError::Status {
            status: 504,
            body: summarize(page),
        };
        assert!(error.is_transient());
        assert_eq!(
            error.to_string(),
            "le service ne répond pas (HTTP 504) ; nouvel essai au prochain rafraîchissement"
        );
        let refused = SourceError::Status {
            status: 401,
            body: summarize("Bad credentials"),
        };
        assert!(!refused.is_transient());
        assert_eq!(refused.to_string(), "HTTP 401: Bad credentials");
        assert!(summarize(&"x".repeat(1000)).ends_with('…'));
    }
}
