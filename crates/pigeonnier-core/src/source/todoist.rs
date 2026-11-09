// SPDX-License-Identifier: AGPL-3.0-or-later

//! Todoist `/api/v1/sync`: one full sync, then only what changed, and the commands that write.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub sections: Vec<Value>,
    #[serde(default)]
    pub projects: Vec<Value>,
    #[serde(default)]
    pub labels: Vec<Value>,
}

/// A write, in the shape of a Todoist sync command.
#[derive(Debug, Clone, PartialEq)]
pub enum TodoistCommand {
    Close {
        id: String,
    },
    /// Keeps `string` so that a recurring task stays recurring when it is moved.
    Reschedule {
        id: String,
        date: String,
        string: Option<String>,
    },
    Add {
        content: String,
        project_id: Option<String>,
        section_id: Option<String>,
        labels: Vec<String>,
        priority: Option<u8>,
        due_string: Option<String>,
        description: Option<String>,
    },
}

impl TodoistCommand {
    fn to_json(&self) -> Value {
        let uuid = uuid::Uuid::new_v4().to_string();
        match self {
            Self::Close { id } => {
                json!({ "type": "item_close", "uuid": uuid, "args": { "id": id } })
            }
            Self::Reschedule { id, date, string } => {
                let mut due = json!({ "date": date });
                if let Some(string) = string {
                    due["string"] = json!(string);
                }
                json!({ "type": "item_update", "uuid": uuid, "args": { "id": id, "due": due } })
            }
            Self::Add {
                content,
                project_id,
                section_id,
                labels,
                priority,
                due_string,
                description,
            } => {
                let mut args = json!({ "content": content, "labels": labels });
                if let Some(project_id) = project_id {
                    args["project_id"] = json!(project_id);
                }
                if let Some(section_id) = section_id {
                    args["section_id"] = json!(section_id);
                }
                if let Some(priority) = priority {
                    args["priority"] = json!(priority);
                }
                if let Some(due_string) = due_string {
                    args["due"] = json!({ "string": due_string });
                }
                if let Some(description) = description {
                    args["description"] = json!(description);
                }
                json!({
                    "type": "item_add",
                    "uuid": uuid,
                    "temp_id": uuid::Uuid::new_v4().to_string(),
                    "args": args,
                })
            }
        }
    }
}

/// A token given by hand never changes; a connection made in the application renews itself.
enum Auth {
    Token(String),
    Session,
}

pub struct TodoistClient {
    http: reqwest::Client,
    auth: Auth,
    base: String,
}

impl TodoistClient {
    #[must_use]
    pub fn new(token: String) -> Self {
        Self::with_base(token, DEFAULT_BASE.to_owned())
    }

    #[must_use]
    pub fn with_base(token: String, base: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth: Auth::Token(token),
            base,
        }
    }

    /// Uses the session saved in the keyring.
    #[must_use]
    pub fn with_session(base: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth: Auth::Session,
            base: base.unwrap_or_else(|| DEFAULT_BASE.to_owned()),
        }
    }

    async fn token(&self) -> Result<String, SourceError> {
        match &self.auth {
            Auth::Token(token) => Ok(token.clone()),
            Auth::Session => crate::credentials::fresh_todoist_token(&self.http)
                .await
                .map_err(|error| SourceError::MissingToken(error.to_string())),
        }
    }

    async fn post(&self) -> Result<reqwest::RequestBuilder, SourceError> {
        Ok(self
            .http
            .post(format!("{}/api/v1/sync", self.base))
            .bearer_auth(self.token().await?))
    }
    /// # Errors
    /// Any HTTP failure, or a body that is not a sync response.
    pub async fn sync(&self, sync_token: &str) -> Result<SyncResponse, SourceError> {
        let response = self
            .post()
            .await?
            .form(&[
                ("sync_token", sync_token),
                ("resource_types", RESOURCE_TYPES),
            ])
            .send()
            .await?;
        Ok(check_status(response).await?.json().await?)
    }

    /// Creates a task from natural language, parsed as in Todoist's own apps.
    ///
    /// # Errors
    /// Any HTTP failure.
    pub async fn quick_add(&self, text: &str) -> Result<(), SourceError> {
        let response = self
            .http
            .post(format!("{}/api/v1/tasks/quick", self.base))
            .bearer_auth(self.token().await?)
            .json(&json!({ "text": text }))
            .send()
            .await?;
        check_status(response).await?;
        Ok(())
    }

    /// Sends one command. Todoist answers 200 even when a command fails, so `sync_status` decides.
    ///
    /// # Errors
    /// Any HTTP failure, or the error Todoist reports for the command.
    pub async fn command(&self, command: &TodoistCommand) -> Result<(), SourceError> {
        let body = command.to_json();
        let uuid = body["uuid"].as_str().unwrap_or_default().to_owned();
        let commands = Value::Array(vec![body]).to_string();
        let response = self
            .post()
            .await?
            .form(&[("commands", commands)])
            .send()
            .await?;
        let answer: Value = check_status(response).await?.json().await?;
        match answer.pointer(&format!("/sync_status/{uuid}")) {
            Some(Value::String(status)) if status == "ok" => Ok(()),
            Some(error) => Err(SourceError::Shape(error.to_string())),
            None => Err(SourceError::Shape("no status for the command".to_owned())),
        }
    }
}

/// What a client keeps between syncs. Resources are keyed by id, because an incremental response
/// only carries the ones that changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
    use super::*;

    fn response(full_sync: bool, token: &str, items: Vec<Value>) -> SyncResponse {
        SyncResponse {
            sync_token: token.to_owned(),
            full_sync,
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

    #[test]
    fn moving_a_recurring_task_keeps_its_recurrence() {
        let body = TodoistCommand::Reschedule {
            id: "1".to_owned(),
            date: "2026-11-10".to_owned(),
            string: Some("every monday".to_owned()),
        }
        .to_json();
        assert_eq!(body["type"], "item_update");
        assert_eq!(body["args"]["due"]["date"], "2026-11-10");
        assert_eq!(body["args"]["due"]["string"], "every monday");
    }
}
