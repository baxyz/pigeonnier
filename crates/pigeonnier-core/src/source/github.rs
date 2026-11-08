// SPDX-License-Identifier: AGPL-3.0-or-later

//! GitHub GraphQL: pull request search and Projects v2.

use std::fmt::Write as _;
use std::process::Command;

use serde_json::{Map, Value, json};

use super::{Provenance, RawItem, SourceError, check_status};

const ENDPOINT: &str = "https://api.github.com/graphql";
const SOURCE_PR: &str = "github:pull_request";
const SOURCE_PROJECT_ITEM: &str = "github:project_item";

/// The fields the spec lists, plus the `id` provenance needs.
const PR_FIELDS: &str = "
    id number title url isDraft updatedAt createdAt reviewDecision mergeable
    repository { nameWithOwner owner { login } }
    statusCheckRollup { state }
    labels(first: 20) { nodes { name } }
    milestone { dueOn }";

/// What the spec's rules need but its field list leaves out: the author's type for grouping bots,
/// the requested reviewers for telling a team from a person, the request date for the age rule.
const PR_EXTRA_FIELDS: &str = "
    author { login __typename }
    reviewRequests(first: 20) { nodes { requestedReviewer { __typename } } }
    timelineItems(itemTypes: [REVIEW_REQUESTED_EVENT], last: 20) {
      nodes { ... on ReviewRequestedEvent { createdAt } }
    }";

const PROJECT_QUERY: &str = "
query($owner: String!, $number: Int!, $after: String) {
  rateLimit { cost remaining }
  repositoryOwner(login: $owner) {
    ... on ProjectV2Owner {
      projectV2(number: $number) {
        id title closed
        fields(first: 50) {
          nodes {
            __typename
            ... on ProjectV2FieldCommon { name dataType }
            ... on ProjectV2SingleSelectField { options { name color } }
            ... on ProjectV2IterationField {
              configuration { iterations { title startDate duration } }
            }
          }
        }
        items(first: 100, after: $after) {
          totalCount
          pageInfo { hasNextPage endCursor }
          nodes {
            id isArchived updatedAt type
            content {
              __typename
              ... on Issue { title url state assignees(first: 10) { nodes { login } } }
              ... on PullRequest { title url state assignees(first: 10) { nodes { login } } }
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

/// Which fields a pull request search asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrFields {
    Spec,
    Extended,
}

impl PrFields {
    fn selection(self) -> String {
        match self {
            Self::Spec => PR_FIELDS.to_owned(),
            Self::Extended => format!("{PR_FIELDS}{PR_EXTRA_FIELDS}"),
        }
    }
}

/// The price of what was fetched, summed over its pages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cost {
    pub requests: u32,
    pub points: u64,
    pub remaining: u64,
}

impl Cost {
    fn add(&mut self, data: &Value) {
        self.requests += 1;
        self.points += data
            .pointer("/rateLimit/cost")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        self.remaining = data
            .pointer("/rateLimit/remaining")
            .and_then(Value::as_u64)
            .unwrap_or(0);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrSearch {
    pub issue_count: u64,
    pub items: Vec<RawItem>,
    pub cost: Cost,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSnapshot {
    pub title: String,
    pub closed: bool,
    pub total_count: u64,
    pub fields: Vec<Value>,
    pub items: Vec<RawItem>,
}

/// GitHub answers `null` with no error for a project the token may not read, and `NOT_FOUND` for a
/// number that does not exist. The difference is the only way to tell a permission problem.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectRead {
    Found(Box<ProjectSnapshot>),
    Inaccessible,
    NotFound,
}

pub struct GithubClient {
    http: reqwest::Client,
    token: String,
}

impl GithubClient {
    #[must_use]
    pub fn new(token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token,
        }
    }

    /// Builds a client with the token of `gh auth token`.
    ///
    /// # Errors
    /// [`SourceError::MissingToken`] when `gh` is missing, fails, or prints nothing.
    pub fn from_gh_cli() -> Result<Self, SourceError> {
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
        Ok(Self::new(token))
    }

    /// Runs one query and returns `data` with the errors GitHub reported next to it.
    async fn request(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<(Value, Vec<Value>), SourceError> {
        let response = self
            .http
            .post(ENDPOINT)
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

    /// Runs one search and follows its pages.
    ///
    /// # Errors
    /// Any HTTP or GraphQL failure, including a partial answer: a search with holes would hide work.
    pub async fn search_pull_requests(
        &self,
        search: &str,
        fields: PrFields,
    ) -> Result<PrSearch, SourceError> {
        let selection = fields.selection();
        let query = format!(
            "query($q: String!, $after: String) {{
               rateLimit {{ cost remaining }}
               search(type: ISSUE, first: 100, query: $q, after: $after) {{
                 issueCount
                 pageInfo {{ hasNextPage endCursor }}
                 nodes {{ ... on PullRequest {{ {selection} }} }}
               }}
             }}"
        );
        let mut result = PrSearch {
            issue_count: 0,
            items: Vec::new(),
            cost: Cost::default(),
        };
        let mut after = Value::Null;
        loop {
            let variables = json!({ "q": search, "after": after });
            let (data, errors) = self.request(&query, variables).await?;
            if !errors.is_empty() {
                return Err(SourceError::GraphQl(error_messages(&errors)));
            }
            result.cost.add(&data);
            let page = &data["search"];
            result.issue_count = page["issueCount"].as_u64().unwrap_or(0);
            let nodes = page["nodes"].as_array().map_or(&[][..], Vec::as_slice);
            result.items.extend(nodes.iter().map(raw_pull_request));
            match next_cursor(page) {
                Some(cursor) => after = cursor,
                None => return Ok(result),
            }
        }
    }

    /// Runs several searches in one request, to compare with one request per search.
    ///
    /// # Errors
    /// Any HTTP or GraphQL failure.
    pub async fn count_in_one_request(
        &self,
        searches: &[&str],
        fields: PrFields,
    ) -> Result<(Vec<u64>, Cost), SourceError> {
        let selection = fields.selection();
        let mut aliases = String::new();
        for (index, search) in searches.iter().enumerate() {
            write!(
                aliases,
                "s{index}: search(type: ISSUE, first: 100, query: {search:?}) {{
                   issueCount
                   nodes {{ ... on PullRequest {{ {selection} }} }}
                 }}\n"
            )
            .expect("writing to a String cannot fail");
        }
        let query = format!("query {{ rateLimit {{ cost remaining }} {aliases} }}");
        let (data, errors) = self.request(&query, json!({})).await?;
        if !errors.is_empty() {
            return Err(SourceError::GraphQl(error_messages(&errors)));
        }
        let mut cost = Cost::default();
        cost.add(&data);
        let counts = (0..searches.len())
            .map(|index| {
                data[format!("s{index}")]["issueCount"]
                    .as_u64()
                    .unwrap_or(0)
            })
            .collect();
        Ok((counts, cost))
    }

    /// Reads a project and all its items. `repositoryOwner` serves both organizations and users.
    ///
    /// # Errors
    /// Any HTTP failure, or a GraphQL error other than the `NOT_FOUND` of [`ProjectRead`].
    pub async fn read_project(
        &self,
        owner: &str,
        number: u32,
    ) -> Result<(ProjectRead, Cost), SourceError> {
        let mut cost = Cost::default();
        let mut after = Value::Null;
        let mut snapshot: Option<ProjectSnapshot> = None;
        loop {
            let variables = json!({ "owner": owner, "number": number, "after": after });
            let (data, errors) = self.request(PROJECT_QUERY, variables).await?;
            cost.add(&data);
            if errors.iter().any(|error| error["type"] == "NOT_FOUND") {
                return Ok((ProjectRead::NotFound, cost));
            }
            if !errors.is_empty() {
                return Err(SourceError::GraphQl(error_messages(&errors)));
            }
            let project = &data["repositoryOwner"]["projectV2"];
            if project.is_null() {
                return Ok((ProjectRead::Inaccessible, cost));
            }
            let page = &project["items"];
            let result = snapshot.get_or_insert_with(|| ProjectSnapshot {
                title: project["title"].as_str().unwrap_or_default().to_owned(),
                closed: project["closed"].as_bool().unwrap_or(false),
                total_count: page["totalCount"].as_u64().unwrap_or(0),
                fields: project
                    .pointer("/fields/nodes")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                items: Vec::new(),
            });
            let nodes = page["nodes"].as_array().map_or(&[][..], Vec::as_slice);
            result.items.extend(nodes.iter().map(raw_project_item));
            let Some(cursor) = next_cursor(page) else {
                let found = snapshot.ok_or_else(|| SourceError::Shape("no page".to_owned()))?;
                return Ok((ProjectRead::Found(Box::new(found)), cost));
            };
            after = cursor;
        }
    }
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

fn raw_pull_request(node: &Value) -> RawItem {
    RawItem {
        provenance: provenance(SOURCE_PR, node),
        payload: node.clone(),
    }
}

fn raw_project_item(node: &Value) -> RawItem {
    let mut payload = node.clone();
    payload["fields"] = flatten_field_values(node);
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
    for node in nodes.map_or(&[][..], Vec::as_slice) {
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
    fn the_last_page_has_no_cursor() {
        let last = json!({ "pageInfo": { "hasNextPage": false, "endCursor": "abc" } });
        let more = json!({ "pageInfo": { "hasNextPage": true, "endCursor": "abc" } });
        assert_eq!(next_cursor(&last), None);
        assert_eq!(next_cursor(&more), Some(json!("abc")));
    }
}
