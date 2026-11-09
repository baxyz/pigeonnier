// SPDX-License-Identifier: AGPL-3.0-or-later

//! `io.github.baxyz.Pigeonnier1` on the session bus.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use chrono::DateTime;
use pigeonnier_core::engine::EngineError;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

use crate::{Shared, connection, now};

pub const BUS_NAME: &str = "io.github.baxyz.Pigeonnier";
pub const OBJECT_PATH: &str = "/io/github/baxyz/Pigeonnier";

pub struct Service {
    shared: Arc<Shared>,
}

fn failed(error: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(error.to_string())
}

impl Service {
    /// zbus runs its methods on a runtime of its own, while the HTTP client needs tokio's.
    async fn on_tokio(
        &self,
        action: impl Future<Output = Result<(), EngineError>> + Send + 'static,
    ) -> fdo::Result<()> {
        self.shared
            .runtime
            .spawn(action)
            .await
            .map_err(failed)?
            .map_err(failed)?;
        self.shared.refresh.notify_one();
        Ok(())
    }
}

#[interface(name = "io.github.baxyz.Pigeonnier1")]
impl Service {
    /// JSON: the projects and their sorted items, for the default view.
    async fn get_snapshot(&self) -> fdo::Result<String> {
        self.get_view("all").await
    }

    /// JSON, for one of the views the snapshot lists.
    async fn get_view(&self, view: &str) -> fdo::Result<String> {
        let engine = self.shared.engine.read().await;
        let snapshot = engine.snapshot(view, now()).map_err(failed)?;
        serde_json::to_string(&snapshot).map_err(failed)
    }

    fn refresh(&self) {
        self.shared.refresh.notify_one();
    }

    async fn complete(&self, item_id: &str) -> fdo::Result<()> {
        let shared = Arc::clone(&self.shared);
        let item_id = item_id.to_owned();
        self.on_tokio(async move { shared.engine.read().await.complete(&item_id).await })
            .await
    }

    async fn snooze(&self, item_id: &str, until_unix: i64) -> fdo::Result<()> {
        let until = DateTime::from_timestamp(until_unix, 0)
            .ok_or_else(|| failed("invalid time"))?
            .with_timezone(now().offset());
        let shared = Arc::clone(&self.shared);
        let item_id = item_id.to_owned();
        self.on_tokio(async move { shared.engine.read().await.snooze(&item_id, until).await })
            .await
    }

    /// Merges a PR of mine that is ready.
    async fn merge(&self, item_id: &str) -> fdo::Result<()> {
        let shared = Arc::clone(&self.shared);
        let item_id = item_id.to_owned();
        self.on_tokio(async move { shared.engine.read().await.merge(&item_id, now()).await })
            .await
    }

    /// Adds a task in natural language. An empty project means the Todoist inbox.
    async fn add_task(&self, project: &str, text: &str) -> fdo::Result<()> {
        let shared = Arc::clone(&self.shared);
        let project = Some(project.to_owned()).filter(|project| !project.is_empty());
        let text = text.to_owned();
        self.on_tokio(async move {
            let engine = shared.engine.read().await;
            engine.quick_add(project.as_deref(), &text).await
        })
        .await
    }

    /// First run: writes the proposed configuration.
    async fn setup(&self, todoist_parent: &str) -> fdo::Result<()> {
        let shared = Arc::clone(&self.shared);
        let parent = Some(todoist_parent.trim().to_owned()).filter(|parent| !parent.is_empty());
        self.on_tokio(async move { crate::first_run(&shared, parent).await })
            .await
    }

    /// Starts connecting GitHub: returns the code to enter and the page to enter it on, and goes on
    /// waiting for the person in the background.
    async fn connect_github(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<(String, String)> {
        let task = connection::github(Arc::clone(&self.shared), connection.clone());
        self.shared
            .runtime
            .spawn(task)
            .await
            .map_err(failed)?
            .map_err(failed)
    }

    /// Starts connecting Todoist: returns the page to open, and waits for the browser to come back.
    async fn connect_todoist(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        let task = connection::todoist(Arc::clone(&self.shared), connection.clone());
        self.shared
            .runtime
            .spawn(task)
            .await
            .map_err(failed)?
            .map_err(failed)
    }

    /// Removes the tokens of `github` or `todoist` and forgets its cache.
    async fn disconnect(&self, service: &str) -> fdo::Result<()> {
        connection::disconnect(&self.shared, service)
            .await
            .map_err(failed)?;
        self.shared.refresh.notify_one();
        Ok(())
    }

    /// A connection started earlier ended badly.
    #[zbus(signal)]
    pub async fn connection_failed(
        emitter: &SignalEmitter<'_>,
        service: &str,
        message: &str,
    ) -> zbus::Result<()>;
    /// Asks for the window, `sidebar` or `full`. The Shell extension places it.
    async fn show_window(
        &self,
        mode: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        spawn_app(mode);
        Self::window_requested(&emitter, mode).await.map_err(failed)
    }

    /// Project → (action required, overdue).
    #[zbus(property)]
    async fn counts(&self) -> fdo::Result<HashMap<String, (u32, u32)>> {
        let engine = self.shared.engine.read().await;
        let snapshot = engine.snapshot("all", now()).map_err(failed)?;
        Ok(snapshot
            .projects
            .into_iter()
            .map(|project| (project.id, (project.action_count, project.overdue_count)))
            .collect())
    }

    #[zbus(property)]
    fn revision(&self) -> u32 {
        self.shared.revision.load(Ordering::SeqCst)
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>, revision: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn window_requested(emitter: &SignalEmitter<'_>, mode: &str) -> zbus::Result<()>;
}

/// `pigeonnier-gnome` is a single-instance application: a second start presents the first window.
pub fn spawn_app(mode: &str) {
    let program = std::env::var("PIGEONNIER_APP").unwrap_or_else(|_| "pigeonnier-gnome".to_owned());
    if let Err(error) = std::process::Command::new(&program)
        .arg(format!("--{mode}"))
        .spawn()
    {
        eprintln!("pigeonnierd: cannot start {program}: {error}");
    }
}

pub async fn serve(shared: Arc<Shared>) -> zbus::Result<zbus::Connection> {
    zbus::connection::Builder::session()
        .name(BUS_NAME)
        .serve_at(OBJECT_PATH, Service { shared })
        .build()
        .await
}

pub async fn announce_change(connection: &zbus::Connection, revision: u32) -> zbus::Result<()> {
    let reference = connection
        .object_server()
        .interface::<_, Service>(OBJECT_PATH)
        .await?;
    let emitter = reference.signal_emitter();
    Service::changed(emitter, revision).await?;
    let service = reference.get().await;
    service.counts_changed(emitter).await?;
    service.revision_changed(emitter).await
}
