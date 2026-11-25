// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the UIs draw: the sidebar, the list of a view already filtered, sorted and grouped, the
//! home page, labels ready to show, semantic colors and the actions each item offers. A new state,
//! filter or sort rule changes this crate only.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{Config, Day, Project};
use crate::item::{Blocker, Due, Duration, Item, Kind, OTHERS, Priority, Reason, State};
use crate::map::Record;
use crate::query::{self, Field, Filter, GroupBy, PRESETS, Query, ViewSpec};
use crate::source::todoist::Comment;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// No project is configured yet: the UI offers the first-run setup.
    pub needs_setup: bool,
    pub github_connected: bool,
    pub todoist_connected: bool,
    pub sidebar: Sidebar,
    pub page: Page,
    pub home: Home,
    pub badge: Badge,
    pub issues: Vec<Issue>,
    pub refreshed_at: Option<DateTime<Utc>>,
    /// Present when projects have days: the UI offers a choice of day.
    pub day_bar: Option<DayBar>,
}

/// The choice of day, when projects have days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayBar {
    /// The day shown; `None` for every day.
    pub day: Option<Day>,
}

/// The entries of the sidebar. Each one is a saved filter: choosing it fills the filter bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidebar {
    pub views: Vec<Entry>,
    pub projects: Vec<Entry>,
    /// "Projets du jeudi" when one day is shown, else "Projets".
    pub projects_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// `view:action`, `saved:revue-du-lundi`, `projects` for all of them, `project:acme`.
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
    /// The color of a project; several for the entry that shows them all.
    pub colors: Vec<String>,
    pub count: u32,
    /// Saved by the person, who can remove it.
    pub saved: bool,
    pub spec: ViewSpec,
}

/// The list of a view, with what the filter bar shows above it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub spec: ViewSpec,
    pub tokens: Vec<Token>,
    pub criteria: Vec<Criterion>,
    pub types: Vec<TypeTab>,
    pub groups: Vec<Group>,
    pub total: u32,
}

/// A filter in use, as the bar spells it: "État est à relire, CI rouge".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub field: Field,
    pub label: String,
    pub verb: String,
    pub values: String,
}

/// A field one can filter on, with how many items each value would keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    pub field: Field,
    pub label: String,
    pub single: bool,
    pub negate: bool,
    pub values: Vec<CriterionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriterionValue {
    pub id: String,
    pub label: String,
    pub count: u32,
    pub selected: bool,
}

/// One of the type buttons: everything, or one type, with its count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeTab {
    /// The value of the type filter; `None` for everything.
    pub id: Option<String>,
    /// What the type icon shows: `pr`, `issue`, `task` or `roadmap`.
    pub type_id: Option<String>,
    pub label: String,
    pub count: u32,
    pub active: bool,
}

/// Items under one heading. Groups are always open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// The heading of the first level, on the first group it holds, when there are two levels.
    pub heading: Option<String>,
    pub title: String,
    pub color: Option<String>,
    pub items: Vec<ItemVm>,
}

/// The home page: what presses, project by project, and what has a date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Home {
    pub stats: Vec<Stat>,
    pub cards: Vec<Card>,
    pub deadlines: Vec<Deadline>,
    /// How many items have no date, said under the deadlines.
    pub undated: Option<String>,
    /// How many items "Commencer" would go through.
    pub start_count: u32,
    /// The day shown, when projects have days and one was chosen.
    pub day: Option<DayHome>,
}

/// The top of the home page on one day, and the projects that wait for another day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayHome {
    /// "Mercredi"
    pub title: String,
    /// "Arnaud Work, baxyz · et Fajro tous les jours"
    pub subtitle: String,
    pub resting: Vec<Resting>,
}

/// A project that is not worked on the day shown, in one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resting {
    pub entry: String,
    pub name: String,
    pub color: String,
    pub action_count: u32,
    /// What presses anyway, in a word or two: "conflits", "en retard de 2 j".
    pub urgent: Option<String>,
    /// Its next day: "jeu."
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    pub label: String,
    pub count: u32,
    pub alert: bool,
    pub spec: ViewSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub entry: String,
    pub name: String,
    pub color: String,
    pub action_count: u32,
    pub lines: Vec<CardLine>,
    pub footer: Option<String>,
    pub spec: ViewSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardLine {
    pub tag: String,
    pub tone: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deadline {
    pub item_id: String,
    pub project: String,
    /// "−2" days late, or the day of the month.
    pub big: String,
    /// "jours", a weekday or a month.
    pub small: String,
    pub overdue: bool,
    pub type_id: String,
    pub type_label: String,
    pub title: String,
    pub sub: String,
}

/// The counter of the top bar: what is P1 or late. Red only when something is late.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    pub count: u32,
    pub alert: bool,
}

/// A source that failed: the rest of the list stays usable, and comes from the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub source: String,
    pub message: String,
    pub reconnect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemVm {
    pub id: String,
    /// `pr`, `issue`, `task` or `roadmap`: what the type icon shows.
    pub type_id: String,
    pub type_label: String,
    pub title: String,
    pub context: String,
    pub project: String,
    pub project_name: String,
    pub project_color: String,
    /// "GitHub" or "Todoist": where opening the item leads.
    pub source: String,
    pub url: String,
    pub priority: String,
    /// `red`, `orange`, `blue`, or nothing for P4.
    pub flag_color: Option<String>,
    pub flag_label: String,
    pub due_label: Option<String>,
    /// The date that matters, in a word or two, for where room is short.
    pub due_short: Option<String>,
    /// `overdue`, `today`, `soon` or `later`.
    pub due_urgency: Option<String>,
    pub state_label: Option<String>,
    /// `red` or `green` when the state calls for it.
    pub state_tone: Option<String>,
    /// What keeps a pull request from being merged, when its state does not already say it.
    pub blocked_label: Option<String>,
    /// Shown dimmed: a draft pull request, not ready for anyone yet.
    pub dimmed: bool,
    pub action_required: bool,
    pub actions: Vec<String>,
    pub details: DetailsVm,
}

/// What the detail pane shows under the title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailsVm {
    /// Markdown, for the UI to render; `None` when the item has no description to read here.
    pub description: Option<String>,
    /// What keeps a pull request from being merged, a sentence each.
    pub blockers: Vec<String>,
    pub facts: Vec<Fact>,
    /// The item has comments that can be read here.
    pub commentable: bool,
    /// Whether there are comments to read; `None` when the service did not say.
    pub has_comments: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentVm {
    pub posted: Option<String>,
    /// Markdown, for the UI to render.
    pub text: String,
    pub attachment: Option<AttachmentVm>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentVm {
    pub name: String,
    pub url: String,
    pub size: Option<String>,
}

const WEEKDAYS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
const MONTHS: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

fn date_label(date: NaiveDate, today: NaiveDate) -> String {
    let days = (date - today).num_days();
    match days {
        0 => "aujourd'hui".to_owned(),
        1 => "demain".to_owned(),
        -1 => "hier".to_owned(),
        _ if days < 0 => format!("en retard de {} j", -days),
        _ if days < 7 => WEEKDAYS[date.weekday().num_days_from_monday() as usize].to_owned(),
        _ => format!("{} {}", date.day(), MONTHS[date.month0() as usize]),
    }
}

fn due_text(due: Due, now: DateTime<FixedOffset>) -> String {
    let offset = *now.offset();
    let date = date_label(due.local_date(offset), now.date_naive());
    match due {
        Due::Date(_) => date,
        Due::DateTime(time) => {
            let local = time.with_timezone(&offset);
            format!("{date} {:02}:{:02}", local.hour(), local.minute())
        }
    }
}

fn full_date(date: NaiveDate) -> String {
    format!(
        "{} {} {} {}",
        WEEKDAYS[date.weekday().num_days_from_monday() as usize],
        date.day(),
        MONTHS[date.month0() as usize],
        date.year()
    )
}

fn full_time(time: DateTime<Utc>, offset: FixedOffset) -> String {
    let local = time.with_timezone(&offset);
    format!(
        "{} à {:02}:{:02}",
        full_date(local.date_naive()),
        local.hour(),
        local.minute()
    )
}

fn duration_text(duration: Duration) -> String {
    match duration {
        Duration::Days(1) => "1 jour".to_owned(),
        Duration::Days(days) => format!("{days} jours"),
        Duration::Minutes(minutes) if minutes < 60 => format!("{minutes} min"),
        Duration::Minutes(minutes) if minutes % 60 == 0 => format!("{} h", minutes / 60),
        Duration::Minutes(minutes) => format!("{} h {:02}", minutes / 60, minutes % 60),
    }
}

fn size_text(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // A size shown with one decimal.
    let size = bytes as f64;
    match bytes {
        0..1_000 => format!("{bytes} o"),
        1_000..1_000_000 => format!("{:.0} ko", size / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} Mo", size / 1e6),
        _ => format!("{:.1} Go", size / 1e9),
    }
}

/// What an unfolded task shows of its comments.
#[must_use]
pub fn comments_vm(comments: &[Comment], now: DateTime<FixedOffset>) -> Vec<CommentVm> {
    comments
        .iter()
        .map(|comment| CommentVm {
            posted: comment.posted_at.map(|time| full_time(time, *now.offset())),
            text: comment.content.clone(),
            attachment: comment.attachment.as_ref().map(|file| AttachmentVm {
                name: file.name.clone(),
                url: file.url.clone(),
                size: file.size.map(size_text),
            }),
        })
        .collect()
}

fn due_label(item: &Item, now: DateTime<FixedOffset>) -> Option<String> {
    let offset = *now.offset();
    match (item.due, item.deadline) {
        (Some(due), Some(deadline)) if due.local_date(offset) != deadline => Some(format!(
            "prévu {} · limite {}",
            due_text(due, now),
            date_label(deadline, now.date_naive())
        )),
        (_, Some(deadline)) => Some(date_label(deadline, now.date_naive())),
        (Some(due), None) => Some(due_text(due, now)),
        (None, None) => None,
    }
}

/// The date that matters, in a word or two: what cards and deadlines have room for.
fn short_due(item: &Item, now: DateTime<FixedOffset>) -> Option<String> {
    match (item.deadline, item.due) {
        (Some(deadline), _) => Some(date_label(deadline, now.date_naive())),
        (None, Some(due)) => Some(due_text(due, now)),
        (None, None) => None,
    }
}

fn due_urgency(item: &Item, now: DateTime<FixedOffset>) -> Option<String> {
    let due = item.effective_due()?;
    let days = (due.local_date(*now.offset()) - now.date_naive()).num_days();
    let urgency = match days {
        _ if item.is_overdue(now) => "overdue",
        0 => "today",
        1..=3 => "soon",
        _ => "later",
    };
    Some(urgency.to_owned())
}

fn state_label(item: &Item, now: DateTime<FixedOffset>) -> Option<String> {
    match item.state {
        State::ActionRequired(Reason::ToReview) => {
            let days = item
                .requested_at
                .map_or(0, |time| (now.with_timezone(&Utc) - time).num_days());
            Some(if days > 0 {
                format!("à relire depuis {days} j")
            } else {
                "à relire".to_owned()
            })
        }
        State::ActionRequired(Reason::ChangesRequested) => Some("à corriger".to_owned()),
        State::ActionRequired(Reason::CiFailed) => Some("CI rouge".to_owned()),
        State::ActionRequired(Reason::Conflicts) => Some("conflits".to_owned()),
        State::ActionRequired(Reason::ToMerge) => Some("à merger".to_owned()),
        State::Waiting if item.draft => Some("brouillon".to_owned()),
        State::Waiting => Some("en attente".to_owned()),
        State::Planned => None,
    }
}

/// The name and the color of each project, as the items name them.
struct Projects<'a> {
    config: &'a Config,
}

impl<'a> Projects<'a> {
    fn muted(&self, id: &str) -> bool {
        self.config
            .projects
            .iter()
            .any(|project| project.id == id && project.muted)
    }

    /// `(order, name, color)`: the pinned ones first, then the order of the configuration, then
    /// "Autres".
    fn look(&self, id: &str) -> (usize, &str, &str) {
        let count = self.config.projects.len();
        self.config
            .projects
            .iter()
            .position(|project| project.id == id)
            .map_or((2 * count, "Autres", "gray"), |index| {
                let project = &self.config.projects[index];
                let order = if project.pinned { index } else { count + index };
                (order, project.name.as_str(), project.color.as_str())
            })
    }

    /// The projects shown, in their order.
    fn shown(&self) -> Vec<&'a crate::config::Project> {
        let mut shown: Vec<_> = self
            .config
            .projects
            .iter()
            .filter(|project| !project.muted)
            .collect();
        shown.sort_by_key(|project| !project.pinned);
        shown
    }
}

fn details_vm(item: &Item, projects: &Projects, now: DateTime<FixedOffset>) -> DetailsVm {
    let offset = *now.offset();
    let details = item.details.as_ref();
    let mut facts = Vec::new();
    let mut fact = |label: &str, value: String| {
        if !value.is_empty() {
            facts.push(Fact {
                label: label.to_owned(),
                value,
            });
        }
    };
    fact("Projet", projects.look(&item.project).1.to_owned());
    match details {
        Some(details) => fact("Où", details.place.clone()),
        None => fact("Où", item.context.clone()),
    }
    if let Some(requested) = item.requested_at {
        fact("Review demandée", full_time(requested, offset));
    }
    if let Some(due) = item.due {
        fact(
            "Prévue",
            match due {
                Due::Date(date) => full_date(date),
                Due::DateTime(time) => full_time(time, offset),
            },
        );
    }
    if let Some(recurrence) = details.and_then(|details| details.recurrence.as_ref()) {
        fact("Récurrence", recurrence.clone());
    }
    if let Some(deadline) = item.deadline {
        fact("Échéance", full_date(deadline));
    }
    if let Some(duration) = details.and_then(|details| details.duration) {
        fact("Durée", duration_text(duration));
    }
    if let Some(details) = details {
        fact("Étiquettes", details.labels.join(", "));
    }
    let added = details.and_then(|details| details.added_at);
    if let Some(added) = added {
        fact("Créée", full_time(added, offset));
    }
    if item.updated_at != DateTime::<Utc>::default() && Some(item.updated_at) != added {
        fact("Modifiée", full_time(item.updated_at, offset));
    }
    DetailsVm {
        description: details.map(|details| details.description.clone()),
        blockers: item.blockers.iter().map(Blocker::sentence).collect(),
        facts,
        commentable: item.is_todoist(),
        has_comments: details
            .and_then(|details| details.comment_count)
            .map(|count| count > 0),
    }
}

/// The types, in the order they are offered: `(id, typed value, label, plural)`.
const TYPES: [(&str, &str, &str, &str); 4] = [
    ("pr", "pr", "Pull request", "Pull requests"),
    ("issue", "issue", "Issue", "Issues"),
    ("task", "tâche", "Tâche", "Tâches"),
    ("roadmap", "roadmap", "Roadmap", "Roadmap"),
];

fn type_label(id: &str) -> &'static str {
    TYPES
        .iter()
        .find(|known| known.0 == id)
        .map_or("", |known| known.2)
}

fn item_vm(item: &Item, projects: &Projects, now: DateTime<FixedOffset>) -> ItemVm {
    let (flag_color, flag_label) = match item.priority {
        Priority::P1 => (Some("red"), "Priorité 1"),
        Priority::P2 => (Some("orange"), "Priorité 2"),
        Priority::P3 => (Some("blue"), "Priorité 3"),
        Priority::P4 => (None, "Priorité 4"),
    };
    let mut actions = vec!["open".to_owned(), "snooze".to_owned()];
    if item.is_todoist() {
        actions.push("complete".to_owned());
    }
    if item.state == State::ActionRequired(Reason::ToMerge) && item.kind == Kind::MyPr {
        actions.push("merge".to_owned());
    }
    let state_tone = match item.state {
        State::ActionRequired(Reason::CiFailed | Reason::Conflicts) => Some("red"),
        State::ActionRequired(Reason::ToMerge) => Some("green"),
        _ => None,
    };
    let state = state_label(item, now);
    let blocked: Vec<&str> = item
        .blockers
        .iter()
        .map(Blocker::short)
        .filter(|short| state.as_deref() != Some(*short))
        .collect();
    let (_, project_name, project_color) = projects.look(&item.project);
    ItemVm {
        id: item.id.clone(),
        type_id: item.type_id().to_owned(),
        type_label: type_label(item.type_id()).to_owned(),
        title: item.title.clone(),
        context: item.context.clone(),
        project: item.project.clone(),
        project_name: project_name.to_owned(),
        project_color: project_color.to_owned(),
        source: if item.is_todoist() {
            "Todoist"
        } else {
            "GitHub"
        }
        .to_owned(),
        url: item.url.clone(),
        priority: format!("{:?}", item.priority),
        flag_color: flag_color.map(str::to_owned),
        flag_label: flag_label.to_owned(),
        due_label: due_label(item, now),
        due_short: short_due(item, now),
        due_urgency: due_urgency(item, now),
        state_label: state,
        state_tone: state_tone.map(str::to_owned),
        blocked_label: (!blocked.is_empty()).then(|| blocked.join(", ")),
        dimmed: item.draft,
        action_required: matches!(item.state, State::ActionRequired(_)),
        actions,
        details: details_vm(item, projects, now),
    }
}

/// The states, in the order their groups are shown: `(id, title, color)`.
const STATES: [(&str, &str, &str); 8] = [
    ("review", "À relire", "blue"),
    ("changes", "À corriger", "orange"),
    ("ci", "CI en échec", "red"),
    ("conflicts", "Conflits", "red"),
    ("merge", "Prêt à merger", "green"),
    ("waiting", "En attente", "gray"),
    ("draft", "Brouillons", "gray"),
    ("planned", "Planifié", "gray"),
];

/// The due dates, in the order their groups are shown: `(id, title, color)`.
const DUES: [(&str, &str, Option<&str>); 5] = [
    ("late", "En retard", Some("red")),
    ("today", "Aujourd'hui", Some("orange")),
    ("week", "Dans les 7 jours", None),
    ("later", "Plus tard", None),
    ("none", "Sans date", None),
];

/// Where an item goes under a grouping: `(order, title, color)`.
fn group_of(
    item: &Item,
    by: GroupBy,
    projects: &Projects,
    now: DateTime<FixedOffset>,
) -> (usize, String, Option<String>) {
    let found = |index: Option<usize>| index.unwrap_or(usize::MAX);
    match by {
        GroupBy::Project => {
            let (order, name, color) = projects.look(&item.project);
            (order, name.to_owned(), Some(color.to_owned()))
        }
        GroupBy::State => {
            let index = found(STATES.iter().position(|known| known.0 == item.state_id()));
            let (_, title, color) = STATES.get(index).copied().unwrap_or_default();
            (index, title.to_owned(), Some(color.to_owned()))
        }
        GroupBy::Type => {
            let index = found(TYPES.iter().position(|known| known.0 == item.type_id()));
            let title = TYPES.get(index).map_or("", |known| known.3);
            (index, title.to_owned(), None)
        }
        GroupBy::Due => {
            let index = found(DUES.iter().position(|known| known.0 == item.due_id(now)));
            let (_, title, color) = DUES.get(index).copied().unwrap_or_default();
            (index, title.to_owned(), color.map(str::to_owned))
        }
        GroupBy::Priority => (
            usize::from(item.priority.rank()),
            format!("{:?}", item.priority),
            None,
        ),
    }
}

/// Everything the builders share: the items that count, their records and the configuration.
struct Board<'a> {
    config: &'a Config,
    projects: Projects<'a>,
    /// The items of the projects that are not muted, and are worked on the day shown, with the
    /// fields a query reads.
    live: Vec<(&'a Item, Record)>,
    now: DateTime<FixedOffset>,
    /// The day shown; `None` shows every project.
    day: Option<Day>,
}

impl<'a> Board<'a> {
    fn new(
        items: &'a [Item],
        config: &'a Config,
        now: DateTime<FixedOffset>,
        day: Option<Day>,
    ) -> Self {
        let projects = Projects { config };
        let day = day.filter(|_| config.defaults.by_day);
        let on = |id: &str| on_day(config, day, id);
        let live = items
            .iter()
            .filter(|item| !projects.muted(&item.project) && on(&item.project))
            .map(|item| (item, item.to_record(now)))
            .collect();
        Self {
            config,
            projects,
            live,
            now,
            day,
        }
    }

    /// The projects shown on the day shown, in their order.
    fn shown(&self) -> Vec<&'a Project> {
        let config = self.config;
        let day = self.day;
        Projects { config }
            .shown()
            .into_iter()
            .filter(|project| day.is_none_or(|day| project.on(day)))
            .collect()
    }

    /// What a view keeps, in its order.
    fn run(&self, spec: &ViewSpec) -> Vec<&'a Item> {
        let by_id: HashMap<&str, &Item> = self
            .live
            .iter()
            .map(|(item, _)| (item.id.as_str(), *item))
            .collect();
        let records = self.live.iter().map(|(_, record)| record.clone()).collect();
        spec.view(self.config)
            .run(records)
            .kept
            .iter()
            .filter_map(|record| by_id.get(record.provenance.id.as_str()).copied())
            .filter(|item| spec.query.matches_text(&item.title, &item.context))
            .collect()
    }

    fn count(&self, spec: &ViewSpec) -> u32 {
        count(self.run(spec).into_iter())
    }

    fn vm(&self, item: &Item) -> ItemVm {
        item_vm(item, &self.projects, self.now)
    }

    fn groups(&self, spec: &ViewSpec, kept: &[&Item]) -> Vec<Group> {
        let levels: Vec<GroupBy> = spec.group.iter().copied().take(2).collect();
        if levels.is_empty() {
            return vec![Group {
                heading: None,
                title: String::new(),
                color: None,
                items: kept.iter().map(|item| self.vm(item)).collect(),
            }];
        }
        // The key of each level, then the position in the sorted list: a stable order.
        let mut keyed: Vec<_> = kept
            .iter()
            .map(|item| {
                let keys: Vec<_> = levels
                    .iter()
                    .map(|by| group_of(item, *by, &self.projects, self.now))
                    .collect();
                (keys, *item)
            })
            .collect();
        keyed.sort_by(|(left, _), (right, _)| {
            let order = |keys: &[(usize, String, Option<String>)]| {
                keys.iter()
                    .map(|key| (key.0, key.1.clone()))
                    .collect::<Vec<_>>()
            };
            order(left).cmp(&order(right))
        });
        let mut groups: Vec<Group> = Vec::new();
        let mut previous: Option<Vec<(usize, String, Option<String>)>> = None;
        for (keys, item) in keyed {
            if previous.as_ref() != Some(&keys) {
                let last = keys.last().cloned().unwrap_or_default();
                let new_heading = previous
                    .as_ref()
                    .is_none_or(|known| known.first() != keys.first());
                groups.push(Group {
                    heading: (keys.len() > 1 && new_heading).then(|| keys[0].1.clone()),
                    title: last.1,
                    color: last.2,
                    items: Vec::new(),
                });
                previous = Some(keys);
            }
            if let Some(group) = groups.last_mut() {
                group.items.push(self.vm(item));
            }
        }
        groups
    }

    fn criteria(&self, query: &Query) -> Vec<Criterion> {
        Field::ALL
            .into_iter()
            .map(|field| {
                let rest = self.run(&ViewSpec::new(query.without(field), &[]));
                let records: Vec<Record> =
                    rest.iter().map(|item| item.to_record(self.now)).collect();
                let current = query.filter(field);
                let values = field
                    .choices(self.config)
                    .into_iter()
                    .map(|choice| CriterionValue {
                        count: count(records.iter().filter(|record| {
                            choice.predicate.eval(record) == crate::filter::Truth::True
                        })),
                        selected: current.is_some_and(|filter| filter.values.contains(&choice.id)),
                        id: choice.id,
                        label: choice.label,
                    })
                    .collect();
                Criterion {
                    field,
                    label: field.label().to_owned(),
                    single: field.single(),
                    negate: current.is_some_and(|filter| filter.negate),
                    values,
                }
            })
            .collect()
    }

    fn page(&self, spec: &ViewSpec) -> Page {
        let kept = self.run(spec);
        let criteria = self.criteria(&spec.query);
        let tokens = spec
            .query
            .filters
            .iter()
            .filter_map(|filter| {
                let criterion = criteria.iter().find(|known| known.field == filter.field)?;
                let values: Vec<&str> = criterion
                    .values
                    .iter()
                    .filter(|value| value.selected)
                    .map(|value| value.label.as_str())
                    .collect();
                let verb = match (criterion.single, filter.negate) {
                    (true, _) => "depuis",
                    (false, true) => "n'est pas",
                    (false, false) => "est",
                };
                Some(Token {
                    field: filter.field,
                    label: criterion.label.clone(),
                    verb: verb.to_owned(),
                    values: values.join(", "),
                })
            })
            .collect();
        let any_type = self.run(&ViewSpec::new(spec.query.without(Field::Type), &[]));
        let chosen = spec
            .query
            .filter(Field::Type)
            .filter(|filter| !filter.negate && filter.values.len() == 1)
            .map(|filter| filter.values[0].as_str());
        let mut types = vec![TypeTab {
            id: None,
            type_id: None,
            label: "Tout".to_owned(),
            count: count(any_type.iter()),
            active: spec.query.filter(Field::Type).is_none(),
        }];
        types.extend(TYPES.iter().map(|(id, typed, _, plural)| TypeTab {
            id: Some((*typed).to_owned()),
            type_id: Some((*id).to_owned()),
            label: (*plural).to_owned(),
            count: count(any_type.iter().filter(|item| item.type_id() == *id)),
            active: chosen == Some(*typed),
        }));
        Page {
            spec: spec.clone(),
            tokens,
            criteria,
            types,
            groups: self.groups(spec, &kept),
            total: count(kept.iter()),
        }
    }

    /// What waits for me in a project, dependency updates included: a review asked of me is mine
    /// to do, whoever opened the pull request.
    fn action_count(&self, project: Option<&str>) -> u32 {
        count(self.live.iter().filter(|(item, _)| {
            project.is_none_or(|id| item.project == id)
                && matches!(item.state, State::ActionRequired(_))
                && !item.is_snoozed(self.now)
        }))
    }

    fn has_items(&self, project: &str) -> bool {
        self.live.iter().any(|(item, _)| item.project == project)
    }

    fn sidebar(&self) -> Sidebar {
        let mut views: Vec<Entry> = PRESETS
            .iter()
            .map(|(id, title, icon, _, _)| {
                let spec = query::preset(id, self.config);
                Entry {
                    id: format!("view:{id}"),
                    title: (*title).to_owned(),
                    icon: Some((*icon).to_owned()),
                    colors: Vec::new(),
                    count: self.count(&spec),
                    saved: false,
                    spec,
                }
            })
            .collect();
        views.extend(self.config.views.iter().map(|saved| {
            let spec = ViewSpec::from_saved(saved, self.config);
            Entry {
                id: format!("saved:{}", saved.id),
                title: saved.name.clone(),
                icon: Some("starred-symbolic".to_owned()),
                colors: Vec::new(),
                count: self.count(&spec),
                saved: true,
                spec,
            }
        }));
        let shown = self.shown();
        let mut projects = vec![Entry {
            id: "projects".to_owned(),
            title: "Tous".to_owned(),
            icon: None,
            colors: shown.iter().map(|project| project.color.clone()).collect(),
            count: self.action_count(None),
            saved: false,
            spec: query::everything(),
        }];
        let others = self.has_items(OTHERS).then_some((OTHERS, "Autres", "gray"));
        projects.extend(
            shown
                .iter()
                .map(|project| {
                    (
                        project.id.as_str(),
                        project.name.as_str(),
                        project.color.as_str(),
                    )
                })
                .chain(others)
                .map(|(id, name, color)| Entry {
                    id: format!("project:{id}"),
                    title: name.to_owned(),
                    icon: None,
                    colors: vec![color.to_owned()],
                    count: self.action_count(Some(id)),
                    saved: false,
                    spec: query::project(id),
                }),
        );
        let projects_title = match self.day {
            Some(day) => format!("Projets du {}", day.name()),
            None => "Projets".to_owned(),
        };
        Sidebar {
            views,
            projects,
            projects_title,
        }
    }

    fn stats(&self) -> Vec<Stat> {
        let with = |field: Field, value: &str| {
            let mut spec = query::everything();
            spec.query.set(Filter::new(field, &[value]));
            spec
        };
        let stat = |label: &str, spec: ViewSpec, alert: bool| {
            let count = self.count(&spec);
            Stat {
                label: label.to_owned(),
                count,
                alert: alert && count > 0,
                spec,
            }
        };
        vec![
            stat("à traiter", query::preset("action", self.config), false),
            stat("en retard", with(Field::Due, "en-retard"), true),
            stat("pour aujourd'hui", with(Field::Due, "aujourd'hui"), false),
            stat(
                "reviews qui attendent",
                query::preset("reviews", self.config),
                false,
            ),
        ]
    }

    /// What a card says of one item: its state when it waits for me, else its date, else its type.
    fn card_line(&self, item: &Item) -> CardLine {
        let vm = self.vm(item);
        let late = vm.due_urgency.as_deref() == Some("overdue");
        let (tag, tone) = match (vm.state_label, vm.due_short) {
            (Some(label), _) if vm.action_required => (label, vm.state_tone),
            (_, Some(date)) => (date, late.then(|| "red".to_owned())),
            (Some(label), None) => (label, None),
            (None, None) => (vm.type_label.to_lowercase(), None),
        };
        CardLine {
            tag,
            tone,
            title: vm.title,
        }
    }

    /// A card for each project that has something to show, dependency updates aside.
    fn cards(&self, everything: &[&Item]) -> Vec<Card> {
        let others = self.has_items(OTHERS).then_some((OTHERS, "Autres", "gray"));
        let shown = self.shown();
        let known = shown.iter().map(|project| {
            (
                project.id.as_str(),
                project.name.as_str(),
                project.color.as_str(),
            )
        });
        known
            .chain(others)
            .filter_map(|(id, name, color)| {
                let own: Vec<&Item> = everything
                    .iter()
                    .copied()
                    .filter(|item| item.project == id)
                    .collect();
                if own.is_empty() {
                    return None;
                }
                let next = own
                    .iter()
                    .filter(|item| item.effective_due().is_some() && !item.is_overdue(self.now))
                    .min_by_key(|item| {
                        item.effective_due()
                            .map(|due| due.local_date(*self.now.offset()))
                    });
                let action_count = self.action_count(Some(id));
                let footer = match next {
                    Some(item) => Some(format!(
                        "Prochaine échéance : {}, {}",
                        item.title,
                        short_due(item, self.now).unwrap_or_default()
                    )),
                    None if action_count == 0 => Some("Rien d'urgent".to_owned()),
                    None => None,
                };
                Some(Card {
                    entry: format!("project:{id}"),
                    name: name.to_owned(),
                    color: color.to_owned(),
                    action_count,
                    lines: own
                        .iter()
                        .take(3)
                        .map(|item| self.card_line(item))
                        .collect(),
                    footer,
                    spec: query::project(id),
                })
            })
            .collect()
    }

    fn deadline(&self, date: NaiveDate, item: &Item) -> Deadline {
        let overdue = item.is_overdue(self.now);
        let days = (date - self.now.date_naive()).num_days();
        let (big, small) = if overdue && days < 0 {
            (format!("−{}", -days), "jours")
        } else if (0..7).contains(&days) {
            (
                date.day().to_string(),
                WEEKDAYS[date.weekday().num_days_from_monday() as usize],
            )
        } else {
            (date.day().to_string(), MONTHS[date.month0() as usize])
        };
        let name = self.projects.look(&item.project).1;
        Deadline {
            item_id: item.id.clone(),
            project: item.project.clone(),
            big,
            small: small.to_owned(),
            overdue,
            type_id: item.type_id().to_owned(),
            type_label: type_label(item.type_id()).to_owned(),
            title: item.title.clone(),
            sub: format!("{name} · {}", short_due(item, self.now).unwrap_or_default()),
        }
    }

    fn home(&self) -> Home {
        let everything = self.run(&query::everything());
        let offset = *self.now.offset();
        let mut dated: Vec<(NaiveDate, &Item)> = everything
            .iter()
            .filter_map(|item| Some((item.effective_due()?.local_date(offset), *item)))
            .collect();
        dated.sort_by_key(|(date, _)| *date);
        let undated = everything.len() - dated.len();
        Home {
            day: None,
            start_count: self.count(&query::preset("action", self.config)),
            stats: self.stats(),
            cards: self.cards(&everything),
            deadlines: dated
                .iter()
                .take(8)
                .map(|(date, item)| self.deadline(*date, item))
                .collect(),
            undated: (undated > 0 && !dated.is_empty()).then(|| {
                format!(
                    "{undated} éléments sur {} n'ont pas de date : ils restent dans les cartes.",
                    everything.len()
                )
            }),
        }
    }
}

/// Whether the items of project `id` are shown on `day`: "Autres" always is.
fn on_day(config: &Config, day: Option<Day>, id: &str) -> bool {
    let Some(day) = day else {
        return true;
    };
    config
        .projects
        .iter()
        .find(|project| project.id == id)
        .is_none_or(|project| project.on(day))
}

/// The next day a project is worked on after `day`, if it has days.
fn next_day(project: &Project, day: Day) -> Option<Day> {
    (1..=7)
        .map(|step| Day::ALL[(day as usize + step) % 7])
        .find(|next| project.days.contains(next))
}

/// The top of the home page on `day`, with every project that waits for another day: `all` sees
/// every project, whatever the day.
fn day_home(board: &Board, all: &Board, day: Day) -> DayHome {
    let shown = board.shown();
    let (named, daily): (Vec<&Project>, Vec<&Project>) =
        shown.iter().partition(|project| !project.days.is_empty());
    let listed = |projects: &[&Project]| {
        projects
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let subtitle = match (named.is_empty(), daily.is_empty()) {
        (_, true) => listed(&named),
        (true, false) => format!("{} : tous les jours", listed(&daily)),
        (false, false) => format!("{} · et {} tous les jours", listed(&named), listed(&daily)),
    };
    let everything = all.run(&query::everything());
    let resting = all
        .shown()
        .into_iter()
        .filter(|project| !project.on(day))
        .map(|project| {
            let urgent = everything
                .iter()
                .filter(|item| item.project == project.id)
                .map(|item| all.card_line(item))
                .find(|line| line.tone.as_deref() == Some("red"))
                .map(|line| line.tag);
            Resting {
                entry: format!("project:{}", project.id),
                name: project.name.clone(),
                color: project.color.clone(),
                action_count: all.action_count(Some(&project.id)),
                urgent,
                next: next_day(project, day).map(|next| next.short().to_owned()),
            }
        })
        .collect();
    let mut title = day.name().to_owned();
    if let Some(first) = title.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    DayHome {
        title,
        subtitle,
        resting,
    }
}

/// Builds what a UI draws: the sidebar, the list of `spec` and the home page. Muted projects are
/// left out everywhere; with `defaults.by_day`, so are the projects not worked on `day`.
#[must_use]
pub fn build(
    items: &[Item],
    config: &Config,
    spec: &ViewSpec,
    day: Option<Day>,
    now: DateTime<FixedOffset>,
    issues: Vec<Issue>,
    refreshed_at: Option<DateTime<Utc>>,
) -> Snapshot {
    let board = Board::new(items, config, now, day);
    let mut home = board.home();
    if let Some(day) = board.day {
        let all = Board::new(items, config, now, None);
        home.day = Some(day_home(&board, &all, day));
    }
    Snapshot {
        needs_setup: config.projects.is_empty(),
        github_connected: false,
        todoist_connected: false,
        sidebar: board.sidebar(),
        page: board.page(spec),
        home,
        badge: badge(items, config, now),
        issues,
        refreshed_at,
        day_bar: config.defaults.by_day.then_some(DayBar { day: board.day }),
    }
}

fn count<T>(items: impl Iterator<Item = T>) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

fn badge(items: &[Item], config: &Config, now: DateTime<FixedOffset>) -> Badge {
    let projects = Projects { config };
    let live = || {
        items
            .iter()
            .filter(|item| !item.is_snoozed(now) && !item.bot && !projects.muted(&item.project))
    };
    Badge {
        count: count(live().filter(|item| item.priority == Priority::P1 || item.is_overdue(now))),
        alert: live().any(|item| item.is_overdue(now)),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::query::{SortBy, everything, preset};
    use crate::source::Provenance;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-08T10:00:00+01:00").unwrap()
    }

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn item(id: &str, project: &str) -> Item {
        Item {
            id: id.to_owned(),
            project: project.to_owned(),
            kind: Kind::Todo,
            title: id.to_owned(),
            context: String::new(),
            url: String::new(),
            priority: Priority::P4,
            due: None,
            deadline: None,
            state: State::Planned,
            updated_at: DateTime::default(),
            requested_at: None,
            bot: false,
            snoozed_until: None,
            details: None,
            draft: false,
            blockers: Vec::new(),
            provenance: Provenance {
                source: "test",
                id: id.to_owned(),
                version: None,
            },
        }
    }

    fn pr(id: &str, project: &str, kind: Kind, state: State) -> Item {
        let mut item = item(id, project);
        item.kind = kind;
        item.state = state;
        item
    }

    fn config() -> Config {
        Config::parse(
            r#"
            [[project]]
            id = "a"
            name = "A"
            [[project]]
            id = "b"
            name = "B"
            pinned = true
            [[project]]
            id = "muted"
            name = "Muted"
            muted = true
            [[project]]
            id = "empty"
            name = "Empty"
            "#,
            "test",
        )
        .unwrap()
    }

    fn snapshot_of(items: &[Item], spec: &ViewSpec) -> Snapshot {
        build(items, &config(), spec, None, now(), Vec::new(), None)
    }

    fn snapshot(items: &[Item]) -> Snapshot {
        snapshot_of(items, &everything())
    }

    fn typed(text: &str, group: &[GroupBy]) -> ViewSpec {
        ViewSpec::new(Query::parse(text, &config()), group)
    }

    /// The groups of a page, as `(heading, title, ids)`.
    fn groups(page: &Page) -> Vec<(Option<&str>, &str, Vec<&str>)> {
        page.groups
            .iter()
            .map(|group| {
                (
                    group.heading.as_deref(),
                    group.title.as_str(),
                    group.items.iter().map(|item| item.id.as_str()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn projects_are_ordered_and_hidden_as_configured() {
        let items = [
            item("1", "a"),
            item("2", "b"),
            item("3", "muted"),
            item("4", OTHERS),
        ];
        let snapshot = snapshot(&items);
        assert_eq!(
            groups(&snapshot.page),
            [
                (None, "B", vec!["2"]),
                (None, "A", vec!["1"]),
                (None, "Autres", vec!["4"]),
            ]
        );
        let entries: Vec<_> = snapshot
            .sidebar
            .projects
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        // Every project that is not muted keeps its entry, even with nothing to show.
        assert_eq!(
            entries,
            [
                "projects",
                "project:b",
                "project:a",
                "project:empty",
                "project:others"
            ]
        );
    }

    #[test]
    fn the_sort_of_the_view_reaches_each_group() {
        let mut low = item("low", "a");
        low.priority = Priority::P3;
        let mut late = item("late", "a");
        late.due = Some(Due::Date(date("2026-11-01")));
        let mut high = item("high", "a");
        high.priority = Priority::P1;
        let mut waiting = item("waiting", "a");
        waiting.priority = Priority::P1;
        waiting.state = State::Waiting;
        let items = [low, waiting, high, late];
        assert_eq!(
            groups(&snapshot(&items).page)[0].2,
            ["late", "high", "low", "waiting"]
        );
        let mut by_priority = everything();
        by_priority.sort = SortBy::Priority;
        assert_eq!(
            groups(&snapshot_of(&items, &by_priority).page)[0].2,
            ["high", "waiting", "low", "late"]
        );
    }

    #[test]
    fn snoozed_items_leave_the_list_until_their_time_or_until_asked_for() {
        let mut snoozed = item("s", "a");
        snoozed.snoozed_until = Some(now().with_timezone(&Utc) + Duration::hours(1));
        let items = [snoozed.clone()];
        assert_eq!(snapshot(&items).page.total, 0);
        let mut shown = everything();
        shown.query.snoozed = true;
        assert_eq!(snapshot_of(&items, &shown).page.total, 1);
        snoozed.snoozed_until = Some(now().with_timezone(&Utc) - Duration::hours(1));
        assert_eq!(snapshot(&[snoozed]).page.total, 1);
    }

    #[test]
    fn the_badge_counts_p1_and_late_and_turns_red_when_late() {
        let mut p1 = item("p1", "a");
        p1.priority = Priority::P1;
        assert_eq!(
            snapshot(std::slice::from_ref(&p1)).badge,
            Badge {
                count: 1,
                alert: false
            }
        );
        let mut late = item("late", "a");
        late.due = Some(Due::Date(date("2026-11-01")));
        assert_eq!(
            snapshot(&[p1, late]).badge,
            Badge {
                count: 2,
                alert: true
            }
        );
    }

    #[test]
    fn today_shows_late_and_today_only() {
        let mut late = item("late", "a");
        late.due = Some(Due::Date(date("2026-11-01")));
        let mut today = item("today", "a");
        today.due = Some(Due::Date(date("2026-11-08")));
        let mut later = item("later", "a");
        later.due = Some(Due::Date(date("2026-11-20")));
        let undated = item("undated", "a");
        let snapshot = snapshot_of(&[late, today, later, undated], &preset("today", &config()));
        assert_eq!(groups(&snapshot.page)[0].2, ["late", "today"]);
    }

    #[test]
    fn labels_read_naturally() {
        let today = date("2026-11-08");
        assert_eq!(date_label(today, today), "aujourd'hui");
        assert_eq!(date_label(date("2026-11-09"), today), "demain");
        assert_eq!(date_label(date("2026-11-05"), today), "en retard de 3 j");
        assert_eq!(date_label(date("2026-11-13"), today), "ven.");
        assert_eq!(date_label(date("2026-12-24"), today), "24 déc.");

        let mut both = item("x", "a");
        both.due = Some(Due::Date(date("2026-11-09")));
        both.deadline = Some(date("2026-11-13"));
        assert_eq!(
            due_label(&both, now()).unwrap(),
            "prévu demain · limite ven."
        );
        assert_eq!(vm(&both).due_short.as_deref(), Some("ven."));

        let mut review = item("x", "a");
        review.state = State::ActionRequired(Reason::ToReview);
        review.requested_at = Some(now().with_timezone(&Utc) - Duration::days(3));
        assert_eq!(state_label(&review, now()).unwrap(), "à relire depuis 3 j");
    }

    fn vm(item: &Item) -> ItemVm {
        let config = config();
        item_vm(item, &Projects { config: &config }, now())
    }

    #[test]
    fn only_todoist_tasks_can_be_completed() {
        assert!(
            vm(&item("td:1", "a"))
                .actions
                .contains(&"complete".to_owned())
        );
        let pr = vm(&item("gh-pr:a/b#1", "a"));
        assert!(!pr.actions.contains(&"complete".to_owned()));
        assert_eq!(pr.source, "GitHub");
    }

    #[test]
    fn only_my_prs_ready_to_merge_can_be_merged() {
        let mut ready = pr(
            "gh-pr:a/b#1",
            "a",
            Kind::MyPr,
            State::ActionRequired(Reason::ToMerge),
        );
        assert!(vm(&ready).actions.contains(&"merge".to_owned()));
        assert_eq!(vm(&ready).state_tone.as_deref(), Some("green"));
        ready.state = State::Waiting;
        assert!(!vm(&ready).actions.contains(&"merge".to_owned()));
    }

    #[test]
    fn every_item_says_what_it_is_and_where_it_belongs() {
        let review = vm(&pr(
            "gh-pr:a/b#1",
            "b",
            Kind::Review,
            State::ActionRequired(Reason::ToReview),
        ));
        assert_eq!(
            (review.type_id.as_str(), review.type_label.as_str()),
            ("pr", "Pull request")
        );
        assert_eq!(
            (review.project_name.as_str(), review.project_color.as_str()),
            ("B", "blue")
        );
        let mut claude = item("td:2", OTHERS);
        claude.kind = Kind::ClaudeAction;
        assert_eq!(vm(&claude).type_id, "task");
        assert_eq!(vm(&claude).project_name, "Autres");
        assert_eq!(
            vm(&pr("r", "a", Kind::Roadmap, State::Planned)).type_id,
            "roadmap"
        );
        assert_eq!(
            vm(&pr("i", "a", Kind::Issue, State::Planned)).type_id,
            "issue"
        );
    }

    #[test]
    fn a_snapshot_survives_json() {
        let snapshot = snapshot(&[item("1", "a")]);
        let text = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(serde_json::from_str::<Snapshot>(&text).unwrap(), snapshot);
    }

    #[test]
    fn details_are_spelled_out() {
        use crate::item::{Details, Duration};
        let mut task = item("td:1", "a");
        task.due = Some(Due::Date(date("2026-11-10")));
        task.updated_at = DateTime::parse_from_rfc3339("2026-11-07T09:05:00Z")
            .unwrap()
            .with_timezone(&Utc);
        task.details = Some(Details {
            description: "x".to_owned(),
            place: "Dev / acme".to_owned(),
            labels: vec!["a".to_owned(), "b".to_owned()],
            recurrence: Some("every tuesday".to_owned()),
            duration: Some(Duration::Minutes(90)),
            added_at: None,
            comment_count: Some(0),
        });
        let details = vm(&task).details;
        let facts: Vec<(&str, &str)> = details
            .facts
            .iter()
            .map(|fact| (fact.label.as_str(), fact.value.as_str()))
            .collect();
        assert_eq!(
            facts,
            [
                ("Projet", "A"),
                ("Où", "Dev / acme"),
                ("Prévue", "mar. 10 nov. 2026"),
                ("Récurrence", "every tuesday"),
                ("Durée", "1 h 30"),
                ("Étiquettes", "a, b"),
                ("Modifiée", "sam. 7 nov. 2026 à 10:05"),
            ]
        );
        assert_eq!(details.description.as_deref(), Some("x"));
        assert_eq!(
            (details.commentable, details.has_comments),
            (true, Some(false))
        );

        // A pull request has no description to read here, and says where it lives.
        let mut pull = item("gh-pr:x", "a");
        pull.context = "tool#1".to_owned();
        let details = vm(&pull).details;
        assert_eq!(details.description, None);
        assert!(!details.commentable);
        assert_eq!(details.facts[1].value, "tool#1");
    }

    #[test]
    fn durations_and_sizes() {
        use crate::item::Duration;
        assert_eq!(duration_text(Duration::Minutes(45)), "45 min");
        assert_eq!(duration_text(Duration::Minutes(120)), "2 h");
        assert_eq!(duration_text(Duration::Days(1)), "1 jour");
        assert_eq!(size_text(512), "512 o");
        assert_eq!(size_text(48_000), "48 ko");
        assert_eq!(size_text(2_500_000), "2.5 Mo");
    }

    fn pull_requests() -> Vec<Item> {
        let mut draft = pr("draft", "a", Kind::MyPr, State::Waiting);
        draft.draft = true;
        let mut bot = pr(
            "bot",
            "a",
            Kind::Review,
            State::ActionRequired(Reason::ToReview),
        );
        bot.bot = true;
        vec![
            pr(
                "review",
                "b",
                Kind::Review,
                State::ActionRequired(Reason::ToReview),
            ),
            pr(
                "ci",
                "a",
                Kind::MyPr,
                State::ActionRequired(Reason::CiFailed),
            ),
            pr("waiting", "a", Kind::MyPr, State::Waiting),
            draft,
            bot,
            item("task", "a"),
        ]
    }

    #[test]
    fn pull_requests_group_by_what_they_wait_for() {
        let snapshot = snapshot_of(&pull_requests(), &preset("prs", &config()));
        assert_eq!(
            groups(&snapshot.page),
            [
                (None, "À relire", vec!["review", "bot"]),
                (None, "CI en échec", vec!["ci"]),
                (None, "En attente", vec!["waiting"]),
                (None, "Brouillons", vec!["draft"]),
            ]
        );
        assert_eq!(snapshot.page.groups[1].color.as_deref(), Some("red"));
        assert_eq!(
            snapshot.page.groups[3].items[0].state_label.as_deref(),
            Some("brouillon")
        );
    }

    #[test]
    fn two_levels_of_groups_show_the_first_one_as_a_heading() {
        let spec = typed("type:pr", &[GroupBy::State, GroupBy::Project]);
        let snapshot = snapshot_of(&pull_requests(), &spec);
        assert_eq!(
            groups(&snapshot.page)[..3],
            [
                (Some("À relire"), "B", vec!["review"]),
                (None, "A", vec!["bot"]),
                (Some("CI en échec"), "A", vec!["ci"]),
            ]
        );
    }

    #[test]
    fn no_grouping_gives_one_flat_list() {
        let snapshot = snapshot_of(&pull_requests(), &typed("type:pr", &[]));
        assert_eq!(snapshot.page.groups.len(), 1);
        assert_eq!(snapshot.page.groups[0].items.len(), 5);
    }

    #[test]
    fn the_filter_bar_spells_the_filters_and_counts_what_each_value_would_keep() {
        let spec = typed("type:pr état:ci-rouge,à-relire -auteur:robot", &[]);
        let page = snapshot_of(&pull_requests(), &spec).page;
        assert_eq!(page.total, 2);
        let tokens: Vec<String> = page
            .tokens
            .iter()
            .map(|token| format!("{} {} {}", token.label, token.verb, token.values))
            .collect();
        assert_eq!(
            tokens,
            [
                "Type est PR",
                "État est à relire, CI rouge",
                "Auteur n'est pas un robot"
            ]
        );
        // The counts of a field ignore its own filter: they say what choosing a value would give.
        let state = page
            .criteria
            .iter()
            .find(|criterion| criterion.field == Field::State)
            .unwrap();
        let counts: Vec<(&str, u32, bool)> = state
            .values
            .iter()
            .filter(|value| value.count > 0)
            .map(|value| (value.id.as_str(), value.count, value.selected))
            .collect();
        assert_eq!(
            counts,
            [
                ("à-traiter", 2, false),
                ("à-relire", 1, true),
                ("ci-rouge", 1, true),
                ("en-attente", 1, false),
                ("brouillon", 1, false),
            ]
        );
        let types: Vec<(&str, u32, bool)> = page
            .types
            .iter()
            .map(|tab| (tab.label.as_str(), tab.count, tab.active))
            .collect();
        assert_eq!(
            types,
            [
                ("Tout", 2, false),
                ("Pull requests", 2, true),
                ("Issues", 0, false),
                ("Tâches", 0, false),
                ("Roadmap", 0, false),
            ]
        );
    }

    #[test]
    fn the_text_narrows_the_list() {
        let page = snapshot_of(&pull_requests(), &typed("WAIT", &[])).page;
        assert_eq!(groups(&page), [(None, "", vec!["waiting"])]);
    }

    #[test]
    fn a_blocked_pr_says_so_unless_its_state_already_does() {
        use crate::item::Blocker;
        let mut review = pr(
            "review",
            "a",
            Kind::Review,
            State::ActionRequired(Reason::ToReview),
        );
        review.blockers = vec![Blocker::Conflicts, Blocker::Checks(vec!["osv".to_owned()])];
        let mut mine = pr(
            "ci",
            "a",
            Kind::MyPr,
            State::ActionRequired(Reason::CiFailed),
        );
        mine.blockers = vec![Blocker::Checks(Vec::new())];
        let snapshot = snapshot_of(&[review, mine], &typed("état:bloquée", &[]));
        let items: Vec<(&str, Option<&str>, Vec<&str>)> = snapshot
            .page
            .groups
            .iter()
            .flat_map(|group| &group.items)
            .map(|item| {
                (
                    item.id.as_str(),
                    item.blocked_label.as_deref(),
                    item.details.blockers.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            items,
            [
                (
                    "review",
                    Some("conflits, CI rouge"),
                    vec!["Conflits avec la branche de base", "Checks en échec : osv"]
                ),
                ("ci", None, vec!["Des checks échouent"]),
            ]
        );
    }

    #[test]
    fn a_day_shows_its_projects_and_the_others_wait_on_the_home_page() {
        use crate::config::Day;
        let mut config = config();
        config.defaults.by_day = true;
        config.projects[0].days = vec![Day::Mon, Day::Thu];
        config.projects[3].days = vec![Day::Tue];
        let mut late = item("td:late", "a");
        late.due = Some(Due::Date(date("2026-11-06")));
        let items = [late, item("td:b", "b"), item("td:other", OTHERS)];
        let on = |day| build(&items, &config, &everything(), day, now(), Vec::new(), None);

        let tuesday = on(Some(Day::Tue));
        assert_eq!(
            tuesday.day_bar,
            Some(DayBar {
                day: Some(Day::Tue)
            })
        );
        assert_eq!(tuesday.sidebar.projects_title, "Projets du mardi");
        let entries: Vec<&str> = tuesday
            .sidebar
            .projects
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(
            entries,
            ["projects", "project:b", "project:empty", "project:others"]
        );
        let listed: Vec<&str> = tuesday
            .page
            .groups
            .iter()
            .flat_map(|group| &group.items)
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(listed, ["td:b", "td:other"]);
        let day = tuesday.home.day.unwrap();
        assert_eq!(day.title, "Mardi");
        assert_eq!(day.subtitle, "Empty · et B tous les jours");
        let resting: Vec<(&str, Option<&str>, Option<&str>)> = day
            .resting
            .iter()
            .map(|project| {
                (
                    project.name.as_str(),
                    project.urgent.as_deref(),
                    project.next.as_deref(),
                )
            })
            .collect();
        // The late task of A presses even on a day without A.
        assert_eq!(resting, [("A", Some("en retard de 2 j"), Some("jeu."))]);

        // Every day, or the setting off: every project.
        assert_eq!(on(None).sidebar.projects.len(), 5);
        assert!(on(None).home.day.is_none());
        config.defaults.by_day = false;
        let off = build(
            &items,
            &config,
            &everything(),
            Some(Day::Tue),
            now(),
            Vec::new(),
            None,
        );
        assert_eq!(off.day_bar, None);
        assert_eq!(off.sidebar.projects_title, "Projets");
        assert_eq!(off.sidebar.projects.len(), 5);
    }

    #[test]
    fn the_sidebar_counts_what_each_view_keeps() {
        let mut config = config();
        config.views.push(crate::config::SavedView {
            id: "ci".to_owned(),
            name: "CI".to_owned(),
            query: "état:ci-rouge".to_owned(),
            group: vec![GroupBy::Project],
            sort: SortBy::default(),
        });
        let snapshot = build(
            &pull_requests(),
            &config,
            &everything(),
            None,
            now(),
            Vec::new(),
            None,
        );
        let views: Vec<(&str, u32, bool)> = snapshot
            .sidebar
            .views
            .iter()
            .map(|entry| (entry.id.as_str(), entry.count, entry.saved))
            .collect();
        assert_eq!(
            views,
            [
                // The dependency update waits for me like any review.
                ("view:action", 3, false),
                ("view:today", 0, false),
                ("view:reviews", 2, false),
                ("view:prs", 5, false),
                ("saved:ci", 1, true),
            ]
        );
        let projects: Vec<(&str, u32)> = snapshot
            .sidebar
            .projects
            .iter()
            .map(|entry| (entry.id.as_str(), entry.count))
            .collect();
        assert_eq!(
            projects,
            [
                ("projects", 3),
                ("project:b", 1),
                ("project:a", 2),
                ("project:empty", 0)
            ]
        );
        assert_eq!(snapshot.sidebar.projects[0].colors.len(), 3);
    }

    #[test]
    fn the_home_page_shows_what_presses_and_what_has_a_date() {
        let mut items = pull_requests();
        let mut late = item("td:late", "a");
        late.due = Some(Due::Date(date("2026-11-06")));
        late.priority = Priority::P1;
        let mut soon = item("td:soon", "b");
        soon.due = Some(Due::Date(date("2026-11-10")));
        let mut far = item("td:far", "b");
        far.due = Some(Due::Date(date("2026-12-24")));
        items.extend([late, soon, far]);

        let home = snapshot(&items).home;

        let stats: Vec<(&str, u32, bool)> = home
            .stats
            .iter()
            .map(|stat| (stat.label.as_str(), stat.count, stat.alert))
            .collect();
        assert_eq!(
            stats,
            [
                ("à traiter", 3, false),
                ("en retard", 1, true),
                ("pour aujourd'hui", 0, false),
                ("reviews qui attendent", 2, false),
            ]
        );
        assert_eq!(home.start_count, 3);

        let cards: Vec<(&str, u32)> = home
            .cards
            .iter()
            .map(|card| (card.name.as_str(), card.action_count))
            .collect();
        assert_eq!(cards, [("B", 1), ("A", 2)]);
        let a = &home.cards[1];
        let lines: Vec<(&str, &str)> = a
            .lines
            .iter()
            .map(|line| (line.tag.as_str(), line.title.as_str()))
            .collect();
        // The late task first, then the red CI, then the dependency update that waits for me.
        assert_eq!(
            lines,
            [
                ("en retard de 2 j", "td:late"),
                ("CI rouge", "ci"),
                ("à relire", "bot"),
            ]
        );
        assert_eq!(a.lines[0].tone.as_deref(), Some("red"));
        assert_eq!(
            home.cards[0].footer.as_deref(),
            Some("Prochaine échéance : td:soon, mar.")
        );

        let deadlines: Vec<(&str, &str, &str, bool)> = home
            .deadlines
            .iter()
            .map(|deadline| {
                (
                    deadline.item_id.as_str(),
                    deadline.big.as_str(),
                    deadline.small.as_str(),
                    deadline.overdue,
                )
            })
            .collect();
        assert_eq!(
            deadlines,
            [
                ("td:late", "−2", "jours", true),
                ("td:soon", "10", "mar.", false),
                ("td:far", "24", "déc.", false),
            ]
        );
        assert_eq!(
            home.undated.as_deref(),
            Some("6 éléments sur 9 n'ont pas de date : ils restent dans les cartes.")
        );
    }
}
