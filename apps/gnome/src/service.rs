// SPDX-License-Identifier: AGPL-3.0-or-later

//! The daemon, seen from the window.

use chrono::Local;
use pigeonnier_core::viewmodel::Snapshot;
use pigeonnier_core::when::parse_until;
use zbus::proxy;

#[proxy(
    interface = "io.github.baxyz.Pigeonnier1",
    default_service = "io.github.baxyz.Pigeonnier",
    default_path = "/io/github/baxyz/Pigeonnier"
)]
pub trait Daemon {
    fn get_view(&self, view: &str) -> zbus::Result<String>;
    fn refresh(&self) -> zbus::Result<()>;
    fn complete(&self, item_id: &str) -> zbus::Result<()>;
    fn snooze(&self, item_id: &str, until_unix: i64) -> zbus::Result<()>;
    fn merge(&self, item_id: &str) -> zbus::Result<()>;
    fn add_task(&self, project: &str, text: &str) -> zbus::Result<()>;
    fn setup(&self, todoist_parent: &str) -> zbus::Result<()>;
    fn connect_github(&self) -> zbus::Result<(String, String)>;
    fn connect_todoist(&self) -> zbus::Result<String>;
    fn disconnect(&self, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn changed(&self, revision: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn window_requested(&self, mode: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn connection_failed(&self, service: String, message: String) -> zbus::Result<()>;
}

pub async fn connect() -> zbus::Result<DaemonProxy<'static>> {
    let connection = zbus::Connection::session().await?;
    DaemonProxy::new(&connection).await
}

/// The view, or a message for the person: the window must stay useful to say the daemon is away.
pub async fn load(view: &str) -> Result<Snapshot, String> {
    let daemon = connect().await.map_err(|error| error.to_string())?;
    let text = daemon
        .get_view(view)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

pub async fn refresh() -> zbus::Result<()> {
    connect().await?.refresh().await
}

pub async fn complete(item_id: &str) -> zbus::Result<()> {
    connect().await?.complete(item_id).await
}

pub async fn snooze(item_id: &str, until: &str) -> zbus::Result<()> {
    let now = Local::now().fixed_offset();
    let until = parse_until(until, now)
        .ok_or_else(|| zbus::Error::Failure(format!("cannot read {until}")))?;
    connect().await?.snooze(item_id, until.timestamp()).await
}

pub async fn merge(item_id: &str) -> zbus::Result<()> {
    connect().await?.merge(item_id).await
}

pub async fn add_task(project: &str, text: &str) -> zbus::Result<()> {
    connect().await?.add_task(project, text).await
}

pub async fn setup(parent: &str) -> zbus::Result<()> {
    connect().await?.setup(parent).await
}

pub async fn connect_github() -> zbus::Result<(String, String)> {
    connect().await?.connect_github().await
}

pub async fn connect_todoist() -> zbus::Result<String> {
    connect().await?.connect_todoist().await
}

pub async fn disconnect(service: &str) -> zbus::Result<()> {
    connect().await?.disconnect(service).await
}

/// What a person can read from an error of the daemon.
pub fn message(error: &zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(_, Some(text), _) => text.clone(),
        other => other.to_string(),
    }
}
