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

    /// The fields a view can filter and sort on. `overdue` and `due_today` stay absent without a
    /// date, so that "no date" is never mistaken for "not late"; `overdue_rank` exists for sorting,
    /// where an undated P1 must still come before a dated P4.
    #[must_use]
    pub fn to_record(&self, now: DateTime<FixedOffset>) -> Record {
        let overdue = self.is_overdue(now);
        let mut fields: std::collections::BTreeMap<String, Value> = [
            ("project", json!(self.project)),
            ("kind", json!(self.kind)),
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
