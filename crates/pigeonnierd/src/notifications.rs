// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shows notifications through `org.freedesktop.Notifications`, and acts on their buttons.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Duration;
use futures_util::StreamExt as _;
use pigeonnier_core::notify::Notification;
use pigeonnier_core::when::parse_until;
use zbus::proxy;
use zbus::zvariant::Variant;

use crate::{Shared, bus, now};

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Variant<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;
}

/// What a notification on screen refers to, for when one of its buttons is pressed.
#[derive(Clone)]
struct Shown {
    item_id: Option<String>,
    url: Option<String>,
}

fn shown() -> &'static Mutex<HashMap<u32, Shown>> {
    static SHOWN: OnceLock<Mutex<HashMap<u32, Shown>>> = OnceLock::new();
    SHOWN.get_or_init(Mutex::default)
}

pub async fn show(
    connection: &zbus::Connection,
    _shared: &Shared,
    notification: Notification,
) -> zbus::Result<()> {
    let proxy = NotificationsProxy::new(connection).await?;
    let mut actions: Vec<&str> = Vec::new();
    for (id, label) in &notification.actions {
        actions.push(id);
        actions.push(label);
    }
    let id = proxy
        .notify(
            "Pigeonnier",
            0,
            "mail-unread-symbolic",
            &notification.title,
            &notification.body,
            &actions,
            HashMap::new(),
            -1,
        )
        .await?;
    if let Ok(mut map) = shown().lock() {
        map.insert(
            id,
            Shown {
                item_id: notification.item_id,
                url: notification.url,
            },
        );
    }
    Ok(())
}

async fn act(shared: &Shared, target: Shown, action: &str) {
    if action == "sidebar" {
        bus::spawn_app("sidebar");
        return;
    }
    if action == "open" {
        if let Some(url) = target.url
            && let Err(error) = std::process::Command::new("xdg-open").arg(&url).spawn()
        {
            eprintln!("pigeonnierd: cannot open {url}: {error}");
        }
        return;
    }
    let Some(item_id) = target.item_id else {
        return;
    };
    let engine = shared.engine.read().await;
    let result = if action == "complete" {
        engine.complete(&item_id).await
    } else if let Some(until) = action.strip_prefix("snooze:") {
        let at = parse_until(until, now()).unwrap_or_else(|| now() + Duration::hours(1));
        engine.snooze(&item_id, at).await
    } else {
        Ok(())
    };
    if let Err(error) = result {
        eprintln!("pigeonnierd: {action} failed on {item_id}: {error}");
    }
    shared.refresh.notify_one();
}

/// Waits for button presses for as long as the daemon runs. A desktop with no notification service
/// is not a failure: there is then nothing to wait for.
pub fn listen(connection: zbus::Connection, shared: Arc<Shared>) {
    tokio::spawn(async move {
        let Ok(proxy) = NotificationsProxy::new(&connection).await else {
            return;
        };
        let Ok(mut signals) = proxy.receive_action_invoked().await else {
            return;
        };
        while let Some(signal) = signals.next().await {
            let Ok(arguments) = signal.args() else {
                continue;
            };
            let target = shown()
                .lock()
                .ok()
                .and_then(|map| map.get(&arguments.id).cloned());
            if let Some(target) = target {
                act(&shared, target, &arguments.action_key).await;
            }
        }
    });
}
