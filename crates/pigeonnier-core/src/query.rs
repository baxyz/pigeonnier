// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the filter bar holds, as data: filters that combine, a grouping and an order. A query can
//! be typed (`type:pr état:ci-rouge,conflits -auteur:robot`), saved, and is turned into the
//! [`Predicate`] and [`SortKey`]s the filter and sort steps already run.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{Config, SavedView};
use crate::discover::slug;
use crate::filter::Predicate;
use crate::item::OTHERS;
use crate::sort::{Absent, Order, SortKey};
use crate::view::View;

/// What a filter looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Project,
    Type,
    Source,
    State,
    Priority,
    Due,
    Author,
    Updated,
}

/// A value a filter can take: how it is typed, how it is shown and what it selects.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub predicate: Predicate,
}

fn is(field: &str, value: serde_json::Value) -> Predicate {
    Predicate::Is(field.to_owned(), value)
}

fn choice(id: &str, label: &str, predicate: Predicate) -> Choice {
    Choice {
        id: id.to_owned(),
        label: label.to_owned(),
        predicate,
    }
}

impl Field {
    pub const ALL: [Self; 8] = [
        Self::Project,
        Self::Type,
        Self::Source,
        Self::State,
        Self::Priority,
        Self::Due,
        Self::Author,
        Self::Updated,
    ];

    /// The word typed before the colon.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Project => "projet",
            Self::Type => "type",
            Self::Source => "source",
            Self::State => "état",
            Self::Priority => "priorité",
            Self::Due => "échéance",
            Self::Author => "auteur",
            Self::Updated => "modifié",
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Projet",
            Self::Type => "Type",
            Self::Source => "Source",
            Self::State => "État",
            Self::Priority => "Priorité",
            Self::Due => "Échéance",
            Self::Author => "Auteur",
            Self::Updated => "Mis à jour",
        }
    }

    /// Only one value at a time makes sense, and "is not" does not.
    #[must_use]
    pub fn single(self) -> bool {
        self == Self::Updated
    }

    fn from_key(text: &str) -> Option<Self> {
        let typed = slug(text);
        Self::ALL
            .into_iter()
            .find(|field| slug(field.key()) == typed)
    }

    /// The values the field can take, in the order they are offered.
    #[must_use]
    pub fn choices(self, config: &Config) -> Vec<Choice> {
        match self {
            Self::Project => config
                .projects
                .iter()
                .filter(|project| !project.muted)
                .map(|project| (project.id.as_str(), project.name.as_str()))
                .chain([(OTHERS, "Autres")])
                .map(|(id, name)| choice(id, name, is("project", json!(id))))
                .collect(),
            Self::Type => vec![
                choice("pr", "PR", is("type", json!("pr"))),
                choice("issue", "Issue", is("type", json!("issue"))),
                choice("tâche", "Tâche", is("type", json!("task"))),
                choice("roadmap", "Roadmap", is("type", json!("roadmap"))),
            ],
            Self::Source => vec![
                choice("github", "GitHub", is("source", json!("github"))),
                choice("todoist", "Todoist", is("source", json!("todoist"))),
            ],
            Self::State => vec![
                choice("à-traiter", "à traiter", is("action_required", json!(true))),
                choice("à-relire", "à relire", is("state", json!("review"))),
                choice("à-corriger", "à corriger", is("state", json!("changes"))),
                choice("ci-rouge", "CI rouge", is("state", json!("ci"))),
                choice("conflits", "conflits", is("state", json!("conflicts"))),
                choice("bloquée", "bloquée", is("blocked", json!(true))),
                choice("à-merger", "à merger", is("state", json!("merge"))),
                choice("en-attente", "en attente", is("state", json!("waiting"))),
                choice("brouillon", "brouillon", is("state", json!("draft"))),
                choice("planifié", "planifié", is("state", json!("planned"))),
            ],
            Self::Priority => (1..=4)
                .map(|rank| {
                    choice(
                        &format!("p{rank}"),
                        &format!("P{rank}"),
                        is("priority", json!(rank)),
                    )
                })
                .collect(),
            Self::Due => vec![
                choice("en-retard", "en retard", is("due", json!("late"))),
                choice("aujourd'hui", "aujourd'hui", is("due", json!("today"))),
                choice(
                    "7-jours",
                    "dans les 7 jours",
                    Predicate::Or(vec![is("due", json!("today")), is("due", json!("week"))]),
                ),
                choice("plus-tard", "plus tard", is("due", json!("later"))),
                choice("sans-date", "sans date", is("due", json!("none"))),
            ],
            Self::Author => vec![
                choice("personne", "une personne", is("bot", json!(false))),
                choice("robot", "un robot", is("bot", json!(true))),
            ],
            Self::Updated => [(1, "1 jour"), (7, "7 jours"), (30, "30 jours")]
                .into_iter()
                .map(|(days, label)| {
                    choice(
                        &format!("{days}j"),
                        label,
                        Predicate::Max("age_days".to_owned(), days),
                    )
                })
                .collect(),
        }
    }
}

/// One filter of the bar: a field, the values it accepts, and whether it keeps or leaves them out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub field: Field,
    pub values: Vec<String>,
    #[serde(default)]
    pub negate: bool,
}

impl Filter {
    #[must_use]
    pub fn new(field: Field, values: &[&str]) -> Self {
        Self {
            field,
            values: values.iter().map(|value| (*value).to_owned()).collect(),
            negate: false,
        }
    }

    fn predicate(&self, config: &Config) -> Predicate {
        let choices = self.field.choices(config);
        let any = Predicate::Or(
            self.values
                .iter()
                .filter_map(|value| choices.iter().find(|choice| choice.id == *value))
                .map(|choice| choice.predicate.clone())
                .collect(),
        );
        if self.negate {
            Predicate::Not(Box::new(any))
        } else {
            any
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// Words looked for in the title and the context.
    #[serde(default)]
    pub text: String,
    /// Also shows what was put off.
    #[serde(default)]
    pub snoozed: bool,
}

impl Query {
    /// Reads a typed query. A word with a known field before its colon is a filter, a leading dash
    /// leaves its values out; everything else is text to look for.
    #[must_use]
    pub fn parse(text: &str, config: &Config) -> Self {
        let mut query = Self::default();
        let mut words = Vec::new();
        for word in text.split_whitespace() {
            match parse_filter(word, config) {
                Some(filter) => query.set(filter),
                None => words.push(word),
            }
        }
        query.text = words.join(" ");
        query
    }

    /// The query as it can be typed again.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut parts: Vec<String> = self
            .filters
            .iter()
            .map(|filter| {
                format!(
                    "{}{}:{}",
                    if filter.negate { "-" } else { "" },
                    filter.field.key(),
                    filter.values.join(",")
                )
            })
            .collect();
        if !self.text.is_empty() {
            parts.push(self.text.clone());
        }
        parts.join(" ")
    }

    #[must_use]
    pub fn filter(&self, field: Field) -> Option<&Filter> {
        self.filters.iter().find(|filter| filter.field == field)
    }

    /// Puts a filter in place of the one on the same field; without values, removes it.
    pub fn set(&mut self, filter: Filter) {
        let position = self
            .filters
            .iter()
            .position(|known| known.field == filter.field);
        match (position, filter.values.is_empty()) {
            (Some(index), true) => {
                self.filters.remove(index);
            }
            (Some(index), false) => self.filters[index] = filter,
            (None, true) => {}
            (None, false) => self.filters.push(filter),
        }
    }

    #[must_use]
    pub fn without(&self, field: Field) -> Self {
        let mut query = self.clone();
        query.filters.retain(|filter| filter.field != field);
        query
    }

    /// What the filters select; the text is matched apart, on what is shown.
    #[must_use]
    pub fn predicate(&self, config: &Config) -> Predicate {
        let mut all = Vec::new();
        if !self.snoozed {
            all.push(is("snoozed", json!(false)));
        }
        all.extend(self.filters.iter().map(|filter| filter.predicate(config)));
        Predicate::And(all)
    }

    /// Whether every word of the text is in `title` or `context`.
    #[must_use]
    pub fn matches_text(&self, title: &str, context: &str) -> bool {
        let (title, context) = (title.to_lowercase(), context.to_lowercase());
        self.text
            .to_lowercase()
            .split_whitespace()
            .all(|word| title.contains(word) || context.contains(word))
    }
}

fn parse_filter(word: &str, config: &Config) -> Option<Filter> {
    let (negate, word) = word
        .strip_prefix('-')
        .map_or((false, word), |rest| (true, rest));
    let (key, values) = word.split_once(':')?;
    let field = Field::from_key(key)?;
    let choices = field.choices(config);
    let mut known: Vec<String> = Vec::new();
    for value in values.split(',') {
        let typed = slug(value);
        let found = choices
            .iter()
            .find(|choice| slug(&choice.id) == typed || slug(&choice.label) == typed)?;
        if !known.contains(&found.id) {
            known.push(found.id.clone());
        }
    }
    if field.single() {
        known.truncate(1);
    }
    Some(Filter {
        field,
        values: known,
        negate: negate && !field.single(),
    })
}

/// What the headings of a list stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Project,
    State,
    Type,
    Due,
    Priority,
}

impl GroupBy {
    pub const ALL: [Self; 5] = [
        Self::Project,
        Self::State,
        Self::Type,
        Self::Due,
        Self::Priority,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Projet",
            Self::State => "État",
            Self::Type => "Type",
            Self::Due => "Échéance",
            Self::Priority => "Priorité",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    /// What waits for others last, then what is late, the priority, the date and the last change.
    #[default]
    Urgency,
    Priority,
    Due,
    Updated,
}

impl SortBy {
    pub const ALL: [Self; 4] = [Self::Urgency, Self::Priority, Self::Due, Self::Updated];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Urgency => "Urgence",
            Self::Priority => "Priorité",
            Self::Due => "Échéance",
            Self::Updated => "Dernière modification",
        }
    }

    #[must_use]
    pub fn keys(self) -> Vec<SortKey> {
        let key = |field: &str, order| SortKey {
            field: field.to_owned(),
            order,
            absent: Absent::Last,
        };
        let urgency = [
            key("waiting", Order::Asc),
            key("overdue_rank", Order::Desc),
            key("priority", Order::Asc),
            key("effective_due", Order::Asc),
            key("updated", Order::Desc),
        ];
        let first = match self {
            Self::Urgency => None,
            Self::Priority => Some(key("priority", Order::Asc)),
            Self::Due => Some(key("effective_due", Order::Asc)),
            Self::Updated => Some(key("updated", Order::Desc)),
        };
        first.into_iter().chain(urgency).collect()
    }
}

/// Everything a list needs: what to keep, how to group it and how to order it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewSpec {
    #[serde(default)]
    pub query: Query,
    /// One or two levels of headings; none for a flat list.
    #[serde(default)]
    pub group: Vec<GroupBy>,
    #[serde(default)]
    pub sort: SortBy,
}

impl ViewSpec {
    #[must_use]
    pub fn new(query: Query, group: &[GroupBy]) -> Self {
        Self {
            query,
            group: group.to_vec(),
            sort: SortBy::default(),
        }
    }

    /// The filter and the order, for the filter and sort steps to run.
    #[must_use]
    pub fn view(&self, config: &Config) -> View {
        View {
            id: String::new(),
            title: String::new(),
            filter: Some(self.query.predicate(config)),
            sort: self.sort.keys(),
        }
    }

    #[must_use]
    pub fn from_saved(saved: &SavedView, config: &Config) -> Self {
        Self {
            query: Query::parse(&saved.query, config),
            group: saved.group.iter().copied().take(2).collect(),
            sort: saved.sort,
        }
    }

    #[must_use]
    pub fn to_saved(&self, name: &str) -> SavedView {
        SavedView {
            id: slug(name),
            name: name.to_owned(),
            query: self.query.to_text(),
            group: self.group.clone(),
            sort: self.sort,
        }
    }
}

/// A view that ships with the app: `(id, title, icon, query, grouping)`.
pub const PRESETS: [(&str, &str, &str, &str, &[GroupBy]); 4] = [
    (
        "action",
        "À traiter",
        "mail-unread-symbolic",
        "état:à-traiter -auteur:robot",
        &[GroupBy::Project],
    ),
    (
        "today",
        "Aujourd'hui",
        "weather-clear-symbolic",
        "échéance:en-retard,aujourd'hui",
        &[GroupBy::Project],
    ),
    (
        "reviews",
        "Mes reviews",
        "avatar-default-symbolic",
        "état:à-relire -auteur:robot",
        &[GroupBy::Project],
    ),
    (
        "prs",
        "Pull requests",
        "media-playlist-shuffle-symbolic",
        "type:pr",
        &[GroupBy::State],
    ),
];

/// The view of a preset, or of everything when the preset is unknown.
#[must_use]
pub fn preset(id: &str, config: &Config) -> ViewSpec {
    PRESETS
        .iter()
        .find(|preset| preset.0 == id)
        .map_or_else(ViewSpec::default, |(_, _, _, query, group)| {
            ViewSpec::new(Query::parse(query, config), group)
        })
}

/// Everything, project by project.
#[must_use]
pub fn everything() -> ViewSpec {
    ViewSpec::new(Query::default(), &[GroupBy::Project])
}

/// One project, by what each item is.
#[must_use]
pub fn project(id: &str) -> ViewSpec {
    let mut query = Query::default();
    query.set(Filter::new(Field::Project, &[id]));
    ViewSpec::new(query, &[GroupBy::Type])
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::filter::Truth;
    use crate::map::Record;
    use crate::source::Provenance;

    fn config() -> Config {
        Config::parse(
            "[[project]]\nid = \"sunny-tech\"\nname = \"Sunny Tech\"\n",
            "test",
        )
        .unwrap()
    }

    fn record(fields: Value) -> Record {
        Record {
            provenance: Provenance {
                source: "test",
                id: "1".to_owned(),
                version: None,
            },
            fields: serde_json::from_value(fields).unwrap(),
        }
    }

    #[test]
    fn a_typed_query_becomes_filters_and_text() {
        let query = Query::parse(
            "type:pr Etat:ci-rouge,Conflits -auteur:robot projet:sunny-tech zen notes",
            &config(),
        );
        assert_eq!(
            query.filters,
            [
                Filter::new(Field::Type, &["pr"]),
                Filter::new(Field::State, &["ci-rouge", "conflits"]),
                Filter {
                    negate: true,
                    ..Filter::new(Field::Author, &["robot"])
                },
                Filter::new(Field::Project, &["sunny-tech"]),
            ]
        );
        assert_eq!(query.text, "zen notes");
        assert_eq!(
            query.to_text(),
            "type:pr état:ci-rouge,conflits -auteur:robot projet:sunny-tech zen notes"
        );
        assert_eq!(Query::parse(&query.to_text(), &config()), query);
    }

    #[test]
    fn what_is_not_a_known_filter_stays_text() {
        let query = Query::parse("label:wontfix état:inconnu https://x.test", &config());
        assert_eq!(query.filters, []);
        assert_eq!(query.text, "label:wontfix état:inconnu https://x.test");
    }

    #[test]
    fn a_filter_typed_twice_keeps_the_last_one() {
        let query = Query::parse("type:pr type:issue,tache", &config());
        assert_eq!(
            query.filters,
            [Filter::new(Field::Type, &["issue", "tâche"])]
        );
    }

    #[test]
    fn filters_combine_and_values_alternate() {
        let query = Query::parse("type:pr état:ci-rouge,conflits", &config());
        let predicate = query.predicate(&config());
        let eval = |kind: &str, state: &str| {
            predicate.eval(&record(
                json!({ "snoozed": false, "type": kind, "state": state }),
            ))
        };
        assert_eq!(eval("pr", "ci"), Truth::True);
        assert_eq!(eval("pr", "conflicts"), Truth::True);
        assert_eq!(eval("pr", "merge"), Truth::False);
        assert_eq!(eval("issue", "ci"), Truth::False);
    }

    #[test]
    fn leaving_out_an_undated_item_is_decided() {
        // "No date" is a value of its own here, so "not late" keeps what has no date.
        let query = Query::parse("-échéance:en-retard", &config());
        let predicate = query.predicate(&config());
        let eval = |due: &str| predicate.eval(&record(json!({ "snoozed": false, "due": due })));
        assert_eq!(eval("none"), Truth::True);
        assert_eq!(eval("late"), Truth::False);
    }

    #[test]
    fn snoozed_items_are_left_out_unless_asked_for() {
        let snoozed = record(json!({ "snoozed": true }));
        let mut query = Query::default();
        assert_eq!(query.predicate(&config()).eval(&snoozed), Truth::False);
        query.snoozed = true;
        assert_eq!(query.predicate(&config()).eval(&snoozed), Truth::True);
    }

    #[test]
    fn the_text_is_looked_for_word_by_word() {
        let query = Query::parse("ZEN mozlz4", &config());
        assert!(query.matches_text("Lire les notes Zen", "mozlz4#41"));
        assert!(!query.matches_text("Lire les notes Zen", "tool#1"));
    }

    #[test]
    fn urgency_sorts_late_then_priority_and_waiting_last() {
        let base = |waiting: bool, late: i32, priority: i32| json!({ "snoozed": false, "waiting": waiting, "overdue_rank": late, "priority": priority });
        let record = |id: &str, fields: Value| Record {
            provenance: Provenance {
                source: "test",
                id: id.to_owned(),
                version: None,
            },
            fields: serde_json::from_value(fields).unwrap(),
        };
        let result = everything().view(&config()).run(vec![
            record("waiting-p1", base(true, 1, 1)),
            record("p3", base(false, 0, 3)),
            record("p1-undated", base(false, 0, 1)),
            record("late-p4", base(false, 1, 4)),
        ]);
        let ids: Vec<&str> = result
            .kept
            .iter()
            .map(|record| record.provenance.id.as_str())
            .collect();
        assert_eq!(ids, ["late-p4", "p1-undated", "p3", "waiting-p1"]);
    }

    #[test]
    fn a_saved_view_keeps_its_query_as_typed() {
        let spec = ViewSpec {
            query: Query::parse("type:pr état:à-relire", &config()),
            group: vec![GroupBy::State, GroupBy::Project],
            sort: SortBy::Priority,
        };
        let saved = spec.to_saved("Revue du lundi");
        assert_eq!(saved.id, "revue-du-lundi");
        assert_eq!(saved.query, "type:pr état:à-relire");
        assert_eq!(ViewSpec::from_saved(&saved, &config()), spec);
    }

    #[test]
    fn the_presets_parse_to_filters_only() {
        for (id, _, _, text, _) in PRESETS {
            let spec = preset(id, &config());
            assert_eq!(spec.query.text, "", "{id}: {text}");
            assert_ne!(spec.query.filters, [], "{id}");
        }
    }
}
