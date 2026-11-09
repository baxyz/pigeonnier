// SPDX-License-Identifier: AGPL-3.0-or-later

//! The connection flows, against made-up services.

use chrono::Utc;
use pigeonnier_core::oauth::{GithubDeviceFlow, OauthError, TodoistSession, start_todoist_login};
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn device_code(expires_in: u64) -> pigeonnier_core::oauth::DeviceCode {
    pigeonnier_core::oauth::DeviceCode {
        device_code: "dev".to_owned(),
        user_code: "ABCD-1234".to_owned(),
        verification_uri: "https://github.com/login/device".to_owned(),
        expires_in,
        interval: 1,
    }
}

#[tokio::test]
async fn github_gives_a_code_then_a_token_once_authorized() {
    let server = MockServer::start().await;
    Mock::given(path("/login/device/code"))
        .and(body_string_contains("client_id=abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "dev", "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900, "interval": 1,
        })))
        .mount(&server)
        .await;
    Mock::given(path("/login/oauth/access_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "error": "authorization_pending" })),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "gho_x" })))
        .mount(&server)
        .await;
    let flow = GithubDeviceFlow::with_base("abc".to_owned(), server.uri());

    let code = flow.start().await.unwrap();
    assert_eq!(code.user_code, "ABCD-1234");
    assert_eq!(flow.wait_for_token(&code).await.unwrap(), "gho_x");
}

#[tokio::test]
async fn github_refusal_and_expiry_end_the_wait() {
    let server = MockServer::start().await;
    Mock::given(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "error": "access_denied" })))
        .mount(&server)
        .await;
    let flow = GithubDeviceFlow::with_base("abc".to_owned(), server.uri());
    assert!(matches!(
        flow.wait_for_token(&device_code(900)).await,
        Err(OauthError::Denied)
    ));
    assert!(matches!(
        flow.wait_for_token(&device_code(0)).await,
        Err(OauthError::Expired)
    ));
}

async fn todoist() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/meta"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorization_endpoint": format!("{}/oauth/authorize", server.uri()),
            "token_endpoint": format!("{}/oauth/access_token", server.uri()),
            "registration_endpoint": format!("{}/oauth/register", server.uri()),
        })))
        .mount(&server)
        .await;
    Mock::given(path("/oauth/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "client_id": "tdd_1" })))
        .mount(&server)
        .await;
    Mock::given(path("/oauth/access_token"))
        .and(body_string_contains("code=the-code"))
        .and(body_string_contains("code_verifier="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at", "refresh_token": "rt", "expires_in": 3600,
        })))
        .mount(&server)
        .await;
    server
}

fn query_value(url: &str, name: &str) -> String {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == name)
        .unwrap()
        .1
        .into_owned()
}

#[tokio::test]
async fn todoist_registers_a_public_client_and_exchanges_the_code() {
    let server = todoist().await;
    let login = start_todoist_login(Some(&format!("{}/meta", server.uri())), 0)
        .await
        .unwrap();

    assert_eq!(query_value(&login.url, "code_challenge_method"), "S256");
    assert_eq!(query_value(&login.url, "client_id"), "tdd_1");
    let redirect = query_value(&login.url, "redirect_uri");
    assert!(redirect.starts_with("http://localhost:"));
    let state = query_value(&login.url, "state");

    let registered = server.received_requests().await.unwrap();
    let body: Value = registered
        .iter()
        .find(|request| request.url.path() == "/oauth/register")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .unwrap();
    assert_eq!(body["token_endpoint_auth_method"], "none");
    assert_eq!(body["redirect_uris"][0], redirect);

    let finish = tokio::spawn(login.finish());
    let port = reqwest::Url::parse(&redirect).unwrap().port().unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let other = reqwest::get(format!("{base}/favicon.ico")).await.unwrap();
    assert_eq!(other.status(), 404);
    let page = reqwest::get(format!("{base}/callback?code=the-code&state={state}"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("connecté"));

    let session = finish.await.unwrap().unwrap();
    assert_eq!(session.access_token, "at");
    assert_eq!(session.refresh_token.as_deref(), Some("rt"));
    assert!(session.expires_at.unwrap() > Utc::now());
}

#[tokio::test]
async fn a_callback_with_the_wrong_state_is_refused() {
    let server = todoist().await;
    let login = start_todoist_login(Some(&format!("{}/meta", server.uri())), 0)
        .await
        .unwrap();
    let redirect = query_value(&login.url, "redirect_uri");
    let port = reqwest::Url::parse(&redirect).unwrap().port().unwrap();

    let finish = tokio::spawn(login.finish());
    reqwest::get(format!(
        "http://127.0.0.1:{port}/callback?code=stolen&state=forged"
    ))
    .await
    .unwrap();

    assert!(matches!(
        finish.await.unwrap(),
        Err(OauthError::StateMismatch)
    ));
}

#[tokio::test]
async fn a_refresh_rotates_the_tokens_and_keeps_the_old_one_if_none_comes() {
    let server = MockServer::start().await;
    Mock::given(path("/token"))
        .and(body_string_contains("refresh_token=old"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 3600,
        })))
        .mount(&server)
        .await;
    Mock::given(path("/token"))
        .and(body_string_contains("refresh_token=keep"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "access_token": "a2" })))
        .mount(&server)
        .await;
    let session = |refresh: &str| TodoistSession {
        client_id: "c".to_owned(),
        token_endpoint: format!("{}/token", server.uri()),
        access_token: "a".to_owned(),
        refresh_token: Some(refresh.to_owned()),
        expires_at: Some(Utc::now()),
    };
    let http = reqwest::Client::new();

    let rotated = session("old").refresh(&http).await.unwrap();
    assert_eq!(rotated.access_token, "new-access");
    assert_eq!(rotated.refresh_token.as_deref(), Some("new-refresh"));

    let kept = session("keep").refresh(&http).await.unwrap();
    assert_eq!(kept.refresh_token.as_deref(), Some("keep"));
}
