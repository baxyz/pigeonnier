// SPDX-License-Identifier: AGPL-3.0-or-later

//! The filter step: select records. An absent field makes a predicate [`Truth::Unknown`], so a task
//! with no due date is neither overdue nor on time.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::map::Record;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    fn from_bool(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }

    #[must_use]
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }
}

impl std::ops::Not for Truth {
    type Output = Self;

    fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Predicate {
    Is(String, Value),
    Not(Box<Predicate>),
    And(Vec<Predicate>),
    Or(Vec<Predicate>),
}

impl Predicate {
    #[must_use]
    pub fn eval(&self, record: &Record) -> Truth {
        match self {
            Self::Is(field, expected) => record
                .fields
                .get(field)
                .map_or(Truth::Unknown, |value| Truth::from_bool(value == expected)),
            Self::Not(inner) => !inner.eval(record),
            Self::And(predicates) => predicates.iter().fold(Truth::True, |truth, predicate| {
                truth.and(predicate.eval(record))
            }),
            Self::Or(predicates) => predicates.iter().fold(Truth::False, |truth, predicate| {
                truth.or(predicate.eval(record))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::source::Provenance;

    fn record(fields: Value) -> Record {
        let fields: BTreeMap<String, Value> = serde_json::from_value(fields).unwrap();
        Record {
            provenance: Provenance {
                source: "test",
                id: "1".to_owned(),
                version: None,
            },
            fields,
        }
    }

    fn is(field: &str, value: Value) -> Predicate {
        Predicate::Is(field.to_owned(), value)
    }

    #[test]
    fn absent_is_not_false() {
        let overdue = is("overdue", json!(true));
        assert_eq!(
            overdue.eval(&record(json!({ "overdue": true }))),
            Truth::True
        );
        assert_eq!(
            overdue.eval(&record(json!({ "overdue": false }))),
            Truth::False
        );
        assert_eq!(overdue.eval(&record(json!({}))), Truth::Unknown);
        assert_eq!(
            Predicate::Not(Box::new(overdue)).eval(&record(json!({}))),
            Truth::Unknown
        );
    }

    #[test]
    fn and_is_false_as_soon_as_one_side_is_false() {
        let both = Predicate::And(vec![is("a", json!(true)), is("b", json!(true))]);
        assert_eq!(both.eval(&record(json!({ "a": false }))), Truth::False);
        assert_eq!(both.eval(&record(json!({ "a": true }))), Truth::Unknown);
        assert_eq!(
            both.eval(&record(json!({ "a": true, "b": true }))),
            Truth::True
        );
    }

    #[test]
    fn or_is_true_as_soon_as_one_side_is_true() {
        let either = Predicate::Or(vec![is("a", json!(true)), is("b", json!(true))]);
        assert_eq!(either.eval(&record(json!({ "a": true }))), Truth::True);
        assert_eq!(either.eval(&record(json!({ "a": false }))), Truth::Unknown);
        assert_eq!(
            either.eval(&record(json!({ "a": false, "b": false }))),
            Truth::False
        );
    }

    #[test]
    fn a_predicate_is_data() {
        let parsed: Predicate = serde_json::from_value(
            json!({ "and": [{ "is": ["a", true] }, { "not": { "is": ["b", 1] } }] }),
        )
        .unwrap();
        assert_eq!(
            parsed.eval(&record(json!({ "a": true, "b": 2 }))),
            Truth::True
        );
    }
}
