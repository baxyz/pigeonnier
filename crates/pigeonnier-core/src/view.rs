// SPDX-License-Identifier: AGPL-3.0-or-later

//! A list on screen, as data: what to keep and how to order it. Default views ship with the
//! application, and a user can add their own in `views.json`.

use serde::{Deserialize, Serialize};

use crate::filter::{Predicate, Truth};
use crate::map::Record;
use crate::sort::{SortKey, sort};

const DEFAULT_VIEWS: &str = include_str!("../views/default.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub filter: Option<Predicate>,
    #[serde(default)]
    pub sort: Vec<SortKey>,
}

/// What a view kept, and what it could not decide on. `unknown` is kept apart so a caller can show
/// it as "maybe" or leave it out; it is never counted as rejected.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewResult {
    pub kept: Vec<Record>,
    pub unknown: Vec<Record>,
    pub rejected: usize,
}

impl View {
    #[must_use]
    pub fn run(&self, records: Vec<Record>) -> ViewResult {
        let mut result = ViewResult {
            kept: Vec::new(),
            unknown: Vec::new(),
            rejected: 0,
        };
        for record in records {
            let truth = self
                .filter
                .as_ref()
                .map_or(Truth::True, |filter| filter.eval(&record));
            match truth {
                Truth::True => result.kept.push(record),
                Truth::Unknown => result.unknown.push(record),
                Truth::False => result.rejected += 1,
            }
        }
        sort(&mut result.kept, &self.sort);
        sort(&mut result.unknown, &self.sort);
        result
    }
}

/// The views that ship with the application.
///
/// # Panics
/// Never at run time: a test parses the embedded file.
#[must_use]
pub fn default_views() -> Vec<View> {
    serde_json::from_str(DEFAULT_VIEWS).expect("the embedded views are valid")
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::source::Provenance;

    fn record(id: &str, fields: Value) -> Record {
        Record {
            provenance: Provenance {
                source: "test",
                id: id.to_owned(),
                version: None,
            },
            fields: serde_json::from_value(fields).unwrap(),
        }
    }

    fn view(id: &str) -> View {
        default_views()
            .into_iter()
            .find(|view| view.id == id)
            .unwrap()
    }

    fn ids(records: &[Record]) -> Vec<&str> {
        records
            .iter()
            .map(|record| record.provenance.id.as_str())
            .collect()
    }

    #[test]
    fn the_default_views_parse() {
        let ids: Vec<_> = default_views().into_iter().map(|view| view.id).collect();
        assert_eq!(ids, ["all", "action", "today"]);
    }

    #[test]
    fn today_leaves_undated_items_undecided() {
        let result = view("today").run(vec![
            record("late", json!({ "snoozed": false, "overdue": true })),
            record(
                "today",
                json!({ "snoozed": false, "overdue": false, "due_today": true }),
            ),
            record(
                "later",
                json!({ "snoozed": false, "overdue": false, "due_today": false }),
            ),
            record("undated", json!({ "snoozed": false })),
            record("snoozed", json!({ "snoozed": true, "overdue": true })),
        ]);
        assert_eq!(ids(&result.kept).len(), 2);
        assert_eq!(ids(&result.unknown), ["undated"]);
        assert_eq!(result.rejected, 2);
    }

    #[test]
    fn urgency_sorts_late_then_priority_and_waiting_last() {
        let base = |waiting: bool, late: i32, priority: i32| json!({ "snoozed": false, "waiting": waiting, "overdue_rank": late, "priority": priority });
        let result = view("all").run(vec![
            record("waiting-p1", base(true, 1, 1)),
            record("p3", base(false, 0, 3)),
            record("p1-undated", base(false, 0, 1)),
            record("late-p4", base(false, 1, 4)),
        ]);
        assert_eq!(
            ids(&result.kept),
            ["late-p4", "p1-undated", "p3", "waiting-p1"]
        );
    }
}
