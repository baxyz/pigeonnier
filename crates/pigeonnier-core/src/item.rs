// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one shape every source ends up in: a priority, a due date and a state.

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::map::Record;
use crate::source::Provenance;

/// The virtual project of what no configured project claims.
pub const OTHERS: &str = "others";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Review,
    MyPr,
    ClaudeAction,
    Todo,
    Roadmap,
    Issue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Priority {
    P1,
    P2,
    P3,
    P4,
}

impl Priority {
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::P1 => 1,
            Self::P2 => 2,
            Self::P3 => 3,
            Self::P4 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    ToReview,
    ChangesRequested,
    CiFailed,
    Conflicts,
    ToMerge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    ActionRequired(Reason),
    Waiting,
    Planned,
}

/// What keeps a pull request from being merged, whoever has to act on it: a review asked of me says
/// nothing of the checks, so this is kept beside the state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    Conflicts,
    /// The names of the checks that failed; empty when GitHub only gave the overall result.
    Checks(Vec<String>),
}

impl Blocker {
    /// A word or two, for a badge.
    #[must_use]
    pub fn short(&self) -> &'static str {
        match self {
            Self::Conflicts => "conflits",
            Self::Checks(_) => "CI rouge",
        }
    }

    /// A sentence, for the detail pane.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::Conflicts => "Conflits avec la branche de base".to_owned(),
            Self::Checks(names) if names.is_empty() => "Des checks échouent".to_owned(),
            Self::Checks(names) => format!("Checks en échec : {}", names.join(", ")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Due {
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
}

impl Due {
    #[must_use]
    pub fn local_date(self, offset: FixedOffset) -> NaiveDate {
        match self {
            Self::Date(date) => date,
            Self::DateTime(time) => time.with_timezone(&offset).date_naive(),
        }
    }

    fn sort_text(self) -> String {
        match self {
            Self::Date(date) => date.to_string(),
            Self::DateTime(time) => time.to_rfc3339(),
        }
    }

    fn is_past(self, now: DateTime<FixedOffset>) -> bool {
        match self {
            Self::Date(date) => date < now.date_naive(),
            Self::DateTime(time) => time < now.with_timezone(&Utc),
        }
    }
}

/// How long a task is meant to take, in the unit the service keeps it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Duration {
    Minutes(u64),
    Days(u64),
}

/// What a task says beyond its title, shown when it is unfolded. Only Todoist tasks have it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Details {
    /// Markdown, as written in the service.
    pub description: String,
    /// "Projet / Section", where the task lives in the service.
    pub place: String,
    pub labels: Vec<String>,
    /// The date as typed, for a recurring task: "every monday".
    pub recurrence: Option<String>,
    pub duration: Option<Duration>,
    pub added_at: Option<DateTime<Utc>>,
    /// `None` when the service does not say: the comments are then read to find out.
    pub comment_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// `gh-pr:helpers4/devcontainer#42`, `td:8812345`, `gh-pv2:PVTI_…`
    pub id: String,
    pub project: String,
    pub kind: Kind,
    pub title: String,
    pub context: String,
    pub url: String,
    pub priority: Priority,
    pub due: Option<Due>,
    pub deadline: Option<NaiveDate>,
    pub state: State,
    pub updated_at: DateTime<Utc>,
    /// When the review was asked for, which is what the age of a review counts from.
    pub requested_at: Option<DateTime<Utc>>,
    pub bot: bool,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub details: Option<Details>,
    /// A pull request not ready for review yet.
    pub draft: bool,
    /// What keeps a pull request from being merged; always empty for anything else.
    pub blockers: Vec<Blocker>,
    pub provenance: Provenance,
}

impl Item {
    /// A deadline wins over a planned date: it is the one that hurts when missed.
    #[must_use]
    pub fn effective_due(&self) -> Option<Due> {
        self.deadline.map(Due::Date).or(self.due)
    }

    #[must_use]
    pub fn is_overdue(&self, now: DateTime<FixedOffset>) -> bool {
        self.effective_due().is_some_and(|due| due.is_past(now))
    }

    #[must_use]
    pub fn is_snoozed(&self, now: DateTime<FixedOffset>) -> bool {
        self.snoozed_until
            .is_some_and(|until| until > now.with_timezone(&Utc))
    }

    /// What the item is, whichever search found it: `pr`, `issue`, `task` or `roadmap`.
    #[must_use]
    pub fn type_id(&self) -> &'static str {
        match self.kind {
            Kind::Review | Kind::MyPr => "pr",
            Kind::Issue => "issue",
            Kind::ClaudeAction | Kind::Todo => "task",
            Kind::Roadmap => "roadmap",
        }
    }

    /// The one state the item is in: `review`, `changes`, `ci`, `conflicts`, `merge`, `waiting`,
    /// `draft` or `planned`.
    #[must_use]
    pub fn state_id(&self) -> &'static str {
        match self.state {
            State::ActionRequired(Reason::ToReview) => "review",
            State::ActionRequired(Reason::ChangesRequested) => "changes",
            State::ActionRequired(Reason::CiFailed) => "ci",
            State::ActionRequired(Reason::Conflicts) => "conflicts",
            State::ActionRequired(Reason::ToMerge) => "merge",
            State::Waiting if self.draft => "draft",
            State::Waiting => "waiting",
            State::Planned => "planned",
        }
    }

    /// Where the due date stands: `late`, `today`, `week` (the next seven days), `later`, or `none`
    /// without a date. Unlike `overdue`, it is always there: "no date" is a value one can ask for.
    #[must_use]
    pub fn due_id(&self, now: DateTime<FixedOffset>) -> &'static str {
        let Some(due) = self.effective_due() else {
            return "none";
        };
        let days = (due.local_date(*now.offset()) - now.date_naive()).num_days();
        match days {
            _ if self.is_overdue(now) => "late",
            0 => "today",
            1..=7 => "week",
            _ => "later",
        }
    }

    #[must_use]
    pub fn is_todoist(&self) -> bool {
        self.id.starts_with("td:")
    }

    /// The fields a view can filter and sort on. `overdue` and `due_today` stay absent without a
    /// date, so that "no date" is never mistaken for "not late"; `overdue_rank` exists for sorting,
    /// where an undated P1 must still come before a dated P4.
    #[must_use]
    pub fn to_record(&self, now: DateTime<FixedOffset>) -> Record {
        let overdue = self.is_overdue(now);
        let mut fields: std::collections::BTreeMap<String, Value> = [
            ("project", json!(self.project)),
            ("kind", json!(self.kind)),
            ("type", json!(self.type_id())),
            ("state", json!(self.state_id())),
            ("due", json!(self.due_id(now))),
            (
                "source",
                json!(if self.is_todoist() {
                    "todoist"
                } else {
                    "github"
                }),
            ),
            ("title", json!(self.title)),
            ("priority", json!(self.priority.rank())),
            (
                "action_required",
                json!(matches!(self.state, State::ActionRequired(_))),
            ),
            ("waiting", json!(self.state == State::Waiting)),
            ("snoozed", json!(self.is_snoozed(now))),
            ("bot", json!(self.bot)),
            ("updated", json!(self.updated_at.to_rfc3339())),
            ("overdue_rank", json!(u8::from(overdue))),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        if self.updated_at != DateTime::<Utc>::default() {
            let age = (now.with_timezone(&Utc) - self.updated_at).num_days();
            fields.insert("age_days".to_owned(), json!(age.max(0)));
        }
        // Only a pull request can be blocked: for the rest the field stays absent, not false.
        if self.type_id() == "pr" {
            fields.insert("blocked".to_owned(), json!(!self.blockers.is_empty()));
        }
        if let Some(due) = self.effective_due() {
            fields.insert("effective_due".to_owned(), json!(due.sort_text()));
            fields.insert("overdue".to_owned(), json!(overdue));
            let today = due.local_date(*now.offset()) == now.date_naive();
            fields.insert("due_today".to_owned(), json!(today));
        }
        Record {
            provenance: Provenance {
                source: self.provenance.source,
                id: self.id.clone(),
                version: self.provenance.version.clone(),
            },
            fields,
        }
    }
}
