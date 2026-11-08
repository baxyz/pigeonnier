// SPDX-License-Identifier: AGPL-3.0-only

//! The third step: select records. An absent field makes a predicate [`Truth::Unknown`], so a task
//! with no due date is neither overdue nor on time.

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

#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    Is(String, Value),
    /// The field, an ISO 8601 date or datetime, starts before this date. Comparing the text is
    /// right as long as both sides begin with `YYYY-MM-DD`.
    Before(String, String),
    Not(Box<Predicate>),
    And(Vec<Predicate>),
}

impl Predicate {
    #[must_use]
    pub fn is(field: &str, value: impl Into<Value>) -> Self {
        Self::Is(field.to_owned(), value.into())
    }

    #[must_use]
    pub fn before(field: &str, date: &str) -> Self {
        Self::Before(field.to_owned(), date.to_owned())
    }

    #[must_use]
    pub fn negated(self) -> Self {
        Self::Not(Box::new(self))
    }

    #[must_use]
    pub fn eval(&self, record: &Record) -> Truth {
        match self {
            Self::Is(field, expected) => record
                .fields
                .get(field)
                .map_or(Truth::Unknown, |value| Truth::from_bool(value == expected)),
            Self::Before(field, date) => match record.fields.get(field).and_then(Value::as_str) {
                Some(value) => {
                    Truth::from_bool(value.get(..date.len()).unwrap_or(value) < date.as_str())
                }
                None => Truth::Unknown,
            },
            Self::Not(inner) => !inner.eval(record),
            Self::And(predicates) => predicates.iter().fold(Truth::True, |truth, predicate| {
                truth.and(predicate.eval(record))
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

    #[test]
    fn no_due_date_is_neither_overdue_nor_on_time() {
        let overdue = Predicate::before("due", "2026-11-08");
        assert_eq!(
            overdue.eval(&record(json!({ "due": "2026-11-07" }))),
            Truth::True
        );
        assert_eq!(
            overdue.eval(&record(json!({ "due": "2026-11-08" }))),
            Truth::False
        );
        assert_eq!(overdue.eval(&record(json!({}))), Truth::Unknown);
        assert_eq!(overdue.negated().eval(&record(json!({}))), Truth::Unknown);
    }

    #[test]
    fn a_datetime_on_the_day_itself_is_not_before_it() {
        let overdue = Predicate::before("due", "2026-11-08");
        assert_eq!(
            overdue.eval(&record(json!({ "due": "2026-11-08T09:00:00Z" }))),
            Truth::False
        );
    }

    #[test]
    fn and_is_false_as_soon_as_one_side_is_false() {
        let both = Predicate::And(vec![Predicate::is("a", true), Predicate::is("b", true)]);
        assert_eq!(both.eval(&record(json!({ "a": false }))), Truth::False);
        assert_eq!(both.eval(&record(json!({ "a": true }))), Truth::Unknown);
        assert_eq!(
            both.eval(&record(json!({ "a": true, "b": true }))),
            Truth::True
        );
    }
}
