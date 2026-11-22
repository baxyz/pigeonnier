// SPDX-License-Identifier: AGPL-3.0-or-later

//! The first run: one owner, one project and one section make a Pigeonnier project, and the
//! convention lets it be proposed without asking anything.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::{
    Config, Defaults, GithubLink, Notifications, Project, TodoistLink, normalize_name,
};
use crate::engine::TodoistChoice;
use crate::item::{Item, OTHERS};
use crate::source::github::Owner;

const PALETTE: [&str; 7] = [
    "blue", "green", "yellow", "orange", "red", "purple", "brown",
];

/// Proposes a configuration from the owners found on GitHub (the signed-in account first) and the
/// sections of the parent Todoist project, paired by normalized name. Sections that pair with no
/// owner become projects of their own, because tasks would otherwise land in "Autres".
#[must_use]
pub fn propose(owners: &[Owner], sections: &[String], parent: Option<&str>) -> Config {
    let mut unpaired: Vec<&String> = sections.iter().collect();
    let mut projects: Vec<Project> = Vec::new();
    for (index, owner) in owners.iter().enumerate() {
        let is_account = index == 0;
        let name = if is_account {
            "Perso"
        } else {
            owner.login.as_str()
        };
        let wanted = [normalize_name(&owner.login), normalize_name(name)];
        let section = unpaired
            .iter()
            .position(|section| wanted.contains(&normalize_name(section)))
            .map(|position| unpaired.remove(position));
        projects.push(Project {
            id: slug(name),
            name: name.to_owned(),
            color: color(index),
            github: Some(GithubLink {
                owner: if is_account {
                    "@me".to_owned()
                } else {
                    owner.login.clone()
                },
                project: owner.projects.first().map(|project| project.number),
                ..GithubLink::owner(String::new())
            }),
            todoist: section.map(|section| TodoistLink {
                section: section.clone(),
                project: None,
            }),
            roadmap: None,
            pinned: false,
            muted: false,
        });
    }
    for section in unpaired {
        projects.push(Project {
            id: slug(section),
            name: section.clone(),
            color: color(projects.len()),
            github: None,
            todoist: Some(TodoistLink {
                section: section.clone(),
                project: None,
            }),
            roadmap: None,
            pinned: false,
            muted: false,
        });
    }
    Config {
        defaults: Defaults {
            todoist_parent: parent.map(str::to_owned),
            ..Defaults::default()
        },
        notifications: Notifications::default(),
        projects,
        ignored_suggestions: Vec::new(),
    }
}

/// Something the settings offer to do to the configuration, which the person applies or ignores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Stable from one refresh to the next, so that an ignored suggestion stays ignored.
    pub key: String,
    pub title: String,
    pub detail: String,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add(Project),
    /// Replaces the project with this identifier.
    Replace(String, Project),
}

impl Change {
    pub fn apply(&self, config: &mut Config) {
        match self {
            Self::Add(project) => config.projects.push(project.clone()),
            Self::Replace(id, project) => {
                if let Some(known) = config.projects.iter_mut().find(|known| &known.id == id) {
                    *known = project.clone();
                }
            }
        }
    }
}

/// What the accounts can see that the configuration does not use yet.
struct Context<'a> {
    config: &'a Config,
    owners: &'a [Owner],
    /// The sections of the parent Todoist project that no project uses.
    free_sections: Vec<String>,
}

impl Context<'_> {
    fn me(&self) -> &str {
        self.owners.first().map_or("", |owner| owner.login.as_str())
    }

    fn login<'a>(&'a self, link: &'a GithubLink) -> &'a str {
        if link.owner == "@me" {
            self.me()
        } else {
            &link.owner
        }
    }

    fn owner(&self, login: &str) -> Option<&Owner> {
        self.owners
            .iter()
            .find(|owner| owner.login.eq_ignore_ascii_case(login))
    }

    /// How the configuration names an owner: `@me` for the signed-in account.
    fn owner_value(&self, login: &str) -> String {
        if login.eq_ignore_ascii_case(self.me()) {
            "@me".to_owned()
        } else {
            login.to_owned()
        }
    }

    fn has_owner_project(&self, login: &str) -> bool {
        self.config.projects.iter().any(|project| {
            project
                .github
                .as_ref()
                .is_some_and(|link| self.login(link).eq_ignore_ascii_case(login))
        })
    }

    fn has_repo_project(&self, repo: &str) -> bool {
        self.config.projects.iter().any(|project| {
            project.github.as_ref().is_some_and(|link| {
                link.repos
                    .iter()
                    .any(|listed| listed.eq_ignore_ascii_case(repo))
            })
        })
    }

    /// A free section named like one of `names`.
    fn free_section(&self, names: &[&str]) -> Option<String> {
        let wanted: Vec<String> = names.iter().map(|name| normalize_name(name)).collect();
        self.free_sections
            .iter()
            .find(|section| wanted.contains(&normalize_name(section)))
            .cloned()
    }

    fn new_project(
        &self,
        name: &str,
        github: Option<GithubLink>,
        section: Option<String>,
    ) -> Project {
        let base = slug(name);
        let mut id = base.clone();
        let mut counter = 2;
        while self.config.projects.iter().any(|project| project.id == id) {
            id = format!("{base}-{counter}");
            counter += 1;
        }
        Project {
            id,
            name: name.to_owned(),
            color: color(self.config.projects.len()),
            github,
            todoist: section.map(|section| TodoistLink {
                section,
                project: None,
            }),
            roadmap: None,
            pinned: false,
            muted: false,
        }
    }
}

/// The repository (`owner/name`) of a pull request or an issue, read from its identifier.
fn repo_of(item: &Item) -> Option<&str> {
    let rest = item
        .id
        .strip_prefix("gh-pr:")
        .or_else(|| item.id.strip_prefix("gh-issue:"))?;
    rest.split_once('#').map(|(repo, _)| repo)
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// Suggestions from what the accounts see and what lands in "Autres": projects to create, and
/// links to add to existing projects. The ignored ones are left out.
#[must_use]
pub fn suggestions(
    config: &Config,
    owners: &[Owner],
    todoist: &[TodoistChoice],
    items: &[Item],
) -> Vec<Suggestion> {
    let used: Vec<String> = config
        .projects
        .iter()
        .filter_map(|project| project.todoist.as_ref())
        .map(|link| normalize_name(&link.section))
        .collect();
    let parent = config.defaults.todoist_parent.as_deref();
    let free_sections = parent
        .and_then(|parent| {
            todoist
                .iter()
                .find(|project| normalize_name(&project.name) == normalize_name(parent))
        })
        .map(|project| {
            project
                .sections
                .iter()
                .filter(|section| !used.contains(&normalize_name(section)))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let context = Context {
        config,
        owners,
        free_sections,
    };
    let mut found = Vec::new();
    complete_projects(&context, &mut found);
    new_projects(&context, items, &mut found);
    let mut seen = std::collections::HashSet::new();
    found.retain(|suggestion: &Suggestion| {
        seen.insert(suggestion.key.clone()) && !config.ignored_suggestions.contains(&suggestion.key)
    });
    found
}

/// Links that existing projects lack: a section, a roadmap, an owner.
fn complete_projects(context: &Context<'_>, found: &mut Vec<Suggestion>) {
    for project in &context.config.projects {
        match (&project.github, &project.todoist) {
            (Some(link), None) => {
                let login = context.login(link);
                let mut names = vec![project.name.as_str(), login];
                names.extend(
                    link.repos
                        .iter()
                        .filter_map(|repo| repo.split_once('/').map(|(_, name)| name)),
                );
                if let Some(section) = context.free_section(&names) {
                    let mut changed = project.clone();
                    changed.todoist = Some(TodoistLink {
                        section: section.clone(),
                        project: None,
                    });
                    found.push(Suggestion {
                        key: format!("section:{}:{}", project.id, normalize_name(&section)),
                        title: format!(
                            "Ajouter la section Todoist « {section} » à « {} »",
                            project.name
                        ),
                        detail: "Ses tâches rejoindront ce projet.".to_owned(),
                        change: Change::Replace(project.id.clone(), changed),
                    });
                }
            }
            (None, Some(section)) => {
                let wanted = [
                    normalize_name(&project.name),
                    normalize_name(&section.section),
                ];
                let owner = context
                    .owners
                    .iter()
                    .find(|owner| wanted.contains(&normalize_name(&owner.login)));
                let repo = context
                    .owners
                    .iter()
                    .flat_map(|owner| &owner.repos)
                    .find(|repo| {
                        repo.split_once('/')
                            .is_some_and(|(_, name)| wanted.contains(&normalize_name(name)))
                    });
                let (target, link) = match (owner, repo) {
                    (Some(owner), _) => (
                        owner.login.clone(),
                        GithubLink::owner(context.owner_value(&owner.login)),
                    ),
                    (None, Some(repo)) => {
                        let login = repo.split_once('/').map_or("", |(login, _)| login);
                        let mut link = GithubLink::owner(context.owner_value(login));
                        link.repos = vec![repo.clone()];
                        (repo.clone(), link)
                    }
                    (None, None) => continue,
                };
                let mut changed = project.clone();
                changed.github = Some(link);
                found.push(Suggestion {
                    key: format!("github:{}:{}", project.id, target.to_lowercase()),
                    title: format!("Relier « {} » à GitHub {target}", project.name),
                    detail: "Ses PR et ses issues rejoindront ce projet.".to_owned(),
                    change: Change::Replace(project.id.clone(), changed),
                });
            }
            _ => {}
        }
        if let Some(link) = &project.github
            && link.project.is_none()
            && link.repos.is_empty()
            && let Some(roadmap) = context
                .owner(context.login(link))
                .and_then(|owner| owner.projects.first())
        {
            let mut changed = project.clone();
            if let Some(changed_link) = &mut changed.github {
                changed_link.project = Some(roadmap.number);
            }
            found.push(Suggestion {
                key: format!("roadmap:{}:{}", project.id, roadmap.number),
                title: format!(
                    "Ajouter la roadmap « {} » (#{}) à « {} »",
                    roadmap.title, roadmap.number, project.name
                ),
                detail: format!(
                    "Le GitHub Project le plus récent de {}.",
                    context.login(link)
                ),
                change: Change::Replace(project.id.clone(), changed),
            });
        }
    }
}

/// Projects for what waits in "Autres", and for the sections no project uses.
fn new_projects(context: &Context<'_>, items: &[Item], found: &mut Vec<Suggestion>) {
    let others = || items.iter().filter(|item| item.project == OTHERS);
    let mut by_owner: BTreeMap<String, usize> = BTreeMap::new();
    for repo in others().filter_map(repo_of) {
        if let Some((login, _)) = repo.split_once('/') {
            *by_owner.entry(login.to_lowercase()).or_default() += 1;
        }
    }
    let mut by_owner: Vec<(String, usize)> = by_owner.into_iter().collect();
    by_owner.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (login, count) in by_owner {
        if context.has_owner_project(&login) {
            continue;
        }
        let login = context
            .owner(&login)
            .map_or(login.clone(), |owner| owner.login.clone());
        // As in the proposal, the signed-in account is "Perso".
        let name = if login.eq_ignore_ascii_case(context.me()) {
            "Perso".to_owned()
        } else {
            login.clone()
        };
        let section = context.free_section(&[&login, &name]);
        let project = context.new_project(
            &name,
            Some(GithubLink::owner(context.owner_value(&login))),
            section,
        );
        found.push(Suggestion {
            key: format!("new:owner:{}", login.to_lowercase()),
            title: format!("Créer le projet « {name} »"),
            detail: format!(
                "{} de {login} {} dans Autres.",
                plural(count, "élément", "éléments"),
                if count == 1 { "est" } else { "sont" }
            ),
            change: Change::Add(project),
        });
    }
    for section in &context.free_sections {
        let wanted = normalize_name(section);
        let owner = context
            .owners
            .iter()
            .find(|owner| normalize_name(&owner.login) == wanted);
        let repo = context
            .owners
            .iter()
            .flat_map(|owner| &owner.repos)
            .find(|repo| {
                repo.split_once('/')
                    .is_some_and(|(_, name)| normalize_name(name) == wanted)
            });
        let waiting = others()
            .filter(|item| item.id.starts_with("td:") && normalize_name(&item.context) == wanted)
            .count();
        let (key, link, detail) = match (owner, repo) {
            (Some(owner), _) if !context.has_owner_project(&owner.login) => (
                format!("new:owner:{}", owner.login.to_lowercase()),
                Some(GithubLink::owner(context.owner_value(&owner.login))),
                format!(
                    "La section Todoist « {section} » porte le nom de {}.",
                    owner.login
                ),
            ),
            (None, Some(repo)) if !context.has_repo_project(repo) => {
                let login = repo.split_once('/').map_or("", |(login, _)| login);
                let mut link = GithubLink::owner(context.owner_value(login));
                link.repos = vec![repo.clone()];
                (
                    format!("new:repo:{}", repo.to_lowercase()),
                    Some(link),
                    format!("La section Todoist « {section} » porte le nom du dépôt {repo}."),
                )
            }
            _ if waiting > 0 => (
                format!("new:section:{wanted}"),
                None,
                format!(
                    "{} de cette section {} dans Autres.",
                    plural(waiting, "tâche", "tâches"),
                    if waiting == 1 { "est" } else { "sont" }
                ),
            ),
            _ => continue,
        };
        found.push(Suggestion {
            key,
            title: format!("Créer le projet « {section} »"),
            detail,
            change: Change::Add(context.new_project(section, link, Some(section.clone()))),
        });
    }
}

/// The names of the sections of a Todoist project, as a sync lists them.
#[must_use]
pub fn sections_of(projects: &[Value], sections: &[Value], parent: &str) -> Option<Vec<String>> {
    let wanted = normalize_name(parent);
    let parent_id = projects
        .iter()
        .find(|project| {
            project["name"]
                .as_str()
                .is_some_and(|name| normalize_name(name) == wanted)
        })
        .and_then(|project| project["id"].as_str())?;
    Some(
        sections
            .iter()
            .filter(|section| section["project_id"].as_str() == Some(parent_id))
            .filter_map(|section| section["name"].as_str().map(str::to_owned))
            .collect(),
    )
}

/// The color of the project at `index`, cycling through the GNOME palette.
#[must_use]
pub fn color(index: usize) -> String {
    PALETTE[index % PALETTE.len()].to_owned()
}

/// A stable identifier for a project name: lower case, without accents, words joined by dashes.
#[must_use]
pub fn slug(name: &str) -> String {
    let lower = name.to_lowercase();
    let parts: Vec<String> = lower
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(normalize_name)
        .collect();
    parts.join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::github::OwnerProject;

    fn owner(login: &str, projects: &[u32]) -> Owner {
        Owner {
            login: login.to_owned(),
            repos: Vec::new(),
            projects: projects
                .iter()
                .map(|number| OwnerProject {
                    number: *number,
                    title: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn owners_and_sections_pair_by_name() {
        let owners = [
            owner("baxyz", &[3]),
            owner("helpers4", &[7, 1]),
            owner("sunny-tech", &[]),
        ];
        let sections = vec![
            "Perso".to_owned(),
            "Sunny Tech".to_owned(),
            "Helpers4".to_owned(),
            "Admin".to_owned(),
        ];
        let config = propose(&owners, &sections, Some("Dev"));

        assert_eq!(config.defaults.todoist_parent.as_deref(), Some("Dev"));
        let ids: Vec<_> = config
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect();
        assert_eq!(ids, ["perso", "helpers4", "sunny-tech", "admin"]);

        let perso = &config.projects[0];
        assert_eq!(perso.github.as_ref().unwrap().owner, "@me");
        assert_eq!(perso.github.as_ref().unwrap().project, Some(3));
        assert_eq!(perso.todoist.as_ref().unwrap().section, "Perso");

        let helpers4 = &config.projects[1];
        assert_eq!(helpers4.github.as_ref().unwrap().project, Some(7));
        assert_eq!(helpers4.todoist.as_ref().unwrap().section, "Helpers4");

        assert_eq!(config.projects[2].github.as_ref().unwrap().project, None);
        assert!(config.projects[3].github.is_none());
    }

    #[test]
    fn a_proposal_is_a_valid_configuration() {
        let config = propose(&[owner("me", &[])], &[], None);
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(Config::parse(&text, "test").unwrap(), config);
    }

    fn waiting(id: &str, context: &str) -> Item {
        use crate::item::{Kind, Priority, State};
        use crate::source::Provenance;
        Item {
            id: id.to_owned(),
            project: OTHERS.to_owned(),
            kind: Kind::Todo,
            title: id.to_owned(),
            context: context.to_owned(),
            url: String::new(),
            priority: Priority::P4,
            due: None,
            deadline: None,
            state: State::Planned,
            updated_at: chrono::DateTime::default(),
            requested_at: None,
            bot: false,
            snoozed_until: None,
            details: None,
            draft: false,
            provenance: Provenance {
                source: "test",
                id: id.to_owned(),
                version: None,
            },
        }
    }

    #[test]
    fn suggestions_complete_projects_and_gather_what_waits_in_others() {
        let mut config = Config::parse(
            r#"
            [defaults]
            todoist_parent = "Dev"

            [[project]]
            id = "acme"
            name = "Acme"
            github = { owner = "acme" }

            [[project]]
            id = "notes"
            name = "Pigeonnier"
            todoist = { section = "pigeonnier" }
            "#,
            "test",
        )
        .unwrap();
        let mut me = owner("baxyz", &[]);
        me.repos = vec!["baxyz/pigeonnier".to_owned()];
        let mut acme = owner("acme", &[4]);
        acme.repos = vec!["acme/tool".to_owned()];
        let mut brig = owner("brig", &[]);
        brig.repos = vec!["brig/x".to_owned()];
        let owners = [me, acme, brig];
        let todoist = [TodoistChoice {
            name: "Dev".to_owned(),
            sections: ["acme", "pigeonnier", "Tool", "Courses", "Vide"]
                .map(str::to_owned)
                .to_vec(),
        }];
        let items = [
            waiting("gh-issue:brig/x#1", "x#1"),
            waiting("gh-pr:brig/x#2", "x#2"),
            waiting("td:9", "Courses"),
        ];

        let found = suggestions(&config, &owners, &todoist, &items);

        let keys: Vec<&str> = found
            .iter()
            .map(|suggestion| suggestion.key.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "section:acme:acme",
                "roadmap:acme:4",
                "github:notes:baxyz/pigeonnier",
                "new:owner:brig",
                "new:repo:acme/tool",
                "new:section:courses",
            ]
        );
        assert_eq!(found[3].detail, "2 éléments de brig sont dans Autres.");
        // The repository of the signed-in account is named as `@me`.
        let Change::Replace(_, linked) = &found[2].change else {
            panic!("a change of an existing project");
        };
        assert_eq!(linked.github.as_ref().unwrap().owner, "@me");
        assert_eq!(linked.github.as_ref().unwrap().repos, ["baxyz/pigeonnier"]);

        found[0].change.apply(&mut config);
        assert_eq!(config.projects[0].todoist.as_ref().unwrap().section, "acme");
        found[4].change.apply(&mut config);
        assert_eq!(config.projects[2].id, "tool");

        config.ignored_suggestions.push("new:owner:brig".to_owned());
        let left = suggestions(&config, &owners, &todoist, &items);
        assert!(
            left.iter()
                .all(|suggestion| suggestion.key != "new:owner:brig")
        );
        assert!(
            left.iter()
                .all(|suggestion| !suggestion.key.starts_with("section:acme"))
        );
    }
}
