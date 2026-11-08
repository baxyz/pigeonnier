// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Linux daemon: keeps the list fresh, serves it on the session bus, and notifies even with the
//! window closed. The app and the GNOME Shell extension are its clients and hold no token.

mod bus;
mod notifications;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local};
use pigeonnier_core::cache::Cache;
use pigeonnier_core::config::Config;
use pigeonnier_core::engine::{Engine, connect};
use pigeonnier_core::paths;
use pigeonnier_core::viewmodel::Snapshot;
use tokio::sync::{Notify, RwLock};

/// How often time-based things are looked at: snoozes ending, tasks due, the digest.
const TICK: Duration = Duration::from_secs(60);

pub struct Shared {
    pub engine: RwLock<Engine>,
    pub revision: AtomicU32,
    pub refresh: Notify,
}

#[must_use]
pub fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

/// What can change on screen, leaving out the time of the refresh itself.
fn fingerprint(snapshot: &Snapshot) -> String {
    let mut stable = snapshot.clone();
    stable.refreshed_at = None;
    serde_json::to_string(&stable).unwrap_or_default()
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

async fn reload_config(shared: &Shared, path: &PathBuf, seen: &mut Option<SystemTime>) {
    let current = modified(path);
    if current == *seen {
        return;
    }
    *seen = current;
    match Config::load(path) {
        Ok(config) => shared.engine.write().await.set_config(config),
        Err(error) => eprintln!("pigeonnierd: keeping the previous configuration: {error}"),
    }
}

async fn publish(shared: &Shared, connection: &zbus::Connection, last: &mut String) -> Result<()> {
    let engine = shared.engine.read().await;
    let snapshot = engine.snapshot("all", now())?;
    let print = fingerprint(&snapshot);
    if print != *last {
        *last = print;
        let revision = shared.revision.fetch_add(1, Ordering::SeqCst) + 1;
        bus::announce_change(connection, revision).await?;
    }
    for notification in engine.take_notifications(now())? {
        if let Err(error) = notifications::show(connection, shared, notification).await {
            eprintln!("pigeonnierd: cannot show a notification: {error}");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = paths::config_file();
    let mut config_seen = modified(&config_path);
    let engine = connect(
        Config::load(&config_path)?,
        Cache::open(&paths::cache_file())?,
    );
    let interval = engine
        .config()
        .defaults
        .refresh_interval()
        .to_std()
        .unwrap_or(Duration::from_secs(300));
    let shared = Arc::new(Shared {
        engine: RwLock::new(engine),
        revision: AtomicU32::new(0),
        refresh: Notify::new(),
    });

    let connection = bus::serve(Arc::clone(&shared)).await?;
    notifications::listen(connection.clone(), Arc::clone(&shared));

    let mut last = String::new();
    let mut tick = tokio::time::interval(TICK);
    let mut next_refresh = tokio::time::Instant::now();
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(next_refresh) => {}
            () = shared.refresh.notified() => {}
            _ = tick.tick() => {
                reload_config(&shared, &config_path, &mut config_seen).await;
                if let Err(error) = publish(&shared, &connection, &mut last).await {
                    eprintln!("pigeonnierd: {error}");
                }
                continue;
            }
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
        reload_config(&shared, &config_path, &mut config_seen).await;
        if let Err(error) = shared.engine.read().await.refresh().await {
            eprintln!("pigeonnierd: refresh failed: {error}");
        }
        next_refresh = tokio::time::Instant::now() + interval;
        if let Err(error) = publish(&shared, &connection, &mut last).await {
            eprintln!("pigeonnierd: {error}");
        }
    }
}
