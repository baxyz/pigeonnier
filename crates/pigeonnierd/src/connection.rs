// SPDX-License-Identifier: AGPL-3.0-or-later

//! Connecting a service from the application: the flows start here and finish in the background,
//! because they wait for a person.

use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow, bail};
use pigeonnier_core::credentials;
use pigeonnier_core::engine::{github_client, todoist_session_client};
use pigeonnier_core::oauth::{CALLBACK_PORT, github_device_flow, start_todoist_login};

use crate::Shared;
use crate::bus::{OBJECT_PATH, Service};

async fn report(connection: &zbus::Connection, service: &str, message: &str) {
    eprintln!("pigeonnierd: connecting {service} failed: {message}");
    let reference = connection
        .object_server()
        .interface::<_, Service>(OBJECT_PATH)
        .await;
    if let Ok(reference) = reference {
        let _ = Service::connection_failed(reference.signal_emitter(), service, message).await;
    }
}

pub async fn github(shared: Arc<Shared>, connection: zbus::Connection) -> Result<(String, String)> {
    let flow = github_device_flow().context(
        "cette version n'a pas d'application GitHub déclarée : connecte GitHub CLI avec `gh auth login`",
    )?;
    let code = flow.start().await?;
    let answer = (code.user_code.clone(), code.verification_uri.clone());
    tokio::spawn(async move {
        match flow.wait_for_token(&code).await {
            Ok(token) => {
                if let Err(error) = credentials::store(credentials::GITHUB, &token) {
                    eprintln!(
                        "pigeonnierd: GitHub stays connected until the daemon stops: {error}"
                    );
                }
                shared
                    .engine
                    .write()
                    .await
                    .set_github(Some(github_client(token)));
                shared.refresh.notify_one();
            }
            Err(error) => report(&connection, "github", &error.to_string()).await,
        }
    });
    Ok(answer)
}

pub async fn todoist(shared: Arc<Shared>, connection: zbus::Connection) -> Result<String> {
    let metadata = std::env::var("PIGEONNIER_TODOIST_METADATA").ok();
    let login = start_todoist_login(metadata.as_deref(), CALLBACK_PORT).await?;
    let url = login.url.clone();
    tokio::spawn(async move {
        let result = async {
            let session = login.finish().await?;
            credentials::store_todoist_session(&session)?;
            anyhow::Ok(())
        };
        match result.await {
            Ok(()) => {
                shared
                    .engine
                    .write()
                    .await
                    .set_todoist(Some(todoist_session_client()));
                shared.refresh.notify_one();
            }
            Err(error) => report(&connection, "todoist", &error.to_string()).await,
        }
    });
    Ok(url)
}

pub async fn disconnect(shared: &Shared, service: &str) -> Result<()> {
    let mut engine = shared.engine.write().await;
    match service {
        "github" => {
            credentials::forget(credentials::GITHUB)?;
            engine.set_github(None);
            engine.purge_service("github.")?;
        }
        "todoist" => {
            credentials::forget_todoist()?;
            engine.set_todoist(None);
            engine.purge_service("todoist.")?;
        }
        other => bail!(anyhow!("unknown service {other}")),
    }
    Ok(())
}
