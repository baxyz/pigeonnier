// SPDX-License-Identifier: AGPL-3.0-or-later

//! The settings page: the accounts, and the projects, chosen among what the accounts can see. The
//! editing rules are plain functions; the drawing is in [`App::draw_settings`].

use pigeonnier_core::config::{Config, GithubLink, Project, TodoistLink, normalize_name};
use pigeonnier_core::discover;
use pigeonnier_core::engine::Choices;
use pigeonnier_core::viewmodel::Snapshot;
use relm4::adw::prelude::*;
use relm4::{adw, gtk};

use crate::{App, Msg, escape};

/// The signed-in account, as the configuration names it.
const ME: &str = "@me";

/// The GitHub owners to choose from: `(value in the configuration, label)`.
#[must_use]
pub fn owner_options(choices: &Choices) -> Vec<(String, String)> {
    choices
        .owners
        .iter()
        .enumerate()
        .map(|(index, owner)| {
            if index == 0 {
                (ME.to_owned(), format!("{} (ton compte)", owner.login))
            } else {
                (owner.login.clone(), owner.login.clone())
            }
        })
        .collect()
}

/// The GitHub Projects of an owner: `(number, label)`.
#[must_use]
pub fn project_options(choices: &Choices, owner: &str) -> Vec<(u32, String)> {
    let found = if owner == ME {
        choices.owners.first()
    } else {
        choices
            .owners
            .iter()
            .find(|known| known.login.eq_ignore_ascii_case(owner))
    };
    found
        .map(|owner| {
            owner
                .projects
                .iter()
                .map(|project| {
                    (
                        project.number,
                        format!("{} (#{})", project.title, project.number),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every Todoist section: `(project, section)`.
#[must_use]
pub fn section_options(choices: &Choices) -> Vec<(String, String)> {
    choices
        .todoist
        .iter()
        .flat_map(|project| {
            project
                .sections
                .iter()
                .map(|section| (project.name.clone(), section.clone()))
        })
        .collect()
}

/// The Todoist project of a link, which may come from the defaults.
fn todoist_project<'a>(config: &'a Config, link: &'a TodoistLink) -> Option<&'a str> {
    link.project
        .as_deref()
        .or(config.defaults.todoist_parent.as_deref())
}

/// Where a configured section sits among the options, compared like the engine compares them.
#[must_use]
pub fn section_index(
    options: &[(String, String)],
    config: &Config,
    link: &TodoistLink,
) -> Option<usize> {
    let project = todoist_project(config, link);
    options.iter().position(|(name, section)| {
        normalize_name(section) == normalize_name(&link.section)
            && project.is_none_or(|project| normalize_name(project) == normalize_name(name))
    })
}

/// A proposal from what the accounts can see. `parent` is the Todoist project holding one section
/// per project, if there is one.
#[must_use]
pub fn propose(choices: &Choices, parent: Option<&str>) -> Config {
    let sections = parent
        .and_then(|parent| {
            choices
                .todoist
                .iter()
                .find(|project| project.name == parent)
        })
        .map(|project| project.sections.clone())
        .unwrap_or_default();
    discover::propose(&choices.owners, &sections, parent)
}

/// The Todoist project whose sections bear the most GitHub owner names: the likely parent, if any
/// section pairs at all.
#[must_use]
pub fn guess_parent(choices: &Choices) -> Option<String> {
    let owners: Vec<String> = choices
        .owners
        .iter()
        .map(|owner| normalize_name(&owner.login))
        .collect();
    choices
        .todoist
        .iter()
        .map(|project| {
            let pairs = project
                .sections
                .iter()
                .filter(|section| owners.contains(&normalize_name(section)))
                .count();
            (pairs, project)
        })
        .filter(|(pairs, _)| *pairs > 0)
        .max_by_key(|(pairs, _)| *pairs)
        .map(|(_, project)| project.name.clone())
}

/// A new, empty project.
#[must_use]
pub fn new_project(index: usize) -> Project {
    Project {
        id: String::new(),
        name: format!("Projet {}", index + 1),
        color: discover::color(index),
        github: None,
        todoist: None,
        roadmap: None,
        pinned: false,
        muted: false,
    }
}

/// Ready to save: unnamed projects are dropped, and each project gets a unique identifier.
#[must_use]
pub fn finalize(mut config: Config) -> Config {
    config
        .projects
        .retain(|project| !project.name.trim().is_empty());
    let mut taken: Vec<String> = Vec::new();
    for project in &mut config.projects {
        project.name = project.name.trim().to_owned();
        let base = if project.id.is_empty() {
            discover::slug(&project.name)
        } else {
            project.id.clone()
        };
        let base = if base.is_empty() {
            "projet".to_owned()
        } else {
            base
        };
        let mut id = base.clone();
        let mut counter = 2;
        while taken.contains(&id) {
            id = format!("{base}-{counter}");
            counter += 1;
        }
        taken.push(id.clone());
        project.id = id;
    }
    config
}

/// One line saying what a project gathers.
#[must_use]
pub fn summary(config: &Config, project: &Project) -> String {
    let mut parts = Vec::new();
    if let Some(link) = &project.github {
        let owner = if link.owner == ME {
            "ton compte GitHub".to_owned()
        } else {
            format!("GitHub {}", link.owner)
        };
        parts.push(match link.project {
            Some(number) => format!("{owner}, Project #{number}"),
            None => owner,
        });
    }
    if let Some(link) = &project.todoist {
        parts.push(match todoist_project(config, link) {
            Some(name) => format!("Todoist {name} / {}", link.section),
            None => format!("Todoist {}", link.section),
        });
    }
    if parts.is_empty() {
        "Rien de choisi".to_owned()
    } else {
        parts.join(" · ")
    }
}

/// What a change in the settings does to the draft.
#[derive(Debug)]
pub enum Edit {
    Name(String),
    Owner(Option<String>),
    GithubProject(Option<u32>),
    Section(Option<(String, String)>),
    Remove,
}

/// Applies an edit to project `index`. Returns whether the page must be drawn again.
pub fn apply(config: &mut Config, index: usize, edit: Edit) -> bool {
    if matches!(edit, Edit::Remove) {
        if index < config.projects.len() {
            config.projects.remove(index);
        }
        return true;
    }
    let Some(project) = config.projects.get_mut(index) else {
        return false;
    };
    match edit {
        Edit::Name(name) => {
            project.name = name;
            false
        }
        Edit::Owner(owner) => {
            let unchanged = project.github.as_ref().map(|link| &link.owner) == owner.as_ref();
            if !unchanged {
                project.github = owner.map(|owner| GithubLink {
                    owner,
                    project: None,
                    repos: Vec::new(),
                });
            }
            !unchanged
        }
        Edit::GithubProject(number) => {
            if let Some(link) = &mut project.github {
                link.project = number;
            }
            false
        }
        Edit::Section(section) => {
            project.todoist = section.map(|(project, section)| TodoistLink {
                section,
                project: Some(project),
            });
            false
        }
        Edit::Remove => true,
    }
}

/// The greeting of the first run. Not a status page: one would scroll on its own inside the list.
fn welcome() -> gtk::Box {
    let welcome = gtk::Box::new(gtk::Orientation::Vertical, 12);
    welcome.set_margin_top(24);
    welcome.set_margin_bottom(12);
    let icon = gtk::Image::from_icon_name("io.github.baxyz.Pigeonnier");
    icon.set_pixel_size(96);
    welcome.append(&icon);
    let title = gtk::Label::new(Some("Bienvenue dans Pigeonnier"));
    title.add_css_class("title-1");
    welcome.append(&title);
    let text = gtk::Label::new(Some(
        "Connecte tes comptes, puis choisis tes projets : Pigeonnier rassemble ensuite ce qui t'attend, projet par projet.",
    ));
    text.set_wrap(true);
    text.set_justify(gtk::Justification::Center);
    welcome.append(&text);
    welcome
}

fn combo(title: &str, labels: &[String], selected: usize) -> adw::ComboRow {
    let row = adw::ComboRow::new();
    row.set_title(title);
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    row.set_model(Some(&gtk::StringList::new(&labels)));
    row.set_selected(u32::try_from(selected).unwrap_or(0));
    row
}

fn selected(row: &adw::ComboRow) -> usize {
    usize::try_from(row.selected()).unwrap_or(0)
}

impl App {
    /// A drop-down of `options` after a "none" entry; picking one edits project `index`.
    fn choice_row<T: Clone + 'static>(
        &self,
        (title, none): (&str, &str),
        options: Vec<(T, String)>,
        current: Option<usize>,
        index: usize,
        edit: fn(Option<T>) -> Edit,
    ) -> adw::ComboRow {
        let mut labels = vec![none.to_owned()];
        labels.extend(options.iter().map(|(_, label)| label.clone()));
        let row = combo(title, &labels, current.map_or(0, |position| position + 1));
        let values: Vec<T> = options.into_iter().map(|(value, _)| value).collect();
        let sender = self.sender.clone();
        row.connect_selected_notify(move |row| {
            let value = selected(row)
                .checked_sub(1)
                .and_then(|position| values.get(position).cloned());
            sender.input(Msg::EditProject(index, edit(value)));
        });
        row
    }

    /// The first thing a new user sees, and the settings later.
    pub(crate) fn draw_settings(&self, snapshot: &Snapshot) {
        if snapshot.needs_setup {
            self.list.append(&welcome());
        }
        self.draw_accounts(snapshot);
        let proposal = self.draw_projects_editor(snapshot);

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        footer.set_halign(gtk::Align::End);
        if !snapshot.needs_setup {
            let cancel = gtk::Button::with_label("Annuler");
            let sender = self.sender.clone();
            cancel.connect_clicked(move |_| sender.input(Msg::HideSettings));
            footer.append(&cancel);
        }
        let save = gtk::Button::with_label("Enregistrer");
        save.add_css_class("suggested-action");
        save.set_sensitive(
            self.draft
                .as_ref()
                .is_some_and(|draft| !draft.projects.is_empty()),
        );
        let sender = self.sender.clone();
        save.connect_clicked(move |_| sender.input(Msg::SaveProjects));
        footer.append(&save);
        self.list.append(&footer);
        if let Some(proposal) = proposal {
            self.list.append(&proposal);
        }
    }

    fn draw_accounts(&self, snapshot: &Snapshot) {
        let group = adw::PreferencesGroup::new();
        group.set_title("Comptes");
        group.set_description(Some(
            "Pigeonnier lit tes PR et tes tâches. Les jetons restent dans le trousseau de cet ordinateur.",
        ));
        let add = |name: &str, connected: bool, connect: Msg, service: &'static str| {
            let row = adw::ActionRow::new();
            row.set_title(name);
            row.set_subtitle(if connected {
                "Connecté"
            } else {
                "Non connecté"
            });
            let button = gtk::Button::with_label(if connected {
                "Déconnecter"
            } else {
                "Connecter"
            });
            button.set_valign(gtk::Align::Center);
            if !connected {
                button.add_css_class("suggested-action");
            }
            let sender = self.sender.clone();
            let connect = std::cell::RefCell::new(Some(connect));
            button.connect_clicked(move |_| {
                if connected {
                    sender.input(Msg::Disconnect(service));
                } else if let Some(message) = connect.borrow_mut().take() {
                    sender.input(message);
                }
            });
            row.add_suffix(&button);
            group.add(&row);
        };
        add(
            "GitHub",
            snapshot.github_connected,
            Msg::ConnectGithub,
            "github",
        );
        add(
            "Todoist",
            snapshot.todoist_connected,
            Msg::ConnectTodoist,
            "todoist",
        );
        self.list.append(&group);
    }

    /// Draws the projects, and returns the proposal group to place after the buttons.
    fn draw_projects_editor(&self, snapshot: &Snapshot) -> Option<adw::PreferencesGroup> {
        let group = adw::PreferencesGroup::new();
        group.set_title("Projets");
        group.set_description(Some(
            "Un projet rassemble un compte ou une org GitHub, éventuellement son GitHub Project, et une section Todoist.",
        ));
        self.list.append(&group);
        if !snapshot.github_connected && !snapshot.todoist_connected {
            let row = adw::ActionRow::new();
            row.set_title("Connecte au moins un compte pour choisir tes projets");
            group.add(&row);
            return None;
        }
        if let Some(error) = &self.choices_error {
            let row = adw::ActionRow::new();
            row.set_title("Impossible de lire tes comptes");
            row.set_subtitle(&escape(error));
            let retry = gtk::Button::with_label("Réessayer");
            retry.set_valign(gtk::Align::Center);
            let sender = self.sender.clone();
            retry.connect_clicked(move |_| sender.input(Msg::LoadChoices));
            row.add_suffix(&retry);
            group.add(&row);
            return None;
        }
        let (Some(draft), Some(choices)) = (&self.draft, &self.choices) else {
            let row = adw::ActionRow::new();
            row.set_title("Lecture de tes comptes…");
            row.add_prefix(&gtk::Spinner::builder().spinning(true).build());
            group.add(&row);
            return None;
        };

        for (index, project) in draft.projects.iter().enumerate() {
            group.add(&self.project_editor(draft, choices, index, project));
        }

        let add = adw::ButtonRow::new();
        add.set_title("Ajouter un projet");
        add.set_start_icon_name(Some("list-add-symbolic"));
        let sender = self.sender.clone();
        add.connect_activated(move |_| sender.input(Msg::AddProject));
        group.add(&add);
        Some(self.proposal_group(choices, draft))
    }

    fn proposal_group(&self, choices: &Choices, draft: &Config) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("Proposition automatique");
        group.set_description(Some(
            "Crée un projet par compte et par org GitHub, apparié à la section Todoist du même nom. Remplace la liste ci-dessus.",
        ));
        let mut labels = vec!["Aucun".to_owned()];
        labels.extend(choices.todoist.iter().map(|project| project.name.clone()));
        let current = draft
            .defaults
            .todoist_parent
            .as_ref()
            .and_then(|parent| {
                choices
                    .todoist
                    .iter()
                    .position(|project| &project.name == parent)
            })
            .map_or(0, |position| position + 1);
        let parent = combo(
            "Projet Todoist qui contient une section par projet",
            &labels,
            current,
        );
        group.add(&parent);
        let propose = adw::ButtonRow::new();
        propose.set_title("Proposer mes projets");
        propose.set_start_icon_name(Some("view-refresh-symbolic"));
        let sender = self.sender.clone();
        propose.connect_activated(move |_| {
            let index = selected(&parent);
            let name = (index > 0).then(|| labels[index].clone());
            sender.input(Msg::Propose(name));
        });
        group.add(&propose);
        group
    }

    fn project_editor(
        &self,
        draft: &Config,
        choices: &Choices,
        index: usize,
        project: &Project,
    ) -> adw::ExpanderRow {
        let expander = adw::ExpanderRow::new();
        expander.set_title(&escape(&project.name));
        expander.set_subtitle(&escape(&summary(draft, project)));
        expander.add_prefix(&crate::dot(&project.color));
        expander.set_expanded(self.expanded == Some(index));
        let sender = self.sender.clone();
        expander.connect_expanded_notify(move |row| {
            sender.input(Msg::Expanded(index, row.is_expanded()));
        });

        let name = adw::EntryRow::new();
        name.set_title("Nom");
        name.set_text(&project.name);
        let sender = self.sender.clone();
        name.connect_changed(move |entry| {
            sender.input(Msg::EditProject(
                index,
                Edit::Name(entry.text().to_string()),
            ));
        });
        expander.add_row(&name);

        // Each choice starts with "none"; a configured value the accounts no longer show stays listed.
        let mut owners = owner_options(choices);
        let current_owner = project.github.as_ref().map(|link| link.owner.clone());
        if let Some(owner) = &current_owner
            && !owners
                .iter()
                .any(|(value, _)| value.eq_ignore_ascii_case(owner))
        {
            owners.push((owner.clone(), owner.clone()));
        }
        let current = current_owner.as_ref().and_then(|owner| {
            owners
                .iter()
                .position(|(value, _)| value.eq_ignore_ascii_case(owner))
        });
        expander.add_row(&self.choice_row(
            ("Compte ou org GitHub", "Aucun"),
            owners,
            current,
            index,
            Edit::Owner,
        ));

        if let Some(link) = &project.github {
            let mut projects = project_options(choices, &link.owner);
            if let Some(number) = link.project
                && !projects.iter().any(|(known, _)| *known == number)
            {
                projects.push((number, format!("#{number}")));
            }
            let current = link
                .project
                .and_then(|number| projects.iter().position(|(known, _)| *known == number));
            expander.add_row(&self.choice_row(
                ("GitHub Project (roadmap)", "Aucun (pas de roadmap)"),
                projects,
                current,
                index,
                Edit::GithubProject,
            ));
        }

        let mut sections: Vec<((String, String), String)> = section_options(choices)
            .into_iter()
            .map(|(project, section)| {
                let label = format!("{project} / {section}");
                ((project, section), label)
            })
            .collect();
        let plain: Vec<(String, String)> =
            sections.iter().map(|(value, _)| value.clone()).collect();
        let mut current = project
            .todoist
            .as_ref()
            .and_then(|link| section_index(&plain, draft, link));
        if let Some(link) = &project.todoist
            && current.is_none()
        {
            let name = todoist_project(draft, link).unwrap_or_default().to_owned();
            let label = format!("{name} / {}", link.section);
            sections.push(((name, link.section.clone()), label));
            current = Some(sections.len() - 1);
        }
        expander.add_row(&self.choice_row(
            ("Section Todoist", "Aucune"),
            sections,
            current,
            index,
            Edit::Section,
        ));

        let remove = adw::ButtonRow::new();
        remove.set_title("Retirer ce projet");
        remove.add_css_class("destructive-action");
        let sender = self.sender.clone();
        remove.connect_activated(move |_| sender.input(Msg::EditProject(index, Edit::Remove)));
        expander.add_row(&remove);

        expander
    }
}

#[cfg(test)]
mod tests {
    use pigeonnier_core::engine::TodoistChoice;
    use pigeonnier_core::source::github::{Owner, OwnerProject};

    use super::*;

    fn choices() -> Choices {
        Choices {
            owners: vec![
                Owner {
                    login: "baxyz".to_owned(),
                    projects: vec![OwnerProject {
                        number: 3,
                        title: "Perso".to_owned(),
                    }],
                },
                Owner {
                    login: "helpers4".to_owned(),
                    projects: Vec::new(),
                },
            ],
            todoist: vec![TodoistChoice {
                name: "Dev".to_owned(),
                sections: vec!["Helpers4".to_owned(), "Sunny Tech".to_owned()],
            }],
        }
    }

    #[test]
    fn the_account_is_offered_as_me() {
        let options = owner_options(&choices());
        assert_eq!(options[0].0, "@me");
        assert_eq!(options[1].0, "helpers4");
        assert_eq!(project_options(&choices(), "@me")[0].0, 3);
        assert_eq!(project_options(&choices(), "HELPERS4"), Vec::new());
    }

    #[test]
    fn a_proposal_pairs_sections_of_the_chosen_parent() {
        let config = propose(&choices(), Some("Dev"));
        let helpers4 = config
            .projects
            .iter()
            .find(|project| project.name == "helpers4")
            .unwrap();
        assert_eq!(helpers4.todoist.as_ref().unwrap().section, "Helpers4");
        assert!(
            config
                .projects
                .iter()
                .any(|project| project.name == "Sunny Tech")
        );
        let link = helpers4.todoist.as_ref().unwrap();
        assert_eq!(
            section_index(&section_options(&choices()), &config, link),
            Some(0)
        );
    }

    #[test]
    fn the_parent_is_the_project_whose_sections_match_owners() {
        assert_eq!(guess_parent(&choices()).as_deref(), Some("Dev"));
        let mut lonely = choices();
        lonely.todoist[0].sections = vec!["Courses".to_owned()];
        assert_eq!(guess_parent(&lonely), None);
    }

    #[test]
    fn changing_the_owner_forgets_the_project_number() {
        let mut config = propose(&choices(), None);
        assert_eq!(config.projects[0].github.as_ref().unwrap().project, Some(3));
        assert!(apply(
            &mut config,
            0,
            Edit::Owner(Some("helpers4".to_owned()))
        ));
        assert_eq!(config.projects[0].github.as_ref().unwrap().project, None);
        assert!(!apply(
            &mut config,
            0,
            Edit::Owner(Some("helpers4".to_owned()))
        ));
    }

    #[test]
    fn saving_names_every_project_once() {
        let mut config = Config::default();
        config.projects.push(new_project(0));
        config.projects.push(new_project(0));
        config.projects.push(Project {
            name: "  ".to_owned(),
            ..new_project(2)
        });
        let config = finalize(config);
        let ids: Vec<&str> = config
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect();
        assert_eq!(ids, ["projet-1", "projet-1-2"]);
    }
}
