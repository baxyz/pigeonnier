// SPDX-License-Identifier: AGPL-3.0-or-later

//! `projects.toml`: what a project is, and the exceptions to the conventions.

use std::path::Path;

use chrono::{Duration, NaiveTime};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::when::parse_duration;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid {path}: {source}")]
    Parse {
        path: String,
        source: Box<toml::de::Error>,
    },
    #[error("cannot serialize the configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub notifications: Notifications,
    #[serde(default, rename = "project")]
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub todoist_parent: Option<String>,
    pub claude_label: String,
    pub refresh: String,
    pub review_escalation_days: u32,
    /// Also lists the PRs of others assigned to me.
    pub include_assigned: bool,
    /// Leaves out review requests made to a team I am in.
    pub direct_reviews_only: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            todoist_parent: None,
            claude_label: "claude".to_owned(),
            refresh: "5m".to_owned(),
            review_escalation_days: 2,
            include_assigned: false,
            direct_reviews_only: false,
        }
    }
}

impl Defaults {
    #[must_use]
    pub fn refresh_interval(&self) -> Duration {
        parse_duration(&self.refresh).unwrap_or_else(|| Duration::minutes(5))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub quiet_start: String,
    pub quiet_end: String,
    pub digest: String,
    pub enabled: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            quiet_start: "20:00".to_owned(),
            quiet_end: "08:00".to_owned(),
            digest: "09:00".to_owned(),
            enabled: true,
        }
    }
}

impl Notifications {
    fn time(text: &str, fallback: (u32, u32)) -> NaiveTime {
        NaiveTime::parse_from_str(text, "%H:%M").unwrap_or_else(|_| {
            NaiveTime::from_hms_opt(fallback.0, fallback.1, 0).unwrap_or_default()
        })
    }

    #[must_use]
    pub fn is_quiet(&self, time: NaiveTime) -> bool {
        let start = Self::time(&self.quiet_start, (20, 0));
        let end = Self::time(&self.quiet_end, (8, 0));
        if start <= end {
            start <= time && time < end
        } else {
            time >= start || time < end
        }
    }

    #[must_use]
    pub fn digest_time(&self) -> NaiveTime {
        Self::time(&self.digest, (9, 0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todoist: Option<TodoistLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roadmap: Option<Roadmap>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
}

fn default_color() -> String {
    "blue".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubLink {
    /// A login, or `@me` for the signed-in account.
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<u32>,
    /// Only these repositories, as `owner/name`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoistLink {
    pub section: String,
    /// The Todoist project holding the section, when it is not `defaults.todoist_parent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// What the fields of a GitHub Project mean; the defaults fit GitHub's template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Roadmap {
    pub status_field: String,
    pub done_values: Vec<String>,
    pub priority_field: String,
    pub date_field: String,
    pub iteration_field: String,
    pub only_assigned: bool,
}

impl Default for Roadmap {
    fn default() -> Self {
        Self {
            status_field: "Status".to_owned(),
            done_values: vec!["Done".to_owned()],
            priority_field: "Priority".to_owned(),
            date_field: "Target date".to_owned(),
            iteration_field: "Iteration".to_owned(),
            only_assigned: false,
        }
    }
}

impl Config {
    /// # Errors
    /// [`ConfigError::Parse`] on invalid TOML.
    pub fn parse(text: &str, path: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source: Box::new(source),
        })
    }

    /// A missing file is an empty configuration: everything falls into "Autres".
    ///
    /// # Errors
    /// An unreadable or invalid file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let shown = path.display().to_string();
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text, &shown),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: shown,
                source,
            }),
        }
    }

    /// # Errors
    /// A configuration that TOML cannot hold.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// # Errors
    /// A file that cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let shown = path.display().to_string();
        let text = self.to_toml()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
                path: shown.clone(),
                source,
            })?;
        }
        std::fs::write(path, text).map_err(|source| ConfigError::Write {
            path: shown,
            source,
        })
    }

    /// The project a repository belongs to. `me` is the signed-in login, for `@me` owners.
    #[must_use]
    pub fn project_for_repo(&self, owner: &str, repo: &str, me: &str) -> Option<&Project> {
        let owner_matches = |project: &&Project| {
            project.github.as_ref().is_some_and(|link| {
                let login = if link.owner == "@me" {
                    me
                } else {
                    link.owner.as_str()
                };
                login.eq_ignore_ascii_case(owner)
            })
        };
        let lists_repo = |project: &&Project| {
            project.github.as_ref().is_some_and(|link| {
                link.repos
                    .iter()
                    .any(|listed| listed.eq_ignore_ascii_case(repo))
            })
        };
        let candidates = || self.projects.iter().filter(owner_matches);
        candidates().find(lists_repo).or_else(|| {
            candidates().find(|project| {
                project
                    .github
                    .as_ref()
                    .is_some_and(|link| link.repos.is_empty())
            })
        })
    }

    /// The project of a Todoist section, compared by normalized name.
    #[must_use]
    pub fn project_for_section(&self, section: &str, todoist_project: &str) -> Option<&Project> {
        let parent = self.defaults.todoist_parent.as_deref();
        self.projects.iter().find(|project| {
            project.todoist.as_ref().is_some_and(|link| {
                let expected = link.project.as_deref().or(parent);
                normalize_name(&link.section) == normalize_name(section)
                    && expected
                        .is_none_or(|name| normalize_name(name) == normalize_name(todoist_project))
            })
        })
    }
}

/// Ignores case, dashes, spaces and accents, so that "Sunny-Tech" pairs with "sunny tech".
#[must_use]
pub fn normalize_name(name: &str) -> String {
    name.nfd()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r##"
[defaults]
todoist_parent = "Dev"
claude_label   = "claude"
refresh        = "5m"

[[project]]
id      = "helpers4"
name    = "helpers4"
color   = "blue"
github  = { owner = "helpers4", project = 1 }
todoist = { section = "helpers4" }

[[project]]
id      = "perso"
name    = "Perso"
color   = "orange"
github  = { owner = "@me", project = 3, repos = ["baxyz/folioscope"] }
todoist = { section = "Perso", project = "Inbox" }

[[project]]
id      = "sunny-tech"
name    = "Sunny Tech"
color   = "#e5a50a"
github  = { owner = "sunnytech" }
todoist = { section = "Sunny Tech" }

[project.roadmap]
status_field = "État"
done_values = ["Fini"]
"##;

    fn config() -> Config {
        Config::parse(SPEC_EXAMPLE, "test").unwrap()
    }

    #[test]
    fn the_example_of_the_spec_parses() {
        let config = config();
        assert_eq!(config.projects.len(), 3);
        assert_eq!(config.defaults.refresh_interval(), Duration::minutes(5));
        let roadmap = config.projects[2].roadmap.as_ref().unwrap();
        assert_eq!(roadmap.status_field, "État");
        assert_eq!(roadmap.priority_field, "Priority");
    }

    #[test]
    fn it_survives_a_round_trip() {
        let config = config();
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(Config::parse(&text, "again").unwrap(), config);
    }

    #[test]
    fn repositories_route_by_owner() {
        let config = config();
        let id = |owner, repo| {
            config
                .project_for_repo(owner, repo, "baxyz")
                .map(|project| project.id.as_str())
        };
        assert_eq!(id("HELPERS4", "helpers4/x"), Some("helpers4"));
        assert_eq!(id("baxyz", "baxyz/folioscope"), Some("perso"));
        assert_eq!(id("baxyz", "baxyz/other"), None);
        assert_eq!(id("nobody", "nobody/x"), None);
    }

    #[test]
    fn sections_route_by_normalized_name() {
        let config = config();
        let id = |section, project| {
            config
                .project_for_section(section, project)
                .map(|project| project.id.as_str())
        };
        assert_eq!(id("sunny-tech", "Dev"), Some("sunny-tech"));
        assert_eq!(id("Perso", "Inbox"), Some("perso"));
        assert_eq!(id("Perso", "Dev"), None);
        assert_eq!(id("Sunny Tech", "Elsewhere"), None);
    }

    #[test]
    fn names_compare_without_case_dashes_or_accents() {
        assert_eq!(normalize_name("Sunny-Tech"), normalize_name("sunny tech"));
        assert_eq!(normalize_name("Été"), normalize_name("ete"));
    }

    #[test]
    fn quiet_hours_can_cross_midnight() {
        let quiet = Notifications::default();
        let at = |hour| NaiveTime::from_hms_opt(hour, 0, 0).unwrap();
        assert!(quiet.is_quiet(at(23)));
        assert!(quiet.is_quiet(at(7)));
        assert!(!quiet.is_quiet(at(8)));
        assert!(!quiet.is_quiet(at(19)));
    }
}
