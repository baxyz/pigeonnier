// SPDX-License-Identifier: AGPL-3.0-or-later

//! The engine, inside the app: it keeps the list fresh in the background and runs the actions the
//! window asks for. Every function returns a message a person can read when it fails.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, FixedOffset, Local};
use pigeonnier_core::cache::Cache;
use pigeonnier_core::config::Config;
use pigeonnier_core::credentials;
use pigeonnier_core::engine::{Choices, Engine, connect, github_client, todoist_session_client};
use pigeonnier_core::oauth::{CALLBACK_PORT, github_device_flow, start_todoist_login};
use pigeonnier_core::paths;
use pigeonnier_core::viewmodel::Snapshot;
use pigeonnier_core::when::parse_until;
use tokio::sync::{Notify, RwLock};

/// What the window hears from the background.
#[derive(Debug)]
pub enum Event {
    /// The list may have changed: read it again.
    Changed,
    /// Something failed in the background: starting, or a connection started earlier.
    Failed(String),
}

type Listener = Box<dyn Fn(Event) + Send + Sync>;

struct Service {
    engine: RwLock<Engine>,
    refresh: Notify,
    listener: Listener,
    config_path: PathBuf,
    config_seen: Mutex<Option<SystemTime>>,
}

static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();

fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

#[allow(clippy::needless_pass_by_value)] // Used as `map_err(readable)`.
fn readable(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn service() -> Result<Arc<Service>, String> {
    SERVICE
        .get()
        .cloned()
        .ok_or_else(|| "Pigeonnier n'a pas encore démarré".to_owned())
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

impl Service {
    fn mark_config_seen(&self) {
        *self
            .config_seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = modified(&self.config_path);
    }

    /// Picks up a configuration edited by hand.
    async fn reload_config(&self) {
        let current = modified(&self.config_path);
        {
            let mut seen = self
                .config_seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if current == *seen {
                return;
            }
            seen.clone_from(&current);
        }
        match Config::load(&self.config_path) {
            Ok(config) => self.engine.write().await.set_config(config),
            Err(error) => eprintln!("pigeonnier: keeping the previous configuration: {error}"),
        }
    }

    async fn run(self: Arc<Self>) {
        loop {
            self.reload_config().await;
            let interval = {
                let engine = self.engine.read().await;
                if let Err(error) = engine.refresh().await {
                    eprintln!("pigeonnier: refresh failed: {error}");
                }
                engine
                    .config()
                    .defaults
                    .refresh_interval()
                    .to_std()
                    .unwrap_or(Duration::from_secs(300))
            };
            (self.listener)(Event::Changed);
            tokio::select! {
                () = tokio::time::sleep(interval) => {}
                () = self.refresh.notified() => {}
            }
        }
    }
}

/// Opens the configuration, the cache and the keyring in the background, then keeps the list fresh
/// until the app quits. The window hears [`Event::Changed`] as soon as the cached list can be read.
pub fn start(listener: impl Fn(Event) + Send + Sync + 'static) {
    let listener: Listener = Box::new(listener);
    relm4::spawn(async move {
        let config_path = paths::config_file();
        let opened = tokio::task::spawn_blocking({
            let config_path = config_path.clone();
            move || {
                let config = Config::load(&config_path).map_err(readable)?;
                let cache = Cache::open(&paths::cache_file()).map_err(readable)?;
                // Reading the tokens waits on the keyring: never on the window's thread.
                Ok::<_, String>(connect(config, cache))
            }
        })
        .await
        .map_err(readable)
        .and_then(|opened| opened);
        let engine = match opened {
            Ok(engine) => engine,
            Err(error) => return listener(Event::Failed(error)),
        };
        let service = Arc::new(Service {
            engine: RwLock::new(engine),
            refresh: Notify::new(),
            listener,
            config_seen: Mutex::new(modified(&config_path)),
            config_path,
        });
        if SERVICE.set(Arc::clone(&service)).is_ok() {
            (service.listener)(Event::Changed);
            service.run().await;
        }
    });
}

/// Asks for a refresh now; the window hears [`Event::Changed`] when it is done.
pub fn refresh() {
    if let Ok(service) = service() {
        service.refresh.notify_one();
    }
}

pub async fn load(view: &str) -> Result<Snapshot, String> {
    let service = service()?;
    let engine = service.engine.read().await;
    engine.snapshot(view, now()).map_err(readable)
}

pub async fn complete(item_id: &str) -> Result<(), String> {
    let service = service()?;
    let result = service.engine.read().await.complete(item_id).await;
    service.refresh.notify_one();
    result.map_err(readable)
}

pub async fn snooze(item_id: &str, until: &str) -> Result<(), String> {
    let until = parse_until(until, now()).ok_or_else(|| format!("je ne comprends pas {until}"))?;
    let service = service()?;
    let result = service.engine.read().await.snooze(item_id, until).await;
    service.refresh.notify_one();
    result.map_err(readable)
}

pub async fn merge(item_id: &str) -> Result<(), String> {
    let service = service()?;
    let result = service.engine.read().await.merge(item_id, now()).await;
    service.refresh.notify_one();
    result.map_err(readable)
}

/// Adds a task in natural language. No project means the Todoist inbox.
pub async fn add_task(project: Option<String>, text: &str) -> Result<(), String> {
    let service = service()?;
    let result = service
        .engine
        .read()
        .await
        .quick_add(project.as_deref(), text)
        .await;
    service.refresh.notify_one();
    result.map_err(readable)
}

/// The configuration in use, to edit in the settings.
pub async fn config() -> Result<Config, String> {
    Ok(service()?.engine.read().await.config().clone())
}

/// What the settings can offer, read from the connected services.
pub async fn choices() -> Result<Choices, String> {
    service()?
        .engine
        .read()
        .await
        .choices()
        .await
        .map_err(readable)
}

/// Writes the configuration and refreshes with it.
pub async fn save_config(config: Config) -> Result<(), String> {
    let service = service()?;
    config.save(&service.config_path).map_err(readable)?;
    service.mark_config_seen();
    service.engine.write().await.set_config(config);
    service.refresh.notify_one();
    Ok(())
}

/// Starts connecting GitHub: returns the code to enter and the page to enter it on, and goes on
/// waiting for the person in the background.
pub async fn connect_github() -> Result<(String, String), String> {
    let service = service()?;
    let flow = github_device_flow().ok_or_else(|| {
        "Cette version de Pigeonnier ne sait pas encore se connecter à GitHub : il lui manque \
         l'identifiant de son application GitHub."
            .to_owned()
    })?;
    let code = flow.start().await.map_err(readable)?;
    let answer = (code.user_code.clone(), code.verification_uri.clone());
    relm4::spawn(async move {
        match flow.wait_for_token(&code).await {
            Ok(token) => {
                if let Err(error) = credentials::store(credentials::GITHUB, &token) {
                    eprintln!("pigeonnier: GitHub stays connected until the app quits: {error}");
                }
                service
                    .engine
                    .write()
                    .await
                    .set_github(Some(github_client(token)));
                service.refresh.notify_one();
            }
            Err(error) => (service.listener)(Event::Failed(format!(
                "La connexion à GitHub a échoué : {error}"
            ))),
        }
    });
    Ok(answer)
}

/// Starts connecting Todoist: returns the page to open, and waits for the browser to come back.
pub async fn connect_todoist() -> Result<String, String> {
    let service = service()?;
    let metadata = std::env::var("PIGEONNIER_TODOIST_METADATA").ok();
    let login = start_todoist_login(metadata.as_deref(), CALLBACK_PORT)
        .await
        .map_err(readable)?;
    let url = login.url.clone();
    relm4::spawn(async move {
        let result = async {
            let session = login.finish().await.map_err(readable)?;
            credentials::store_todoist_session(&session).map_err(readable)
        };
        match result.await {
            Ok(()) => {
                service
                    .engine
                    .write()
                    .await
                    .set_todoist(Some(todoist_session_client()));
                service.refresh.notify_one();
            }
            Err(error) => (service.listener)(Event::Failed(format!(
                "La connexion à Todoist a échoué : {error}"
            ))),
        }
    });
    Ok(url)
}

/// Removes the tokens of `github` or `todoist` and forgets what was read from it.
pub async fn disconnect(service_name: &str) -> Result<(), String> {
    let service = service()?;
    {
        let mut engine = service.engine.write().await;
        match service_name {
            "github" => {
                credentials::forget(credentials::GITHUB).map_err(readable)?;
                engine.set_github(None);
                engine.purge_service("github.").map_err(readable)?;
            }
            "todoist" => {
                credentials::forget_todoist().map_err(readable)?;
                engine.set_todoist(None);
                engine.purge_service("todoist.").map_err(readable)?;
            }
            other => return Err(format!("service inconnu : {other}")),
        }
    }
    service.refresh.notify_one();
    Ok(())
}
