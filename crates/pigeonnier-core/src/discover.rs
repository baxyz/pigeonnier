// SPDX-License-Identifier: AGPL-3.0-or-later

//! The first run: one owner, one project and one section make a Pigeonnier project, and the
//! convention lets it be proposed without asking anything.

use crate::config::{
    Config, Defaults, GithubLink, Notifications, Project, TodoistLink, normalize_name,
};
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
                repos: Vec::new(),
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
    }
}

fn color(index: usize) -> String {
    PALETTE[index % PALETTE.len()].to_owned()
}

fn slug(name: &str) -> String {
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
}
