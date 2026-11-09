// SPDX-License-Identifier: AGPL-3.0-or-later

//! GitHub GraphQL: pull request search, Projects v2 and the discovery of owners.

use std::process::Command;

use serde_json::{Map, Value, json};

use super::{Provenance, RawItem, SourceError, check_status};

const ENDPOINT: &str = "https://api.github.com/graphql";
const SOURCE_PR: &str = "github:pull_request";
const SOURCE_PROJECT_ITEM: &str = "github:project_item";

const PR_FIELDS: &str = "
    id number title url isDraft updatedAt createdAt reviewDecision mergeable headRefOid
    repository { name nameWithOwner viewerDefaultMergeMethod owner { login } }
    statusCheckRollup { state }
    labels(first: 20) { nodes { name } }
    milestone { dueOn }
    author { login __typename }
    timelineItems(itemTypes: [REVIEW_REQUESTED_EVENT], last: 20) {
      nodes { ... on ReviewRequestedEvent { createdAt } }
    }";

const PROJECT_QUERY: &str = "
query($owner: String!, $number: Int!, $after: String) {
  repositoryOwner(login: $owner) {
    ... on ProjectV2Owner {
      projectV2(number: $number) {
        id title closed
        items(first: 100, after: $after) {
          pageInfo { hasNextPage endCursor }
          nodes {
            id isArchived updatedAt type
            content {
              __typename
              ... on Issue {
                title url state number repository { name }
                assignees(first: 10) { nodes { login } }
              }
              ... on PullRequest {
                title url state number repository { name }
                assignees(first: 10) { nodes { login } }
              }
              ... on DraftIssue { title assignees(first: 10) { nodes { login } } }
            }
            fieldValues(first: 20) {
              nodes {
                __typename
                ... on ProjectV2ItemFieldSingleSelectValue {
                  name field { ... on ProjectV2FieldCommon { name } }
                }
                ... on ProjectV2ItemFieldDateValue {
                  date field { ... on ProjectV2FieldCommon { name } }
                }
                ... on ProjectV2ItemFieldIterationValue {
                  title startDate duration field { ... on ProjectV2FieldCommon { name } }
                }
              }
            }
          }
        }
      }
    }
  }
}";

/// GitHub answers `null` with no error for a project the token may not read, and `NOT_FOUND` for a
/// number that does not exist. The difference is the only way to tell a permission problem.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectRead {
    Found(Vec<Value>),
    Inaccessible,
    NotFound,
}

/// An owner the signed-in user can see, with the project to use for it if there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub login: String,
    pub projects: Vec<OwnerProject>,
}

/// A project of an owner; projects come most recently updated first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerProject {
    pub number: u32,
    pub title: String,
}

pub struct GithubClient {
    http: reqwest::Client,
    token: String,
    endpoint: String,
}

impl GithubClient {
    #[must_use]
    pub fn new(token: String) -> Self {
        Self::with_endpoint(token, ENDPOINT.to_owned())
    }

    #[must_use]
    pub fn with_endpoint(token: String, endpoint: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token,
            endpoint,
        }
    }

    /// The token of `gh auth token`.
    ///
    /// # Errors
    /// [`SourceError::MissingToken`] when `gh` is missing, fails, or prints nothing.
    pub fn token_from_gh_cli() -> Result<String, SourceError> {
        let output = Command::new("gh")
            .args(["auth", "token"])
            .output()
            .map_err(|error| SourceError::MissingToken(format!("cannot run gh: {error}")))?;
        let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || token.is_empty() {
            return Err(SourceError::MissingToken(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(token)
    }

    /// Runs one query and returns `data` with the errors GitHub reported next to it.
    async fn request(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<(Value, Vec<Value>), SourceError> {
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.token)
            .header(reqwest::header::USER_AGENT, "pigeonnier")
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await?;
        let mut body: Value = check_status(response).await?.json().await?;
        let errors = match body.get_mut("errors").map(Value::take) {
            Some(Value::Array(errors)) => errors,
            _ => Vec::new(),
        };
        match body.get_mut("data").map(Value::take) {
            Some(data) if !data.is_null() => Ok((data, errors)),
            _ => Err(SourceError::GraphQl(error_messages(&errors))),
        }
    }

    /// The login behind the token, which is what `@me` means in the configuration.
    ///
    /// # Errors
    /// Any HTTP or GraphQL failure.
    pub async fn viewer_login(&self) -> Result<String, SourceError> {
        let (data, _) = self
            .request("query { viewer { login } }", json!({}))
            .await?;
        data.pointer("/viewer/login")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| SourceError::Shape("no viewer login".to_owned()))
    }

    /// Runs one search and follows its pages.
    ///
    /// # Errors
    /// Any HTTP or GraphQL failure, including a partial answer: a search with holes would hide work.
    pub async fn search_pull_requests(&self, search: &str) -> Result<Vec<Value>, SourceError> {
        let query = format!(
            "query($q: String!, $after: String) {{
               search(type: ISSUE, first: 100, query: $q, after: $after) {{
                 pageInfo {{ hasNextPage endCursor }}
                 nodes {{ ... on PullRequest {{ {PR_FIELDS} }} }}
               }}
             }}"
        );
        let mut items = Vec::new();
        let mut after = Value::Null;
        loop {
            let variables = json!({ "q": search, "after": after });
            let (data, errors) = self.request(&query, variables).await?;
            if !errors.is_empty() {
                return Err(SourceError::GraphQl(error_messages(&errors)));
            }
            let page = &data["search"];
            items.extend(page["nodes"].as_array().into_iter().flatten().cloned());
            let Some(cursor) = next_cursor(page) else {
                return Ok(items);
            };
            after = cursor;
        }
    }

    /// Reads all the items of a project. `repositoryOwner` serves both organizations and users.
    ///
    /// # Errors
    /// Any HTTP failure, or a GraphQL error other than the `NOT_FOUND` of [`ProjectRead`].
    pub async fn read_project(&self, owner: &str, number: u32) -> Result<ProjectRead, SourceError> {
        let mut items = Vec::new();
        let mut after = Value::Null;
        loop {
            let variables = json!({ "owner": owner, "number": number, "after": after });
            let (data, errors) = self.request(PROJECT_QUERY, variables).await?;
            if errors.iter().any(|error| error["type"] == "NOT_FOUND") {
                return Ok(ProjectRead::NotFound);
            }
            if !errors.is_empty() {
                return Err(SourceError::GraphQl(error_messages(&errors)));
            }
            let project = &data["repositoryOwner"]["projectV2"];
            if project.is_null() {
                return Ok(ProjectRead::Inaccessible);
            }
            let page = &project["items"];
            items.extend(page["nodes"].as_array().into_iter().flatten().cloned());
            let Some(cursor) = next_cursor(page) else {
                return Ok(ProjectRead::Found(items));
            };
            after = cursor;
        }
    }

    /// Merges a pull request with the repository's default method. `head` makes GitHub refuse if
    /// commits arrived since the list was shown.
    ///
    /// # Errors
    /// Any HTTP failure, or what GitHub says when it refuses.
    pub async fn merge_pull_request(
        &self,
        node_id: &str,
        method: Option<&str>,
        head: Option<&str>,
    ) -> Result<(), SourceError> {
        let query = "mutation($id: ID!, $method: PullRequestMergeMethod, $head: GitObjectID) {
            mergePullRequest(input: { pullRequestId: $id, mergeMethod: $method, expectedHeadOid: $head }) {
                pullRequest { merged }
            }
        }";
        let variables = json!({ "id": node_id, "method": method, "head": head });
        let (_, errors) = self.request(query, variables).await?;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SourceError::GraphQl(error_messages(&errors)))
        }
    }

    /// The signed-in user and the organizations they belong to, with their projects.
    ///
    /// # Errors
    /// Any HTTP or GraphQL failure. A token that may not list organizations gets an empty list from
    /// GitHub rather than an error, so an empty result is not a proof that there are none.
    pub async fn discover_owners(&self) -> Result<Vec<Owner>, SourceError> {
        const PROJECTS: &str =
            "projectsV2(first: 20, orderBy: {field: UPDATED_AT, direction: DESC}) {
            nodes { number title closed }
        }";
        let query = format!(
            "query {{
               viewer {{
                 login {PROJECTS}
                 organizations(first: 100) {{ nodes {{ login {PROJECTS} }} }}
               }}
             }}"
        );
        let (data, errors) = self.request(&query, json!({})).await?;
        if data["viewer"].is_null() {
            return Err(SourceError::GraphQl(error_messages(&errors)));
        }
        let viewer = &data["viewer"];
        let organizations = viewer
            .pointer("/organizations/nodes")
            .and_then(Value::as_array);
        Ok(std::iter::once(viewer)
            .chain(organizations.into_iter().flatten())
            .filter_map(owner_from_node)
            .collect())
    }
}

fn owner_from_node(node: &Value) -> Option<Owner> {
    let projects = node.pointer("/projectsV2/nodes").and_then(Value::as_array);
    Some(Owner {
        login: node["login"].as_str()?.to_owned(),
        projects: projects
            .into_iter()
            .flatten()
            .filter(|project| project["closed"] != Value::Bool(true))
            .filter_map(|project| {
                Some(OwnerProject {
                    number: u32::try_from(project["number"].as_u64()?).ok()?,
                    title: project["title"].as_str()?.to_owned(),
                })
            })
            .collect(),
    })
}

fn next_cursor(page: &Value) -> Option<Value> {
    (page.pointer("/pageInfo/hasNextPage") == Some(&Value::Bool(true)))
        .then(|| page.pointer("/pageInfo/endCursor").cloned())
        .flatten()
}

fn error_messages(errors: &[Value]) -> String {
    let messages: Vec<&str> = errors
        .iter()
        .filter_map(|error| error["message"].as_str())
        .collect();
    messages.join("; ")
}

fn provenance(source: &'static str, node: &Value) -> Provenance {
    Provenance {
        source,
        id: node["id"].as_str().unwrap_or_default().to_owned(),
        version: node["updatedAt"].as_str().map(str::to_owned),
    }
}

#[must_use]
pub fn raw_pull_request(node: &Value) -> RawItem {
    RawItem {
        provenance: provenance(SOURCE_PR, node),
        payload: node.clone(),
    }
}

/// A draft issue has no `state` because it can only be open; saying so here keeps the rules simple.
#[must_use]
pub fn raw_project_item(node: &Value) -> RawItem {
    let mut payload = node.clone();
    payload["fields"] = flatten_field_values(node);
    if node.pointer("/content/__typename").and_then(Value::as_str) == Some("DraftIssue") {
        payload["content"]["state"] = json!("OPEN");
    }
    RawItem {
        provenance: provenance(SOURCE_PROJECT_ITEM, node),
        payload,
    }
}

/// Field values come as a list of unions; keyed by field name they can be mapped like any other
/// field. Values of the field types we do not ask for come back empty and are dropped.
#[must_use]
pub fn flatten_field_values(item: &Value) -> Value {
    let mut fields = Map::new();
    let nodes = item.pointer("/fieldValues/nodes").and_then(Value::as_array);
    for node in nodes.into_iter().flatten() {
        if let Some(name) = node.pointer("/field/name").and_then(Value::as_str) {
            fields.insert(name.to_owned(), node.clone());
        }
    }
    Value::Object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_values_are_keyed_by_field_name() {
        let item = json!({ "fieldValues": { "nodes": [
            { "__typename": "ProjectV2ItemFieldSingleSelectValue", "name": "Todo",
              "field": { "name": "Status" } },
            { "__typename": "ProjectV2ItemFieldDateValue", "date": "2026-12-01",
              "field": { "name": "Target date" } },
            { "__typename": "ProjectV2ItemFieldTextValue" },
        ] } });
        let fields = flatten_field_values(&item);
        assert_eq!(fields["Status"]["name"], "Todo");
        assert_eq!(fields["Target date"]["date"], "2026-12-01");
        assert_eq!(fields.as_object().map(Map::len), Some(2));
    }

    #[test]
    fn a_draft_issue_is_open() {
        let raw =
            raw_project_item(&json!({ "id": "x", "content": { "__typename": "DraftIssue" } }));
        assert_eq!(raw.payload["content"]["state"], "OPEN");
    }

    #[test]
    fn the_last_page_has_no_cursor() {
        let last = json!({ "pageInfo": { "hasNextPage": false, "endCursor": "abc" } });
        let more = json!({ "pageInfo": { "hasNextPage": true, "endCursor": "abc" } });
        assert_eq!(next_cursor(&last), None);
        assert_eq!(next_cursor(&more), Some(json!("abc")));
    }

    #[test]
    fn closed_projects_are_not_proposed() {
        let owner = owner_from_node(&json!({ "login": "acme", "projectsV2": { "nodes": [
            { "number": 2, "title": "Old", "closed": true },
            { "number": 3, "title": "Roadmap", "closed": false },
        ] } }));
        let owner = owner.unwrap();
        assert_eq!(owner.projects.len(), 1);
        assert_eq!(owner.projects[0].number, 3);
    }
}
