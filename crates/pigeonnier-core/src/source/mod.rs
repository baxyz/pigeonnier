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
    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("GraphQL: {0}")]
    GraphQl(String),
    #[error("unexpected response: {0}")]
    Shape(String),
}

impl SourceError {
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
        body,
    })
}
