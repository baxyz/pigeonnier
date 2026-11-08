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

    #[zbus(signal)]
    fn changed(&self, revision: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn window_requested(&self, mode: String) -> zbus::Result<()>;
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
