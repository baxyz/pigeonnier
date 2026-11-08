// SPDX-License-Identifier: AGPL-3.0-or-later

//! Todoist `/api/v1/sync`: one full sync, then only what changed.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::{Provenance, RawItem, SourceError, check_status};

const DEFAULT_BASE: &str = "https://api.todoist.com";
const SOURCE: &str = "todoist:item";
const RESOURCE_TYPES: &str = r#"["items","sections","projects","labels"]"#;
/// The `sync_token` that asks for a full sync.
pub const FULL_SYNC: &str = "*";

#[derive(Debug, Clone, Deserialize)]
pub struct SyncResponse {
    pub sync_token: String,
    pub full_sync: bool,
    pub full_sync_date_utc: Option<String>,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub sections: Vec<Value>,
    #[serde(default)]
    pub projects: Vec<Value>,
    #[serde(default)]
    pub labels: Vec<Value>,
}

pub struct TodoistClient {
    http: reqwest::Client,
    token: String,
    base: String,
}

impl TodoistClient {
    #[must_use]
    pub fn new(token: String) -> Self {
        Self::with_base(token, DEFAULT_BASE.to_owned())
    }

    /// Points at another server: recorded responses in tests, or a local stand-in.
    #[must_use]
    pub fn with_base(token: String, base: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token,
            base,
        }
    }

    /// # Errors
    /// Any HTTP failure, or a body that is not a sync response.
    pub async fn sync(&self, sync_token: &str) -> Result<SyncResponse, SourceError> {
        let response = self
            .http
            .post(format!("{}/api/v1/sync", self.base))
            .bearer_auth(&self.token)
            .form(&[
                ("sync_token", sync_token),
                ("resource_types", RESOURCE_TYPES),
            ])
            .send()
            .await?;
        Ok(check_status(response).await?.json().await?)
    }
}

/// What a client keeps between syncs. Resources are keyed by id, because an incremental response
/// only carries the ones that changed.
#[derive(Debug, Clone, Default)]
pub struct SyncState {
    pub token: Option<String>,
    pub items: BTreeMap<String, Value>,
    pub sections: BTreeMap<String, Value>,
    pub projects: BTreeMap<String, Value>,
    pub labels: BTreeMap<String, Value>,
}

impl SyncState {
    /// The token to send next: the last one received, or a full sync.
    #[must_use]
    pub fn next_token(&self) -> &str {
        self.token.as_deref().unwrap_or(FULL_SYNC)
    }

    pub fn apply(&mut self, response: SyncResponse) {
        if response.full_sync {
            *self = Self::default();
        }
        merge(&mut self.items, response.items);
        merge(&mut self.sections, response.sections);
        merge(&mut self.projects, response.projects);
        merge(&mut self.labels, response.labels);
        self.token = Some(response.sync_token);
    }

    #[must_use]
    pub fn raw_items(&self) -> Vec<RawItem> {
        self.items
            .iter()
            .map(|(id, item)| RawItem {
                provenance: Provenance {
                    source: SOURCE,
                    id: id.clone(),
                    version: item["updated_at"].as_str().map(str::to_owned),
                },
                payload: item.clone(),
            })
            .collect()
    }
}

fn merge(known: &mut BTreeMap<String, Value>, changes: Vec<Value>) {
    for change in changes {
        let Some(id) = change["id"].as_str().map(str::to_owned) else {
            continue;
        };
        if change["is_deleted"] == Value::Bool(true) {
            known.remove(&id);
        } else {
            known.insert(id, change);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn response(full_sync: bool, token: &str, items: Vec<Value>) -> SyncResponse {
        SyncResponse {
            sync_token: token.to_owned(),
            full_sync,
            full_sync_date_utc: None,
            items,
            sections: Vec::new(),
            projects: Vec::new(),
            labels: Vec::new(),
        }
    }

    #[test]
    fn an_incremental_sync_updates_adds_and_deletes() {
        let mut state = SyncState::default();
        assert_eq!(state.next_token(), FULL_SYNC);
        state.apply(response(
            true,
            "t1",
            vec![
                json!({ "id": "1", "content": "a" }),
                json!({ "id": "2", "content": "b" }),
            ],
        ));
        assert_eq!(state.next_token(), "t1");
        state.apply(response(
            false,
            "t2",
            vec![
                json!({ "id": "1", "content": "a2" }),
                json!({ "id": "2", "is_deleted": true }),
                json!({ "id": "3", "content": "c" }),
            ],
        ));
        assert_eq!(state.next_token(), "t2");
        assert_eq!(state.items["1"]["content"], "a2");
        assert!(!state.items.contains_key("2"));
        assert!(state.items.contains_key("3"));
    }

    #[test]
    fn a_full_sync_forgets_what_it_does_not_list() {
        let mut state = SyncState::default();
        state.apply(response(true, "t1", vec![json!({ "id": "1" })]));
        state.apply(response(true, "t2", vec![json!({ "id": "2" })]));
        assert_eq!(state.items.keys().collect::<Vec<_>>(), ["2"]);
    }
}
