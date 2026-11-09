// SPDX-License-Identifier: AGPL-3.0-or-later

//! The whole path, from HTTP answers to what a UI draws, against made-up services.

use chrono::{DateTime, FixedOffset};
use pigeonnier_core::cache::Cache;
use pigeonnier_core::config::Config;
use pigeonnier_core::engine::{Engine, EngineError, NewAction};
use pigeonnier_core::source::github::GithubClient;
use pigeonnier_core::source::todoist::TodoistClient;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const CONFIG: &str = r#"
[defaults]
todoist_parent = "Dev"

[[project]]
id = "acme"
name = "Acme"
color = "blue"
github = { owner = "acme", project = 1 }
todoist = { section = "Acme" }
"#;

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-11-09T10:00:00+01:00").unwrap()
}

fn pr(number: u32, author: &str, kind: &str) -> Value {
    json!({
        "id": format!("PR_{number}"), "number": number, "title": format!("PR {number}"),
        "url": format!("https://github.com/acme/tool/pull/{number}"), "isDraft": false,
        "updatedAt": "2026-11-08T10:00:00Z", "createdAt": "2026-11-08T10:00:00Z",
        "reviewDecision": null, "mergeable": "MERGEABLE", "headRefOid": "abc123",
        "repository": { "name": "tool", "nameWithOwner": "acme/tool",
                        "viewerDefaultMergeMethod": "SQUASH", "owner": { "login": "acme" } },
        "statusCheckRollup": { "state": "SUCCESS" }, "labels": { "nodes": [] }, "milestone": null,
        "author": { "login": author, "__typename": kind }, "timelineItems": { "nodes": [] },
    })
}

fn search(nodes: &[Value]) -> Value {
    json!({ "data": { "search": { "pageInfo": { "hasNextPage": false }, "nodes": nodes } } })
}

async fn github() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(body_string_contains("viewer { login }"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": { "viewer": { "login": "me" } } })),
        )
        .mount(&server)
        .await;
    Mock::given(body_string_contains("review-requested"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search(&[
            pr(1, "someone", "User"),
            pr(2, "dependabot", "Bot"),
        ])))
        .mount(&server)
        .await;
    Mock::given(body_string_contains("author:@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search(&[pr(3, "me", "User")])))
        .mount(&server)
        .await;
    Mock::given(body_string_contains("projectV2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "data": { "repositoryOwner": { "projectV2": {
            "id": "PVT", "title": "Roadmap", "closed": false,
            "items": { "pageInfo": { "hasNextPage": false }, "nodes": [
                { "id": "PVTI_1", "isArchived": false, "updatedAt": "2026-11-08T10:00:00Z",
                  "content": { "__typename": "DraftIssue", "title": "Plan",
                               "assignees": { "nodes": [] } },
                  "fieldValues": { "nodes": [
                      { "name": "P0", "field": { "name": "Priority" } },
                      { "date": "2026-11-01", "field": { "name": "Target date" } },
                  ] } },
            ] },
        } } } }),
        ))
        .mount(&server)
        .await;
    server
}

fn task(id: &str, checked: bool) -> Value {
    json!({ "id": id, "content": format!("Task {id}"), "section_id": "s1", "project_id": "p1",
            "priority": 4, "labels": ["claude"], "checked": checked, "due": null,
            "updated_at": "2026-11-08T10:00:00Z" })
}

/// Todoist answering a full sync with `items`, and a status of `ok` to any command.
async fn todoist(items: &[Value]) -> MockServer {
    let server = MockServer::start().await;
    let full = json!({
        "sync_token": "t1", "full_sync": true, "items": items,
        "sections": [{ "id": "s1", "name": "acme", "project_id": "p1" }],
        "projects": [{ "id": "p1", "name": "Dev" }],
    });
    Mock::given(method("POST"))
        .and(path("/api/v1/sync"))
        .and(body_string_contains("sync_token=*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(full))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/sync"))
        .and(body_string_contains("sync_token=t1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "sync_token": "t1", "full_sync": false })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/sync"))
        .and(body_string_contains("commands="))
        .respond_with(|request: &Request| {
            let body = String::from_utf8_lossy(&request.body);
            let commands: String = url_decode(body.trim_start_matches("commands="));
            let parsed: Value = serde_json::from_str(&commands).unwrap();
            let uuid = parsed[0]["uuid"].as_str().unwrap().to_owned();
            ResponseTemplate::new(200).set_body_json(json!({ "sync_status": { uuid: "ok" } }))
        })
        .mount(&server)
        .await;
    server
}

fn url_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
                decoded.push(u8::from_str_radix(hex, 16).unwrap());
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).unwrap()
}

fn engine(github: Option<&MockServer>, todoist: Option<&MockServer>) -> Engine {
    Engine::new(
        Config::parse(CONFIG, "test").unwrap(),
        Cache::in_memory().unwrap(),
        github.map(|server| GithubClient::with_endpoint("gh".to_owned(), server.uri())),
        todoist.map(|server| TodoistClient::with_base("td".to_owned(), server.uri())),
    )
}

#[tokio::test]
async fn sources_end_up_in_one_list_per_project() {
    let (github, todoist) = (github().await, todoist(&[task("9", false)]).await);
    let engine = engine(Some(&github), Some(&todoist));

    assert_eq!(engine.refresh().await.unwrap(), []);

    let snapshot = engine.snapshot("all", now()).unwrap();
    assert_eq!(snapshot.projects.len(), 1);
    let project = &snapshot.projects[0];
    assert_eq!(project.id, "acme");
    let ids: Vec<_> = project.items.iter().map(|item| item.id.as_str()).collect();
    // The late roadmap item first, then the P1 task, the review, and my PR ready to merge.
    assert_eq!(
        ids,
        [
            "gh-pv2:PVTI_1",
            "td:9",
            "gh-pr:acme/tool#1",
            "gh-pr:acme/tool#3"
        ]
    );
    assert_eq!(project.items[0].due_urgency.as_deref(), Some("overdue"));
    assert_eq!(project.items[3].state_label.as_deref(), Some("à merger"));
    assert_eq!(project.bots.as_ref().unwrap().items.len(), 1);
    assert_eq!(project.action_count, 2);
    assert!(snapshot.badge.alert);
}

#[tokio::test]
async fn a_source_that_fails_keeps_its_last_answer_and_is_reported() {
    let github = github().await;
    let failing = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
        .mount(&failing)
        .await;
    let engine = engine(Some(&github), Some(&failing));

    let issues = engine.refresh().await.unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, "Todoist");
    assert!(issues[0].reconnect);
    let snapshot = engine.snapshot("all", now()).unwrap();
    assert_eq!(snapshot.projects[0].items.len(), 3);
    assert_eq!(snapshot.issues, issues);
}

#[tokio::test]
async fn a_project_the_token_may_not_read_says_so() {
    let server = MockServer::start().await;
    Mock::given(body_string_contains("projectV2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": { "repositoryOwner": { "projectV2": null } } })),
        )
        .mount(&server)
        .await;
    Mock::given(body_string_contains("viewer { login }"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": { "viewer": { "login": "me" } } })),
        )
        .mount(&server)
        .await;
    Mock::given(body_string_contains("search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search(&[])))
        .mount(&server)
        .await;
    let engine = engine(Some(&server), None);

    let issues = engine.refresh().await.unwrap();

    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("ne peut pas lire"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.source == "Todoist" && issue.reconnect)
    );
}

#[tokio::test]
async fn completing_a_task_that_changed_is_refused() {
    let todoist = todoist(&[task("9", false), task("10", true)]).await;
    let engine = engine(None, Some(&todoist));
    engine.refresh().await.unwrap();

    engine.complete("td:9").await.unwrap();

    assert!(matches!(
        engine.complete("td:10").await,
        Err(EngineError::Gone(_))
    ));
    assert!(matches!(
        engine.complete("td:404").await,
        Err(EngineError::Gone(_))
    ));
    assert!(matches!(
        engine.complete("gh-pr:acme/tool#1").await,
        Err(EngineError::NotTodoist(_))
    ));
}

#[tokio::test]
async fn a_pr_snoozes_locally() {
    let github = github().await;
    let engine = engine(Some(&github), None);
    engine.refresh().await.unwrap();

    let until = DateTime::parse_from_rfc3339("2026-11-10T08:00:00+01:00").unwrap();
    engine.snooze("gh-pr:acme/tool#3", until).await.unwrap();
    assert!(matches!(
        engine.snooze("gh-pr:acme/tool#404", until).await,
        Err(EngineError::Gone(_))
    ));

    let listed = |snapshot: pigeonnier_core::viewmodel::Snapshot| {
        snapshot.projects[0]
            .items
            .iter()
            .any(|item| item.id == "gh-pr:acme/tool#3")
    };
    assert!(!listed(engine.snapshot("all", now()).unwrap()));
    let later = DateTime::parse_from_rfc3339("2026-11-10T09:00:00+01:00").unwrap();
    assert!(listed(engine.snapshot("all", later).unwrap()));
}

#[tokio::test]
async fn a_task_moves_for_real_when_snoozed() {
    let todoist = todoist(&[task("9", false)]).await;
    let engine = engine(None, Some(&todoist));
    engine.refresh().await.unwrap();

    let until = DateTime::parse_from_rfc3339("2026-11-10T08:00:00+01:00").unwrap();
    engine.snooze("td:9", until).await.unwrap();

    let requests = todoist.received_requests().await.unwrap();
    let body = String::from_utf8_lossy(&requests.iter().rev().nth(1).unwrap().body).into_owned();
    let commands = url_decode(&body);
    assert!(commands.contains("item_update"));
    assert!(commands.contains("2026-11-10"));
}

#[tokio::test]
async fn a_manual_action_lands_in_the_section_with_the_label() {
    let todoist = todoist(&[]).await;
    let engine = engine(None, Some(&todoist));
    engine.refresh().await.unwrap();

    engine
        .add_manual_action(NewAction {
            project: "acme".to_owned(),
            title: "Rotate the key".to_owned(),
            pr_url: Some("https://x/1".to_owned()),
            ..NewAction::default()
        })
        .await
        .unwrap();

    let requests = todoist.received_requests().await.unwrap();
    let commands = url_decode(&String::from_utf8_lossy(
        &requests.iter().rev().nth(1).unwrap().body,
    ));
    assert!(commands.contains("item_add"));
    assert!(commands.contains(r#""section_id":"s1""#));
    assert!(commands.contains(r#""labels":["claude"]"#));
    assert!(matches!(
        engine
            .add_manual_action(NewAction {
                project: "nope".to_owned(),
                ..NewAction::default()
            })
            .await,
        Err(EngineError::UnknownProject(_))
    ));
}

#[tokio::test]
async fn the_first_notifications_are_recorded_and_new_ones_shown() {
    let (github, todoist) = (github().await, todoist(&[]).await);
    let engine = engine(Some(&github), Some(&todoist));
    engine.refresh().await.unwrap();

    assert_eq!(engine.take_notifications(now()).unwrap(), []);
    assert_eq!(engine.take_notifications(now()).unwrap(), []);
}

#[tokio::test]
async fn a_pr_ready_to_merge_merges_at_the_commit_that_was_checked() {
    let github = github().await;
    Mock::given(body_string_contains("mergePullRequest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "data": { "mergePullRequest": { "pullRequest": { "merged": true } } } }),
        ))
        .mount(&github)
        .await;
    let engine = engine(Some(&github), None);
    engine.refresh().await.unwrap();

    engine.merge("gh-pr:acme/tool#3", now()).await.unwrap();

    let requests = github.received_requests().await.unwrap();
    let merge = requests
        .iter()
        .map(|request| String::from_utf8_lossy(&request.body).into_owned())
        .find(|body| body.contains("mergePullRequest"))
        .unwrap();
    assert!(merge.contains(r#""id":"PR_3""#));
    assert!(merge.contains(r#""head":"abc123""#));
    assert!(merge.contains(r#""method":"SQUASH""#));

    assert!(matches!(
        engine.merge("gh-pr:acme/tool#404", now()).await,
        Err(EngineError::NotMergeable(_))
    ));
}

#[tokio::test]
async fn quick_add_targets_the_section_of_the_project() {
    let todoist = todoist(&[]).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks/quick"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&todoist)
        .await;
    let mut engine = engine(None, Some(&todoist));
    let mut config = Config::parse(CONFIG, "test").unwrap();
    config.projects[0].todoist.as_mut().unwrap().section = "Sunny Tech".to_owned();
    engine.set_config(config);

    engine
        .quick_add(Some("acme"), "relire vendredi p2")
        .await
        .unwrap();
    engine.quick_add(None, "acheter du pain").await.unwrap();

    let requests = todoist.received_requests().await.unwrap();
    let texts: Vec<String> = requests
        .iter()
        .filter(|request| request.url.path() == "/api/v1/tasks/quick")
        .map(|request| {
            serde_json::from_slice::<Value>(&request.body).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        texts,
        [r"relire vendredi p2 #Dev /Sunny\ Tech", "acheter du pain"]
    );
    assert!(matches!(
        engine.quick_add(Some("nope"), "x").await,
        Err(EngineError::UnknownProject(_))
    ));
}

#[tokio::test]
async fn a_configuration_is_proposed_from_owners_and_sections() {
    let (github, todoist) = (MockServer::start().await, todoist(&[]).await);
    Mock::given(body_string_contains("organizations"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "viewer": {
            "login": "me", "projectsV2": { "nodes": [] },
            "organizations": { "nodes": [{ "login": "acme", "projectsV2": { "nodes": [
                { "number": 4, "title": "R", "closed": false }
            ] } }] },
        } } })),
        )
        .mount(&github)
        .await;
    let engine = engine(Some(&github), Some(&todoist));

    let config = engine.propose_config(Some("Dev")).await.unwrap();

    let acme = config
        .projects
        .iter()
        .find(|project| project.id == "acme")
        .unwrap();
    assert_eq!(acme.github.as_ref().unwrap().project, Some(4));
    assert_eq!(acme.todoist.as_ref().unwrap().section, "acme");
    assert!(matches!(
        engine.propose_config(Some("Nope")).await,
        Err(EngineError::UnknownParent(_))
    ));
}
