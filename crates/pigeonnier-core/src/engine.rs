// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one interface the rest of the application talks to: refresh from the services, get a view,
//! act on an item. Sources that fail keep their last answer and are reported next to the list.

use std::collections::HashSet;

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::{Cache, CacheError};
use crate::config::Config;
use crate::discover;
use crate::item::{Item, Kind, Priority, Reason, State};
use crate::normalize::{Context, pull_request, roadmap_item, todoist_item};
use crate::notify;
use crate::source::SourceError;
use crate::source::github::{GithubClient, Owner, ProjectRead, raw_project_item, raw_pull_request};
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
    #[error("{0} is not ready to merge: it changed since the last refresh")]
    NotMergeable(String),
    #[error("no Todoist project named {0}")]
    UnknownParent(String),
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

/// What the settings can offer for a project.
#[derive(Debug, Clone, Default)]
pub struct Choices {
    /// The signed-in account first, then its organizations.
    pub owners: Vec<Owner>,
    pub todoist: Vec<TodoistChoice>,
}

/// A Todoist project and its sections, in the order Todoist shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoistChoice {
    pub name: String,
    pub sections: Vec<String>,
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

    pub fn set_todoist(&mut self, todoist: Option<TodoistClient>) {
        self.todoist = todoist;
    }

    pub fn set_github(&mut self, github: Option<GithubClient>) {
        self.github = github;
    }
    /// Proposes a configuration from the GitHub owners and, with a parent project, Todoist sections.
    ///
    /// # Errors
    /// A missing connection, an unknown parent, or a service failure.
    pub async fn propose_config(&self, parent: Option<&str>) -> Result<Config, EngineError> {
        let github = self
            .github
            .as_ref()
            .ok_or(EngineError::NotConnected("GitHub"))?;
        let owners = github.discover_owners().await?;
        let sections = match parent {
            Some(parent) => {
                let todoist = self
                    .todoist
                    .as_ref()
                    .ok_or(EngineError::NotConnected("Todoist"))?;
                let state = self.sync_todoist(todoist).await?;
                let projects: Vec<Value> = state.projects.values().cloned().collect();
                let sections: Vec<Value> = state.sections.values().cloned().collect();
                discover::sections_of(&projects, &sections, parent)
                    .ok_or_else(|| EngineError::UnknownParent(parent.to_owned()))?
            }
            None => Vec::new(),
        };
        Ok(discover::propose(&owners, &sections, parent))
    }

    /// What the settings can offer: the GitHub owners with their Projects, and the Todoist projects
    /// with their sections. A service that is not connected offers nothing.
    ///
    /// # Errors
    /// A service failure, or the cache.
    pub async fn choices(&self) -> Result<Choices, EngineError> {
        let owners = match &self.github {
            Some(github) => github.discover_owners().await?,
            None => Vec::new(),
        };
        let todoist = match &self.todoist {
            Some(todoist) => todoist_choices(&self.sync_todoist(todoist).await?),
            None => Vec::new(),
        };
        Ok(Choices { owners, todoist })
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
        let mut snapshot = viewmodel::build(
            &self.items(now)?,
            &self.config,
            &self.views,
            view,
            now,
            self.cache.get(KEY_ISSUES)?.unwrap_or_default(),
            self.cache.get(KEY_REFRESHED)?,
        );
        snapshot.github_connected = self.github.is_some();
        snapshot.todoist_connected = self.todoist.is_some();
        Ok(snapshot)
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

    /// Adds a task from natural language: `relire la doc vendredi p2`. With a project, it goes to
    /// that project's section; without, Todoist puts it in its inbox.
    ///
    /// # Errors
    /// A project with no section, no Todoist connection, or a service failure.
    pub async fn quick_add(&self, project: Option<&str>, text: &str) -> Result<(), EngineError> {
        let todoist = self
            .todoist
            .as_ref()
            .ok_or(EngineError::NotConnected("Todoist"))?;
        let target = project.map(|id| self.quick_add_target(id)).transpose()?;
        let line = match target {
            Some(target) => format!("{text} {target}"),
            None => text.to_owned(),
        };
        todoist.quick_add(&line).await?;
        self.sync_todoist(todoist).await?;
        Ok(())
    }

    fn quick_add_target(&self, project_id: &str) -> Result<String, EngineError> {
        let unknown = || EngineError::UnknownProject(project_id.to_owned());
        let project = self
            .config
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .ok_or_else(unknown)?;
        let link = project.todoist.as_ref().ok_or_else(unknown)?;
        let parent = link
            .project
            .as_deref()
            .or(self.config.defaults.todoist_parent.as_deref());
        let section = format!("/{}", escape_quick_add(&link.section));
        Ok(match parent {
            Some(parent) => format!("#{} {section}", escape_quick_add(parent)),
            None => section,
        })
    }

    /// Merges a PR of mine with the repository's default method, after looking at it again: the
    /// state on screen may be minutes old, and GitHub refuses if commits arrived in between.
    ///
    /// # Errors
    /// Not a PR of mine ready to merge, no GitHub connection, or GitHub refusing.
    pub async fn merge(
        &self,
        item_id: &str,
        now: DateTime<FixedOffset>,
    ) -> Result<(), EngineError> {
        let github = self
            .github
            .as_ref()
            .ok_or(EngineError::NotConnected("GitHub"))?;
        self.refresh_prs(github).await?;
        let prs = self.cached_prs()?.unwrap_or_default();
        let context = Context {
            config: &self.config,
            me: &prs.me,
            now,
        };
        let node = prs.mine.iter().find(|node| {
            let item = pull_request(&raw_pull_request(node), Kind::MyPr, &context);
            item.id == item_id && item.state == State::ActionRequired(Reason::ToMerge)
        });
        let node = node.ok_or_else(|| EngineError::NotMergeable(item_id.to_owned()))?;
        github
            .merge_pull_request(
                node["id"].as_str().unwrap_or_default(),
                node.pointer("/repository/viewerDefaultMergeMethod")
                    .and_then(Value::as_str),
                node["headRefOid"].as_str(),
            )
            .await?;
        self.refresh_prs(github).await?;
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

/// Quick Add reads a space as the end of a name, unless it is escaped.
fn escape_quick_add(name: &str) -> String {
    name.replace(' ', "\\ ")
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

fn order(value: &Value, field: &str) -> i64 {
    value[field].as_i64().unwrap_or(0)
}

fn is_archived(value: &Value) -> bool {
    value["is_archived"] == Value::Bool(true)
}

/// The Todoist projects and their sections, as the Todoist app orders them.
fn todoist_choices(state: &SyncState) -> Vec<TodoistChoice> {
    let mut projects: Vec<&Value> = state
        .projects
        .values()
        .filter(|project| !is_archived(project))
        .collect();
    projects.sort_by_key(|project| order(project, "child_order"));
    projects
        .into_iter()
        .filter_map(|project| {
            let id = project["id"].as_str()?;
            let mut sections: Vec<&Value> = state
                .sections
                .values()
                .filter(|section| section["project_id"].as_str() == Some(id))
                .filter(|section| !is_archived(section))
                .collect();
            sections.sort_by_key(|section| order(section, "section_order"));
            Some(TodoistChoice {
                name: project["name"].as_str()?.to_owned(),
                sections: sections
                    .into_iter()
                    .filter_map(|section| section["name"].as_str().map(str::to_owned))
                    .collect(),
            })
        })
        .collect()
}

fn not_connected(source: &str) -> Issue {
    Issue {
        source: source.to_owned(),
        message: "non connecté".to_owned(),
        reconnect: true,
    }
}

#[must_use]
pub fn github_client(token: String) -> GithubClient {
    match std::env::var("PIGEONNIER_GITHUB_API") {
        Ok(endpoint) => GithubClient::with_endpoint(token, endpoint),
        Err(_) => GithubClient::new(token),
    }
}

#[must_use]
pub fn todoist_session_client() -> TodoistClient {
    TodoistClient::with_session(std::env::var("PIGEONNIER_TODOIST_API").ok())
}

#[must_use]
pub fn todoist_client(token: String) -> TodoistClient {
    match std::env::var("PIGEONNIER_TODOIST_API") {
        Ok(base) => TodoistClient::with_base(token, base),
        Err(_) => TodoistClient::new(token),
    }
}

/// Builds an engine with the tokens available on this machine. `PIGEONNIER_GITHUB_API` and
/// `PIGEONNIER_TODOIST_API` point it at other servers, which is how the app is tested.
#[must_use]
pub fn connect(config: Config, cache: Cache) -> Engine {
    let github = crate::credentials::github_token().map(github_client);
    let todoist = crate::credentials::todoist_token()
        .map(todoist_client)
        .or_else(|| crate::credentials::todoist_session().map(|_| todoist_session_client()));
    let mut engine = Engine::new(config, cache, github, todoist);
    if let Ok(text) = std::fs::read_to_string(crate::paths::views_file()) {
        match serde_json::from_str(&text) {
            Ok(views) => engine.add_views(views),
            Err(error) => eprintln!("pigeonnier: ignoring views.json: {error}"),
        }
    }
    engine
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn todoist_choices_follow_the_order_of_the_app_and_skip_archives() {
        let mut state = SyncState::default();
        for project in [
            json!({ "id": "p2", "name": "Perso", "child_order": 2 }),
            json!({ "id": "p1", "name": "Dev", "child_order": 1 }),
            json!({ "id": "p3", "name": "Old", "child_order": 0, "is_archived": true }),
        ] {
            state
                .projects
                .insert(project["id"].as_str().unwrap().to_owned(), project);
        }
        for section in [
            json!({ "id": "s2", "project_id": "p1", "name": "helpers4", "section_order": 2 }),
            json!({ "id": "s1", "project_id": "p1", "name": "baxyz", "section_order": 1 }),
            json!({ "id": "s3", "project_id": "p1", "name": "gone", "is_archived": true }),
        ] {
            state
                .sections
                .insert(section["id"].as_str().unwrap().to_owned(), section);
        }
        assert_eq!(
            todoist_choices(&state),
            vec![
                TodoistChoice {
                    name: "Dev".to_owned(),
                    sections: vec!["baxyz".to_owned(), "helpers4".to_owned()],
                },
                TodoistChoice {
                    name: "Perso".to_owned(),
                    sections: Vec::new(),
                },
            ]
        );
    }
}
