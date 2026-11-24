// SPDX-License-Identifier: AGPL-3.0-or-later

//! A list on screen, as data: what to keep and how to order it. [`crate::query`] builds one from
//! what the filter bar holds.

use serde::{Deserialize, Serialize};

use crate::filter::{Predicate, Truth};
use crate::map::Record;
use crate::sort::{SortKey, sort};

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
