// SPDX-License-Identifier: AGPL-3.0-only

//! A list on screen, as data: how to map, what to keep, how to order.

use crate::filter::{Predicate, Truth};
use crate::map::{Mapping, Record};
use crate::sort::{SortKey, sort};
use crate::source::RawItem;

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub mapping: Mapping,
    pub filter: Option<Predicate>,
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
    pub fn run(&self, raw: &[RawItem]) -> ViewResult {
        let mut result = ViewResult {
            kept: Vec::new(),
            unknown: Vec::new(),
            rejected: 0,
        };
        for record in raw.iter().map(|item| self.mapping.apply(item)) {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sort::{Absent, Order};
    use crate::source::Provenance;

    fn raw(id: &str, payload: serde_json::Value) -> RawItem {
        RawItem {
            provenance: Provenance {
                source: "test",
                id: id.to_owned(),
                version: None,
            },
            payload,
        }
    }

    #[test]
    fn the_steps_compose() {
        let view = View {
            mapping: Mapping::new(&[("due", "/due/date"), ("done", "/checked")]),
            filter: Some(Predicate::before("due", "2026-11-08")),
            sort: vec![SortKey::new("due", Order::Ascending, Absent::Last)],
        };
        let result = view.run(&[
            raw("b", json!({ "due": { "date": "2026-11-07" } })),
            raw("a", json!({ "due": { "date": "2026-11-01" } })),
            raw("later", json!({ "due": { "date": "2026-12-01" } })),
            raw("no-date", json!({ "due": null })),
        ]);
        let kept: Vec<_> = result
            .kept
            .iter()
            .map(|record| record.provenance.id.as_str())
            .collect();
        assert_eq!(kept, ["a", "b"]);
        assert_eq!(result.unknown.len(), 1);
        assert_eq!(result.rejected, 1);
    }
}
