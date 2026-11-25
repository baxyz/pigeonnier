// SPDX-License-Identifier: AGPL-3.0-or-later

//! The settings page: the accounts, and the projects, chosen among what the accounts can see. The
//! editing rules are plain functions; the drawing is in [`App::draw_settings`].

use pigeonnier_core::config::{
    Config, Day, GithubLink, IssueScope, Project, TodoistLink, TodoistOthers, normalize_name,
};
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

/// The repositories of an owner, as `owner/name`, most recently pushed first.
#[must_use]
pub fn repo_options(choices: &Choices, owner: &str) -> Vec<(String, String)> {
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
                .repos
                .iter()
                .map(|repo| {
                    let name = repo.split_once('/').map_or(repo.as_str(), |(_, name)| name);
                    (repo.clone(), name.to_owned())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The choices of issues, in the order they are offered.
pub const ISSUE_SCOPES: [IssueScope; 3] = [IssueScope::None, IssueScope::Assigned, IssueScope::All];

#[must_use]
pub fn issue_label(scope: IssueScope) -> &'static str {
    match scope {
        IssueScope::None => "Aucune",
        IssueScope::Assigned => "Celles qui me sont assignées",
        IssueScope::All => "Toutes les ouvertes",
    }
}

/// What a project can list of the issues: `(value, label)`, after the default choice.
#[must_use]
pub fn issue_options() -> Vec<(IssueScope, String)> {
    ISSUE_SCOPES
        .iter()
        .map(|scope| (*scope, issue_label(*scope).to_owned()))
        .collect()
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
        days: Vec::new(),
    }
}

/// Ready to save: unnamed projects are dropped, and each project has a unique identifier.
#[must_use]
pub fn finalize(mut config: Config) -> Config {
    config
        .projects
        .retain(|project| !project.name.trim().is_empty());
    for project in &mut config.projects {
        project.name = project.name.trim().to_owned();
    }
    assign_ids(&mut config);
    config
}

/// Gives each project a unique identifier, keeping the ones it has.
pub fn assign_ids(config: &mut Config) {
    let mut taken: Vec<String> = Vec::new();
    for project in &mut config.projects {
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
        let mut github = match link.repos.as_slice() {
            [] => owner,
            [repo] => format!("GitHub {repo}"),
            repos => format!("GitHub {}", repos.join(", ")),
        };
        if let Some(number) = link.project {
            github.push_str(", Project #");
            github.push_str(&number.to_string());
        }
        github.push_str(match config.issue_scope(link) {
            IssueScope::None => ", sans issues",
            IssueScope::Assigned => ", mes issues",
            IssueScope::All => ", toutes les issues",
        });
        parts.push(github);
    }
    if let Some(link) = &project.todoist {
        parts.push(match todoist_project(config, link) {
            Some(name) => format!("Todoist {name} / {}", link.section),
            None => format!("Todoist {}", link.section),
        });
    }
    if config.defaults.by_day && !project.days.is_empty() {
        parts.push(crate::week::days_text(project));
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
    /// One repository of the owner, or `None` for all of them.
    Repo(Option<String>),
    /// `None` follows the default.
    Issues(Option<IssueScope>),
    GithubProject(Option<u32>),
    Section(Option<(String, String)>),
    /// One place up (`-1`) or down (`1`) in the list, which is the order everywhere.
    Move(isize),
    /// To that place in the list, dragged there.
    MoveTo(usize),
    /// Worked on that day, or not.
    Day(Day, bool),
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
            true
        }
        Edit::Owner(owner) => {
            let unchanged = project.github.as_ref().map(|link| &link.owner) == owner.as_ref();
            if !unchanged {
                project.github = owner.map(GithubLink::owner);
            }
            !unchanged
        }
        Edit::Repo(repo) => {
            if let Some(link) = &mut project.github {
                link.repos = repo.into_iter().collect();
            }
            false
        }
        Edit::Issues(scope) => {
            if let Some(link) = &mut project.github {
                link.issues = scope;
            }
            false
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
        Edit::Move(step) => {
            let target = index.checked_add_signed(step);
            match target {
                Some(target) if target < config.projects.len() => {
                    config.projects.swap(index, target);
                    true
                }
                _ => false,
            }
        }
        Edit::MoveTo(target) => {
            if target == index || target >= config.projects.len() {
                return false;
            }
            let project = config.projects.remove(index);
            config.projects.insert(target, project);
            true
        }
        Edit::Day(day, on) => {
            project.days.retain(|known| *known != day);
            if on {
                project.days.push(day);
                project.days.sort();
            }
            true
        }
        Edit::Remove => true,
    }
}

/// Where the project at `place` ends up once the one at `from` was moved to `to`.
#[must_use]
pub fn moved_to(place: usize, from: usize, to: usize) -> usize {
    if place == from {
        return to;
    }
    let without = if place > from { place - 1 } else { place };
    if without >= to { without + 1 } else { without }
}

/// Puts the projects in the order of their names, accents and case aside.
pub fn sort_projects(config: &mut Config) {
    config
        .projects
        .sort_by_cached_key(|project| normalize_name(&project.name));
}

/// A change on the "Application" page.
#[derive(Debug)]
pub enum DefaultsEdit {
    Issues(IssueScope),
    IncludeAssigned(bool),
    DirectReviewsOnly(bool),
    EscalationDays(u32),
    Refresh(&'static str),
    ClaudeLabel(String),
    TodoistOthers(TodoistOthers),
    ByDay(bool),
}

/// How often to refresh: `(value in the configuration, label)`.
pub const REFRESH_CHOICES: [(&str, &str); 5] = [
    ("2m", "2 minutes"),
    ("5m", "5 minutes"),
    ("10m", "10 minutes"),
    ("15m", "15 minutes"),
    ("30m", "30 minutes"),
];

pub fn apply_defaults(config: &mut Config, edit: DefaultsEdit) {
    let defaults = &mut config.defaults;
    match edit {
        DefaultsEdit::Issues(scope) => defaults.issues = scope,
        DefaultsEdit::IncludeAssigned(on) => defaults.include_assigned = on,
        DefaultsEdit::DirectReviewsOnly(on) => defaults.direct_reviews_only = on,
        DefaultsEdit::EscalationDays(days) => defaults.review_escalation_days = days,
        DefaultsEdit::Refresh(every) => every.clone_into(&mut defaults.refresh),
        DefaultsEdit::ClaudeLabel(label) => label.trim().clone_into(&mut defaults.claude_label),
        DefaultsEdit::TodoistOthers(others) => defaults.todoist_others = others,
        DefaultsEdit::ByDay(on) => defaults.by_day = on,
    }
}

/// The greeting of the first run, in the main window.
pub fn welcome(sender: &relm4::ComponentSender<App>, snapshot: &Snapshot) -> gtk::Box {
    let welcome = gtk::Box::new(gtk::Orientation::Vertical, 12);
    welcome.set_margin_top(48);
    welcome.set_valign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name("io.github.baxyz.Pigeonnier");
    icon.set_pixel_size(128);
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
    let start = gtk::Button::with_label("Configurer Pigeonnier");
    start.add_css_class("pill");
    start.add_css_class("suggested-action");
    start.set_halign(gtk::Align::Center);
    start.set_margin_top(12);
    let page = if snapshot.github_connected || snapshot.todoist_connected {
        PAGE_PROJECTS
    } else {
        PAGE_ACCOUNTS
    };
    let sender = sender.clone();
    start.connect_clicked(move |_| sender.input(Msg::ShowPreferences(Some(page))));
    welcome.append(&start);
    welcome
}

pub const PAGE_PROJECTS: &str = "projects";
pub const PAGE_ACCOUNTS: &str = "accounts";
pub const PAGE_APP: &str = "app";
pub const PAGE_WEEK: &str = "week";

/// The preferences dialog and what was put in its pages, to take out when it is drawn again.
pub struct Preferences {
    pub dialog: adw::PreferencesDialog,
    pages: [adw::PreferencesPage; 4],
    groups: Vec<(usize, adw::PreferencesGroup)>,
}

impl Preferences {
    fn new() -> Self {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_search_enabled(false);
        dialog.set_content_width(720);
        dialog.set_content_height(680);
        let page = |name: &str, title: &str, icon: &str| {
            let page = adw::PreferencesPage::new();
            page.set_name(Some(name));
            page.set_title(title);
            page.set_icon_name(Some(icon));
            dialog.add(&page);
            page
        };
        let pages = [
            page(PAGE_PROJECTS, "Projets", "view-grid-symbolic"),
            page(PAGE_WEEK, "Semaine", "x-office-calendar-symbolic"),
            page(PAGE_ACCOUNTS, "Comptes", "system-users-symbolic"),
            page(PAGE_APP, "Application", "preferences-system-symbolic"),
        ];
        Self {
            dialog,
            pages,
            groups: Vec::new(),
        }
    }

    fn add(&mut self, page: usize, group: adw::PreferencesGroup) {
        self.pages[page].add(&group);
        self.groups.push((page, group));
    }

    fn clear(&mut self) {
        for (page, group) in self.groups.drain(..) {
            self.pages[page].remove(&group);
        }
    }
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

fn note_row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);
    row
}

impl App {
    /// Opens the preferences, on `page` if given, and reads the accounts again in the background.
    pub(crate) fn open_preferences(&mut self, page: Option<&str>) {
        if self.preferences.is_none() {
            let preferences = Preferences::new();
            let sender = self.sender.clone();
            preferences
                .dialog
                .connect_closed(move |_| sender.input(Msg::PreferencesClosed));
            self.preferences = Some(preferences);
            self.draw_preferences();
            if let Some(preferences) = &self.preferences {
                preferences.dialog.present(Some(&self.window));
            }
        }
        if let (Some(preferences), Some(page)) = (&self.preferences, page) {
            preferences.dialog.set_visible_page_name(page);
        }
        self.sender.input(Msg::LoadPreferences);
    }

    /// Draws the three pages again from the configuration, the choices and the suggestions.
    pub(crate) fn draw_preferences(&mut self) {
        let Some(mut preferences) = self.preferences.take() else {
            return;
        };
        preferences.clear();
        for group in self.projects_page() {
            preferences.add(0, group);
        }
        for group in self.week_page() {
            preferences.add(1, group);
        }
        preferences.add(2, self.accounts_group());
        for group in self.app_page() {
            preferences.add(3, group);
        }
        self.preferences = Some(preferences);
    }

    /// Shows a message in the preferences, with a button that puts `previous` back.
    pub(crate) fn toast(&self, title: &str) {
        if let Some(preferences) = &self.preferences {
            preferences
                .dialog
                .add_toast(adw::Toast::new(&escape(title)));
        }
    }

    pub(crate) fn toast_undo(&self, title: &str, previous: Config) {
        let Some(preferences) = &self.preferences else {
            return;
        };
        let toast = adw::Toast::new(title);
        toast.set_button_label(Some("Annuler"));
        let sender = self.sender.clone();
        let previous = std::cell::RefCell::new(Some(previous));
        toast.connect_button_clicked(move |_| {
            if let Some(previous) = previous.borrow_mut().take() {
                sender.input(Msg::RestoreConfig(previous));
            }
        });
        preferences.dialog.add_toast(toast);
    }

    fn accounts_group(&self) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("Comptes");
        group.set_description(Some(
            "Pigeonnier lit tes PR et tes tâches. Les jetons restent dans le trousseau de cet ordinateur.",
        ));
        let (github, todoist) = self.snapshot.as_ref().map_or((false, false), |snapshot| {
            (snapshot.github_connected, snapshot.todoist_connected)
        });
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
        add("GitHub", github, Msg::ConnectGithub, "github");
        add("Todoist", todoist, Msg::ConnectTodoist, "todoist");
        group
    }

    /// The suggestions, the projects and the automatic proposal, or why they cannot be shown yet.
    fn projects_page(&self) -> Vec<adw::PreferencesGroup> {
        let group = adw::PreferencesGroup::new();
        group.set_title("Projets");
        group.set_description(Some(
            "Un projet rassemble un compte, une org ou un dépôt GitHub avec ses issues, éventuellement son GitHub Project, et une section Todoist. Glisse-les par leur poignée pour les ranger.",
        ));
        let connected = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.github_connected || snapshot.todoist_connected);
        if !connected {
            let row = note_row("Connecte au moins un compte pour choisir tes projets");
            let go = gtk::Button::with_label("Comptes");
            go.set_valign(gtk::Align::Center);
            let preferences = self.preferences.as_ref().map(|p| p.dialog.clone());
            go.connect_clicked(move |_| {
                if let Some(dialog) = &preferences {
                    dialog.set_visible_page_name(PAGE_ACCOUNTS);
                }
            });
            row.add_suffix(&go);
            group.add(&row);
            return vec![group];
        }
        let (Some(config), Some(choices)) = (&self.config, &self.choices) else {
            if let Some(error) = &self.choices_error {
                let row = note_row("Impossible de lire tes comptes");
                row.set_subtitle(&escape(error));
                let retry = gtk::Button::with_label("Réessayer");
                retry.set_valign(gtk::Align::Center);
                let sender = self.sender.clone();
                retry.connect_clicked(move |_| sender.input(Msg::LoadPreferences));
                row.add_suffix(&retry);
                group.add(&row);
            } else {
                let row = note_row("Lecture de tes comptes…");
                row.add_prefix(&gtk::Spinner::builder().spinning(true).build());
                group.add(&row);
            }
            return vec![group];
        };
        let mut groups = Vec::new();
        if !self.suggestions.is_empty() {
            groups.push(self.suggestions_group());
        }
        for (index, project) in config.projects.iter().enumerate() {
            group.add(&self.project_editor(config, choices, index, project));
        }
        // The handles order the projects; sorting them all by name is a small button aside.
        if config.projects.len() > 1 {
            let sort = gtk::Button::from_icon_name("view-sort-ascending-symbolic");
            sort.add_css_class("flat");
            sort.set_valign(gtk::Align::Center);
            sort.set_tooltip_text(Some("Trier par nom"));
            sort.update_property(&[gtk::accessible::Property::Label("Trier par nom")]);
            let sender = self.sender.clone();
            sort.connect_clicked(move |_| sender.input(Msg::SortProjects));
            group.set_header_suffix(Some(&sort));
        }
        let add = adw::ButtonRow::new();
        add.set_title("Ajouter un projet");
        add.set_start_icon_name(Some("list-add-symbolic"));
        let sender = self.sender.clone();
        add.connect_activated(move |_| sender.input(Msg::AddProject));
        group.add(&add);
        groups.push(group);
        if !choices.todoist.is_empty() {
            groups.push(self.todoist_others_group(choices, config));
        }
        groups.push(self.proposal_group(choices, config));
        groups
    }

    /// Which Todoist tasks outside the projects still come in, under "Autres".
    fn todoist_others_group(&self, choices: &Choices, config: &Config) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("Autres tâches Todoist");
        group.set_description(Some(
            "Les tâches des sections de tes projets viennent toujours. Les autres arrivent dans « Autres » seulement si tu le choisis ici.",
        ));
        let mut values = vec![TodoistOthers::None, TodoistOthers::All];
        values.extend(
            choices
                .todoist
                .iter()
                .map(|project| TodoistOthers::Project(project.name.clone())),
        );
        let current = &config.defaults.todoist_others;
        if !values.contains(current) {
            values.push(current.clone());
        }
        let labels: Vec<String> = values
            .iter()
            .map(|value| match value {
                TodoistOthers::None => "Aucune".to_owned(),
                TodoistOthers::All => "Toutes".to_owned(),
                TodoistOthers::Project(name) => format!("Celles de « {name} »"),
            })
            .collect();
        let selected_now = values
            .iter()
            .position(|value| value == current)
            .unwrap_or(0);
        let row = combo("Tâches hors de mes projets", &labels, selected_now);
        let sender = self.sender.clone();
        row.connect_selected_notify(move |row| {
            if let Some(value) = values.get(selected(row)) {
                sender.input(Msg::EditDefaults(DefaultsEdit::TodoistOthers(
                    value.clone(),
                )));
            }
        });
        group.add(&row);
        group
    }

    fn suggestions_group(&self) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("Suggestions");
        group.set_description(Some(
            "D'après ce que voient tes comptes et ce qui tombe dans « Autres ».",
        ));
        for (index, suggestion) in self.suggestions.iter().enumerate() {
            let row = adw::ActionRow::new();
            row.set_title(&escape(&suggestion.title));
            row.set_subtitle(&escape(&suggestion.detail));
            let ignore = gtk::Button::from_icon_name("window-close-symbolic");
            ignore.add_css_class("flat");
            ignore.set_valign(gtk::Align::Center);
            ignore.set_tooltip_text(Some("Ignorer cette suggestion"));
            let sender = self.sender.clone();
            let key = suggestion.key.clone();
            ignore.connect_clicked(move |_| sender.input(Msg::IgnoreSuggestion(key.clone())));
            let apply = gtk::Button::with_label("Appliquer");
            apply.set_valign(gtk::Align::Center);
            let sender = self.sender.clone();
            apply.connect_clicked(move |_| sender.input(Msg::ApplySuggestion(index)));
            row.add_suffix(&apply);
            row.add_suffix(&ignore);
            group.add(&row);
        }
        group
    }

    fn proposal_group(&self, choices: &Choices, config: &Config) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("Proposition automatique");
        group.set_description(Some(
            "Crée un projet par compte et par org GitHub, apparié à la section Todoist du même nom. Remplace la liste des projets.",
        ));
        let mut labels = vec!["Aucun".to_owned()];
        labels.extend(choices.todoist.iter().map(|project| project.name.clone()));
        let parent = config
            .defaults
            .todoist_parent
            .clone()
            .or_else(|| guess_parent(choices));
        let current = parent
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

    fn app_page(&self) -> Vec<adw::PreferencesGroup> {
        let Some(config) = &self.config else {
            let group = adw::PreferencesGroup::new();
            group.add(&note_row("Lecture des réglages…"));
            return vec![group];
        };
        let defaults = &config.defaults;
        let issues = adw::PreferencesGroup::new();
        issues.set_title("Issues GitHub");
        let labels: Vec<String> = ISSUE_SCOPES
            .iter()
            .map(|scope| issue_label(*scope).to_owned())
            .collect();
        let current = ISSUE_SCOPES
            .iter()
            .position(|scope| *scope == defaults.issues)
            .unwrap_or(1);
        let row = combo("Issues listées par défaut", &labels, current);
        row.set_subtitle("Pour les projets qui ne choisissent pas");
        let sender = self.sender.clone();
        row.connect_selected_notify(move |row| {
            let scope = ISSUE_SCOPES[selected(row).min(ISSUE_SCOPES.len() - 1)];
            sender.input(Msg::EditDefaults(DefaultsEdit::Issues(scope)));
        });
        issues.add(&row);

        let prs = adw::PreferencesGroup::new();
        prs.set_title("Pull requests");
        let switch = |title: &str, subtitle: &str, on: bool, edit: fn(bool) -> DefaultsEdit| {
            let row = adw::SwitchRow::new();
            row.set_title(title);
            row.set_subtitle(subtitle);
            row.set_active(on);
            let sender = self.sender.clone();
            row.connect_active_notify(move |row| {
                sender.input(Msg::EditDefaults(edit(row.is_active())));
            });
            row
        };
        prs.add(&switch(
            "Les PR des autres qui me sont assignées",
            "En plus des miennes et de celles à relire",
            defaults.include_assigned,
            DefaultsEdit::IncludeAssigned,
        ));
        prs.add(&switch(
            "Seulement les reviews demandées à moi",
            "Sans celles demandées à une de mes équipes",
            defaults.direct_reviews_only,
            DefaultsEdit::DirectReviewsOnly,
        ));
        let days = adw::SpinRow::with_range(1.0, 15.0, 1.0);
        days.set_title("Une review qui attend passe en priorité 1 après");
        days.set_subtitle("Jours ouvrés");
        days.set_value(f64::from(defaults.review_escalation_days));
        let sender = self.sender.clone();
        days.connect_value_notify(move |row| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 1 to 15.
            let value = row.value().round() as u32;
            sender.input(Msg::EditDefaults(DefaultsEdit::EscalationDays(value)));
        });
        prs.add(&days);

        let general = adw::PreferencesGroup::new();
        general.set_title("Général");
        let labels: Vec<String> = REFRESH_CHOICES
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect();
        let current = REFRESH_CHOICES
            .iter()
            .position(|(value, _)| *value == defaults.refresh)
            .unwrap_or(1);
        let refresh = combo("Rafraîchir toutes les", &labels, current);
        let sender = self.sender.clone();
        refresh.connect_selected_notify(move |row| {
            let (value, _) = REFRESH_CHOICES[selected(row).min(REFRESH_CHOICES.len() - 1)];
            sender.input(Msg::EditDefaults(DefaultsEdit::Refresh(value)));
        });
        general.add(&refresh);
        let label = adw::EntryRow::new();
        label.set_title("Étiquette Todoist des actions demandées par Claude");
        label.set_text(&defaults.claude_label);
        label.set_show_apply_button(true);
        let sender = self.sender.clone();
        label.connect_apply(move |entry| {
            sender.input(Msg::EditDefaults(DefaultsEdit::ClaudeLabel(
                entry.text().to_string(),
            )));
        });
        general.add(&label);
        vec![issues, prs, general]
    }

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

    /// The repository, issues and GitHub Project of a project linked to an owner.
    fn github_rows(
        &self,
        expander: &adw::ExpanderRow,
        config: &Config,
        choices: &Choices,
        index: usize,
        link: &GithubLink,
    ) {
        let mut repos = repo_options(choices, &link.owner);
        for listed in &link.repos {
            if !repos
                .iter()
                .any(|(known, _)| known.eq_ignore_ascii_case(listed))
            {
                repos.push((listed.clone(), listed.clone()));
            }
        }
        let current = link.repos.first().and_then(|listed| {
            repos
                .iter()
                .position(|(known, _)| known.eq_ignore_ascii_case(listed))
        });
        expander.add_row(&self.choice_row(
            ("Dépôt", "Tous les dépôts"),
            repos,
            current,
            index,
            Edit::Repo,
        ));
        let issues = issue_options();
        let current = link
            .issues
            .and_then(|chosen| issues.iter().position(|(scope, _)| *scope == chosen));
        let default = format!("Par défaut ({})", issue_label(config.defaults.issues));
        expander.add_row(&self.choice_row(
            ("Issues", &default),
            issues,
            current,
            index,
            Edit::Issues,
        ));
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

    /// A handle to drag the project to another place in the list; the whole row takes a drop.
    fn drag_handle(&self, expander: &adw::ExpanderRow, index: usize) -> gtk::Image {
        let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        handle.add_css_class("drag-handle");
        handle.set_tooltip_text(Some("Glisser pour déplacer"));
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let place = u32::try_from(index).unwrap_or(u32::MAX);
        source.set_content(Some(&gtk::gdk::ContentProvider::for_value(
            &place.to_value(),
        )));
        let row = expander.clone();
        source.connect_drag_begin(move |source, _| {
            source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 0, 0);
        });
        handle.add_controller(source);

        let target = gtk::DropTarget::new(u32::static_type(), gtk::gdk::DragAction::MOVE);
        let row = expander.clone();
        target.connect_enter(move |_, _, _| {
            row.add_css_class("drop-here");
            gtk::gdk::DragAction::MOVE
        });
        let row = expander.clone();
        target.connect_leave(move |_| row.remove_css_class("drop-here"));
        let sender = self.sender.clone();
        target.connect_drop(move |_, value, _, _| {
            let Ok(from) = value.get::<u32>() else {
                return false;
            };
            let from = usize::try_from(from).unwrap_or(usize::MAX);
            sender.input(Msg::EditProject(from, Edit::MoveTo(index)));
            true
        });
        expander.add_controller(target);
        handle
    }

    /// The Todoist section of a project: one to choose, or a new one to create.
    fn todoist_rows(
        &self,
        expander: &adw::ExpanderRow,
        config: &Config,
        choices: &Choices,
        index: usize,
        project: &Project,
    ) {
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
            .and_then(|link| section_index(&plain, config, link));
        if let Some(link) = &project.todoist
            && current.is_none()
        {
            let name = todoist_project(config, link).unwrap_or_default().to_owned();
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

        // Or a new section, in the Todoist project that holds the others.
        let parent = project
            .todoist
            .as_ref()
            .and_then(|link| todoist_project(config, link))
            .map(str::to_owned)
            .or_else(|| config.defaults.todoist_parent.clone())
            .or_else(|| guess_parent(choices))
            .or_else(|| choices.todoist.first().map(|known| known.name.clone()));
        if let Some(parent) = parent.filter(|_| !choices.todoist.is_empty()) {
            let create = adw::EntryRow::new();
            create.set_title(&escape(&format!("Nouvelle section dans « {parent} »")));
            create.set_show_apply_button(true);
            create.set_tooltip_text(Some("Crée la section dans Todoist et l'associe au projet"));
            let sender = self.sender.clone();
            create.connect_apply(move |entry| {
                sender.input(Msg::CreateSection(
                    index,
                    parent.clone(),
                    entry.text().to_string(),
                ));
            });
            expander.add_row(&create);
        }
    }

    fn project_editor(
        &self,
        config: &Config,
        choices: &Choices,
        index: usize,
        project: &Project,
    ) -> adw::ExpanderRow {
        let expander = adw::ExpanderRow::new();
        expander.set_title(&escape(&project.name));
        expander.set_subtitle(&escape(&summary(config, project)));
        expander.add_prefix(&self.drag_handle(&expander, index));
        expander.add_prefix(&crate::dot(&project.color));
        expander.set_expanded(self.expanded == Some(index));
        let sender = self.sender.clone();
        expander.connect_expanded_notify(move |row| {
            sender.input(Msg::Expanded(index, row.is_expanded()));
        });

        let name = adw::EntryRow::new();
        name.set_title("Nom");
        name.set_text(&project.name);
        // Applied on Enter: drawing the page again while typing would take the focus away.
        name.set_show_apply_button(true);
        let (sender, title) = (self.sender.clone(), expander.clone());
        name.connect_changed(move |entry| {
            title.set_title(&escape(&entry.text()));
            sender.input(Msg::NameTyped(index, entry.text().to_string()));
        });
        let sender = self.sender.clone();
        name.connect_apply(move |entry| {
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
            self.github_rows(&expander, config, choices, index, link);
        }

        self.todoist_rows(&expander, config, choices, index, project);
        if config.defaults.by_day {
            expander.add_row(&self.days_row(index, project));
        }

        // Up and down: the order of this list is the order of the sidebar and the home page.
        let count = config.projects.len();
        let order = adw::ActionRow::new();
        order.set_title("Position");
        order.set_subtitle(&format!("{} sur {count}", index + 1));
        for (icon, tip, step, enabled) in [
            ("go-up-symbolic", "Monter", -1, index > 0),
            ("go-down-symbolic", "Descendre", 1, index + 1 < count),
        ] {
            let button = gtk::Button::from_icon_name(icon);
            button.add_css_class("flat");
            button.set_valign(gtk::Align::Center);
            button.set_tooltip_text(Some(tip));
            button.set_sensitive(enabled);
            let sender = self.sender.clone();
            button
                .connect_clicked(move |_| sender.input(Msg::EditProject(index, Edit::Move(step))));
            order.add_suffix(&button);
        }
        expander.add_row(&order);

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
                    repos: vec!["baxyz/pigeonnier".to_owned()],
                    projects: vec![OwnerProject {
                        number: 3,
                        title: "Perso".to_owned(),
                    }],
                },
                Owner {
                    login: "helpers4".to_owned(),
                    repos: vec!["helpers4/devcontainer".to_owned()],
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
    fn projects_move_one_place_or_sort_by_name() {
        let mut config = Config::default();
        for (index, name) in ["Zen", "écrits", "Brig-id"].into_iter().enumerate() {
            let mut project = new_project(index);
            name.clone_into(&mut project.name);
            config.projects.push(project);
        }
        let names = |config: &Config| -> Vec<String> {
            config
                .projects
                .iter()
                .map(|project| project.name.clone())
                .collect()
        };
        assert!(apply(&mut config, 2, Edit::Move(-1)));
        assert_eq!(names(&config), ["Zen", "Brig-id", "écrits"]);
        assert!(!apply(&mut config, 0, Edit::Move(-1)));
        assert!(!apply(&mut config, 2, Edit::Move(1)));
        sort_projects(&mut config);
        assert_eq!(names(&config), ["Brig-id", "écrits", "Zen"]);
    }

    #[test]
    fn a_dragged_project_lands_where_it_was_dropped() {
        let mut config = Config::default();
        for (index, name) in ["A", "B", "C", "D"].into_iter().enumerate() {
            let mut project = new_project(index);
            name.clone_into(&mut project.name);
            config.projects.push(project);
        }
        let names = |config: &Config| -> String {
            config
                .projects
                .iter()
                .map(|project| project.name.as_str())
                .collect()
        };
        assert!(apply(&mut config, 3, Edit::MoveTo(0)));
        assert_eq!(names(&config), "DABC");
        assert!(apply(&mut config, 0, Edit::MoveTo(2)));
        assert_eq!(names(&config), "ABDC");
        assert!(!apply(&mut config, 1, Edit::MoveTo(1)));
        // The open project follows: B (1) moved to 3, so C (3) is now at 2.
        assert_eq!(moved_to(3, 1, 3), 2);
        assert_eq!(moved_to(1, 1, 3), 3);
        assert_eq!(moved_to(0, 3, 0), 1);
        assert!(apply(&mut config, 0, Edit::Day(Day::Thu, true)));
        assert!(apply(&mut config, 0, Edit::Day(Day::Mon, true)));
        assert_eq!(config.projects[0].days, [Day::Mon, Day::Thu]);
        assert!(apply(&mut config, 0, Edit::Day(Day::Mon, false)));
        assert_eq!(config.projects[0].days, [Day::Thu]);
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

    #[test]
    fn a_project_narrows_to_one_repository_and_chooses_its_issues() {
        let mut config = Config::default();
        config.projects.push(new_project(0));
        apply(&mut config, 0, Edit::Owner(Some("helpers4".to_owned())));
        assert_eq!(
            repo_options(&choices(), "helpers4"),
            [(
                "helpers4/devcontainer".to_owned(),
                "devcontainer".to_owned()
            )]
        );
        apply(
            &mut config,
            0,
            Edit::Repo(Some("helpers4/devcontainer".to_owned())),
        );
        apply(&mut config, 0, Edit::Issues(Some(IssueScope::All)));
        let link = config.projects[0].github.as_ref().unwrap();
        assert_eq!(link.repos, ["helpers4/devcontainer"]);
        assert_eq!(link.issues, Some(IssueScope::All));
        assert_eq!(
            summary(&config, &config.projects[0]),
            "GitHub helpers4/devcontainer, toutes les issues"
        );
        apply(&mut config, 0, Edit::Repo(None));
        apply(&mut config, 0, Edit::Issues(None));
        let link = config.projects[0].github.as_ref().unwrap();
        assert_eq!(link.repos, Vec::<String>::new());
        assert_eq!(link.issues, None);
    }
}
