// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one interface the rest of the application talks to: refresh from the services, get a view,
//! act on an item. Sources that fail keep their last answer and are reported next to the list.

use std::collections::HashSet;

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::{Cache, CacheError};
use crate::config::Config;
use crate::item::{Item, Kind, Priority};
use crate::normalize::{Context, pull_request, roadmap_item, todoist_item};
use crate::notify;
use crate::source::SourceError;
use crate::source::github::{GithubClient, ProjectRead, raw_project_item, raw_pull_request};
use crate::source::todoist::{SyncState, TodoistClient, TodoistCommand};
use crate::view::{View, default_views};
use crate::viewmodel::{self, Issue, Snapshot};

const KEY_PRS: &str = "github.prs";
const KEY_ROADMAP: &str = "github.roadmap.";
const KEY_TODOIST: &str = "todoist.state";
const KEY_ISSUES: &str = "issues";
const KEY_REFRESHED: &str = "refreshed_at";

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Cache(#[from] CacheError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("{0} is not connected")]
    NotConnected(&'static str),
    #[error("{0} no longer exists: it changed on the service since the last refresh")]
    Gone(String),
    #[error("{0} is not a Todoist task")]
    NotTodoist(String),
    #[error("no project {0} with a Todoist section")]
    UnknownProject(String),
}

/// The last answer of the pull request searches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PrCache {
    me: String,
    reviews: Vec<Value>,
    mine: Vec<Value>,
    assigned: Vec<Value>,
}

/// A manual action to create in a project's Todoist section.
#[derive(Debug, Clone, Default)]
pub struct NewAction {
    pub project: String,
    pub title: String,
    pub due: Option<String>,
    pub priority: Option<Priority>,
    pub pr_url: Option<String>,
}

pub struct Engine {
    config: Config,
    views: Vec<View>,
    cache: Cache,
    github: Option<GithubClient>,
    todoist: Option<TodoistClient>,
}

impl Engine {
    #[must_use]
    pub fn new(
        config: Config,
        cache: Cache,
        github: Option<GithubClient>,
        todoist: Option<TodoistClient>,
    ) -> Self {
        Self {
            config,
            views: default_views(),
            cache,
            github,
            todoist,
        }
    }

    /// Adds user views after the default ones; one with the id of a default view replaces it.
    pub fn add_views(&mut self, views: Vec<View>) {
        for view in views {
            match self.views.iter_mut().find(|known| known.id == view.id) {
                Some(known) => *known = view,
                None => self.views.push(view),
            }
        }
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn set_config(&mut self, config: Config) {
        self.config = config;
    }

    #[must_use]
    pub fn views(&self) -> &[View] {
        &self.views
    }

    /// Fetches every source, one failing without stopping the others. Returns what failed.
    ///
    /// # Errors
    /// Only a cache failure: source failures are reported in the returned list.
    pub async fn refresh(&self) -> Result<Vec<Issue>, EngineError> {
        let mut issues = Vec::new();
        let mut me = None;
        if let Some(github) = &self.github {
            match self.refresh_prs(github).await {
                Ok(login) => me = Some(login),
                Err(error) => issues.push(issue("GitHub", &error)),
            }
        } else {
            issues.push(not_connected("GitHub"));
        }
        if let Some(github) = &self.github {
            let me = me.or(self.cached_prs()?.map(|prs| prs.me));
            self.refresh_roadmaps(github, me.as_deref(), &mut issues)
                .await?;
        }
        if let Some(todoist) = &self.todoist {
            match self.sync_todoist(todoist).await {
                Ok(_) => {}
                Err(EngineError::Source(source)) => issues.push(issue("Todoist", &source)),
                Err(other) => return Err(other),
            }
        } else {
            issues.push(not_connected("Todoist"));
        }
        self.cache.put(KEY_ISSUES, &issues)?;
        self.cache.put(KEY_REFRESHED, &Utc::now())?;
        Ok(issues)
    }

    fn cached_prs(&self) -> Result<Option<PrCache>, CacheError> {
        self.cache.get(KEY_PRS)
    }

    async fn refresh_prs(&self, github: &GithubClient) -> Result<String, SourceError> {
        let defaults = &self.config.defaults;
        let requested = if defaults.direct_reviews_only {
            "user-review-requested:@me"
        } else {
            "review-requested:@me"
        };
        let base = "is:pr is:open archived:false";
        let me = github.viewer_login().await?;
        let reviews = github
            .search_pull_requests(&format!("{base} {requested}"))
            .await?;
        let mine = github
            .search_pull_requests(&format!("{base} author:@me"))
            .await?;
        let assigned = if defaults.include_assigned {
            github
                .search_pull_requests(&format!("{base} assignee:@me -author:@me"))
                .await?
        } else {
            Vec::new()
        };
        let cache = PrCache {
            me: me.clone(),
            reviews,
            mine,
            assigned,
        };
        self.cache
            .put(KEY_PRS, &cache)
            .map_err(|error| SourceError::Shape(error.to_string()))?;
        Ok(me)
    }

    async fn refresh_roadmaps(
        &self,
        github: &GithubClient,
        me: Option<&str>,
        issues: &mut Vec<Issue>,
    ) -> Result<(), EngineError> {
        for project in &self.config.projects {
            let Some(link) = &project.github else {
                continue;
            };
            let Some(number) = link.project else {
                continue;
            };
            let owner = if link.owner == "@me" {
                match me {
                    Some(me) => me,
                    None => continue,
                }
            } else {
                link.owner.as_str()
            };
            let source = format!("Project {number} de {owner}");
            match github.read_project(owner, number).await {
                Ok(ProjectRead::Found(items)) => {
                    self.cache
                        .put(&format!("{KEY_ROADMAP}{}", project.id), &items)?;
                }
                Ok(ProjectRead::Inaccessible) => issues.push(Issue {
                    source,
                    message: "le token ne peut pas lire ce Project".to_owned(),
                    reconnect: true,
                }),
                Ok(ProjectRead::NotFound) => issues.push(Issue {
                    source,
                    message: "ce Project n'existe pas".to_owned(),
                    reconnect: false,
                }),
                Err(error) => issues.push(issue(&source, &error)),
            }
        }
        Ok(())
    }

    async fn sync_todoist(&self, todoist: &TodoistClient) -> Result<SyncState, EngineError> {
        let mut state: SyncState = self.cache.get(KEY_TODOIST)?.unwrap_or_default();
        let response = todoist.sync(state.next_token()).await?;
        state.apply(response);
        self.cache.put(KEY_TODOIST, &state)?;
        Ok(state)
    }

    /// Everything known, from the cache: normalizing again at each read keeps age rules current.
    ///
    /// # Errors
    /// A cache failure.
    pub fn items(&self, now: DateTime<FixedOffset>) -> Result<Vec<Item>, CacheError> {
        let prs = self.cached_prs()?.unwrap_or_default();
        let context = Context {
            config: &self.config,
            me: &prs.me,
            now,
        };
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        // A PR can be in several searches: a review request is the most urgent reading of it.
        let searches = [
            (&prs.reviews, Kind::Review),
            (&prs.mine, Kind::MyPr),
            (&prs.assigned, Kind::MyPr),
        ];
        for (nodes, kind) in searches {
            for node in nodes {
                let item = pull_request(&raw_pull_request(node), kind, &context);
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
        }
        for project in &self.config.projects {
            let key = format!("{KEY_ROADMAP}{}", project.id);
            let nodes: Vec<Value> = self.cache.get(&key)?.unwrap_or_default();
            items.extend(
                nodes
                    .iter()
                    .filter_map(|node| roadmap_item(&raw_project_item(node), project, &context)),
            );
        }
        let sync: SyncState = self.cache.get(KEY_TODOIST)?.unwrap_or_default();
        items.extend(
            sync.raw_items()
                .iter()
                .filter_map(|raw| todoist_item(raw, &sync, &context)),
        );
        let snoozes = self.cache.snoozes()?;
        for item in &mut items {
            item.snoozed_until = snoozes.get(&item.id).copied();
        }
        Ok(items)
    }

    /// What a UI draws for a view.
    ///
    /// # Errors
    /// A cache failure.
    pub fn snapshot(&self, view: &str, now: DateTime<FixedOffset>) -> Result<Snapshot, CacheError> {
        Ok(viewmodel::build(
            &self.items(now)?,
            &self.config,
            &self.views,
            view,
            now,
            self.cache.get(KEY_ISSUES)?.unwrap_or_default(),
            self.cache.get(KEY_REFRESHED)?,
        ))
    }

    /// Marks a Todoist task done. Syncs first and refuses if the task is already gone, rather than
    /// acting on what the list showed a while ago.
    ///
    /// # Errors
    /// Not a task, no Todoist connection, the task is gone, or a service failure.
    pub async fn complete(&self, item_id: &str) -> Result<(), EngineError> {
        let id = todoist_id(item_id)?;
        let todoist = self
            .todoist
            .as_ref()
            .ok_or(EngineError::NotConnected("Todoist"))?;
        let state = self.sync_todoist(todoist).await?;
        let open = state
            .items
            .get(id)
            .is_some_and(|item| item["checked"] != Value::Bool(true));
        if !open {
            return Err(EngineError::Gone(item_id.to_owned()));
        }
        todoist
            .command(&TodoistCommand::Close { id: id.to_owned() })
            .await?;
        self.sync_todoist(todoist).await?;
        Ok(())
    }

    /// Reports an item. A Todoist task really moves; GitHub has no such notion, so a PR or a roadmap
    /// item is hidden locally.
    ///
    /// # Errors
    /// No Todoist connection for a task, the task is gone, or a service or cache failure.
    pub async fn snooze(
        &self,
        item_id: &str,
        until: DateTime<FixedOffset>,
    ) -> Result<(), EngineError> {
        let Ok(id) = todoist_id(item_id) else {
            if !self.items(until)?.iter().any(|item| item.id == item_id) {
                return Err(EngineError::Gone(item_id.to_owned()));
            }
            self.cache.snooze(item_id, until.with_timezone(&Utc))?;
            return Ok(());
        };
        let todoist = self
            .todoist
            .as_ref()
            .ok_or(EngineError::NotConnected("Todoist"))?;
        let state = self.sync_todoist(todoist).await?;
        let item = state
            .items
            .get(id)
            .ok_or_else(|| EngineError::Gone(item_id.to_owned()))?;
        let recurring = item.pointer("/due/is_recurring") == Some(&Value::Bool(true));
        let string = recurring
            .then(|| item.pointer("/due/string").and_then(Value::as_str))
            .flatten()
            .map(str::to_owned);
        todoist
            .command(&TodoistCommand::Reschedule {
                id: id.to_owned(),
                date: until.date_naive().to_string(),
                string,
            })
            .await?;
        self.sync_todoist(todoist).await?;
        Ok(())
    }

    /// Creates a manual action in the section of a project, with the label the convention asks for.
    ///
    /// # Errors
    /// A project with no section, no Todoist connection, or a service failure.
    pub async fn add_manual_action(&self, action: NewAction) -> Result<(), EngineError> {
        let todoist = self
            .todoist
            .as_ref()
            .ok_or(EngineError::NotConnected("Todoist"))?;
        let project = self
            .config
            .projects
            .iter()
            .find(|project| project.id == action.project)
            .ok_or_else(|| EngineError::UnknownProject(action.project.clone()))?;
        let link = project
            .todoist
            .as_ref()
            .ok_or_else(|| EngineError::UnknownProject(action.project.clone()))?;
        let state = self.sync_todoist(todoist).await?;
        let wanted = crate::config::normalize_name(&link.section);
        let section = state
            .sections
            .values()
            .find(|section| {
                section["name"]
                    .as_str()
                    .is_some_and(|name| crate::config::normalize_name(name) == wanted)
            })
            .ok_or_else(|| EngineError::UnknownProject(action.project.clone()))?;
        todoist
            .command(&TodoistCommand::Add {
                content: action.title,
                project_id: section["project_id"].as_str().map(str::to_owned),
                section_id: section["id"].as_str().map(str::to_owned),
                labels: vec![self.config.defaults.claude_label.clone()],
                priority: action.priority.map(|priority| 5 - priority.rank()),
                due_string: action.due,
                description: action.pr_url,
            })
            .await?;
        self.sync_todoist(todoist).await?;
        Ok(())
    }

    /// Notifications to show now, recording them so each goes out once.
    ///
    /// # Errors
    /// A cache failure.
    pub fn take_notifications(
        &self,
        now: DateTime<FixedOffset>,
    ) -> Result<Vec<notify::Notification>, CacheError> {
        let items = self.items(now)?;
        let seed = !self.cache.has_notification_history()?;
        let sent = |key: &str| self.cache.was_notified(key).unwrap_or(false);
        let plan = notify::plan(&items, &self.config, now, &sent, seed);
        let at = now.with_timezone(&Utc);
        for key in &plan.mark {
            self.cache.mark_notified(key, at)?;
        }
        Ok(plan.send)
    }

    /// Forgets a service's copy, for when it is disconnected.
    ///
    /// # Errors
    /// A cache failure.
    pub fn purge_service(&self, prefix: &str) -> Result<(), CacheError> {
        self.cache.purge(prefix)
    }
}

fn todoist_id(item_id: &str) -> Result<&str, EngineError> {
    item_id
        .strip_prefix("td:")
        .ok_or_else(|| EngineError::NotTodoist(item_id.to_owned()))
}

fn issue(source: &str, error: &SourceError) -> Issue {
    Issue {
        source: source.to_owned(),
        message: error.to_string(),
        reconnect: error.needs_reconnect(),
    }
}

fn not_connected(source: &str) -> Issue {
    Issue {
        source: source.to_owned(),
        message: "non connecté".to_owned(),
        reconnect: true,
    }
}

/// Builds an engine with the tokens available on this machine. `PIGEONNIER_GITHUB_API` and
/// `PIGEONNIER_TODOIST_API` point it at other servers, which is how the daemon is tested.
#[must_use]
pub fn connect(config: Config, cache: Cache) -> Engine {
    let github = crate::credentials::github_token().map(|token| {
        match std::env::var("PIGEONNIER_GITHUB_API") {
            Ok(endpoint) => GithubClient::with_endpoint(token, endpoint),
            Err(_) => GithubClient::new(token),
        }
    });
    let todoist = crate::credentials::todoist_token().map(|token| {
        match std::env::var("PIGEONNIER_TODOIST_API") {
            Ok(base) => TodoistClient::with_base(token, base),
            Err(_) => TodoistClient::new(token),
        }
    });
    let mut engine = Engine::new(config, cache, github, todoist);
    if let Ok(text) = std::fs::read_to_string(crate::paths::views_file()) {
        match serde_json::from_str(&text) {
            Ok(views) => engine.add_views(views),
            Err(error) => eprintln!("pigeonnier: ignoring views.json: {error}"),
        }
    }
    engine
}
