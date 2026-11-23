// SPDX-License-Identifier: AGPL-3.0-or-later

//! GitHub timing out: asking again, then for smaller pages, and saying so in a few words.

use pigeonnier_core::source::github::GithubClient;
use serde_json::json;
use wiremock::matchers::body_string_contains;
use wiremock::{Mock, MockServer, ResponseTemplate};

const TIMEOUT_PAGE: &str = "<!DOCTYPE html>\n<!--\nHello future GitHubber!\n--><html></html>";

fn client(server: &MockServer) -> GithubClient {
    GithubClient::with_endpoint("gh".to_owned(), server.uri()).without_retry_delay()
}

fn page(numbers: &[u32]) -> serde_json::Value {
    let nodes: Vec<_> = numbers
        .iter()
        .map(|number| json!({ "number": number }))
        .collect();
    json!({ "data": { "search": { "pageInfo": { "hasNextPage": false }, "nodes": nodes } } })
}

#[tokio::test]
async fn a_timeout_is_asked_again_once() {
    let server = MockServer::start().await;
    Mock::given(body_string_contains("search"))
        .respond_with(ResponseTemplate::new(504).set_body_string(TIMEOUT_PAGE))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(body_string_contains("search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(&[1])))
        .mount(&server)
        .await;

    let found = client(&server).search_pull_requests("is:pr").await.unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn pages_get_smaller_while_github_times_out() {
    let server = MockServer::start().await;
    for first in ["\"first\":100", "\"first\":50"] {
        Mock::given(body_string_contains(first))
            .respond_with(ResponseTemplate::new(504).set_body_string(TIMEOUT_PAGE))
            .mount(&server)
            .await;
    }
    Mock::given(body_string_contains("\"first\":25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(&[1, 2])))
        .mount(&server)
        .await;

    let found = client(&server).search_issues("is:issue").await.unwrap();

    assert_eq!(found.len(), 2);
}

#[tokio::test]
async fn a_lasting_timeout_says_so_without_the_page() {
    let server = MockServer::start().await;
    Mock::given(body_string_contains("search"))
        .respond_with(ResponseTemplate::new(504).set_body_string(TIMEOUT_PAGE))
        .mount(&server)
        .await;

    let error = client(&server)
        .search_pull_requests("is:pr")
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "le service ne répond pas (HTTP 504) ; nouvel essai au prochain rafraîchissement"
    );
    // 100, 50 and 25 items, each asked twice.
    assert_eq!(server.received_requests().await.unwrap().len(), 6);
}
