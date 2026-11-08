// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sort step: order records. Where absent values go is the key's choice, whatever the order.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::map::Record;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Absent {
    First,
    #[default]
    Last,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKey {
    pub field: String,
    pub order: Order,
    #[serde(default)]
    pub absent: Absent,
}

impl SortKey {
    fn compare(&self, left: &Record, right: &Record) -> Ordering {
        match (left.fields.get(&self.field), right.fields.get(&self.field)) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) if self.absent == Absent::First => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) if self.absent == Absent::First => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left), Some(right)) => {
                let ordering = compare_values(left, right);
                match self.order {
                    Order::Asc => ordering,
                    Order::Desc => ordering.reverse(),
                }
            }
        }
    }
}

fn compare_values(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (Value::Number(left), Value::Number(right)) => {
            let (left, right) = (left.as_f64().unwrap_or(0.0), right.as_f64().unwrap_or(0.0));
            left.total_cmp(&right)
        }
        _ => Ordering::Equal,
    }
}

/// A stable sort: records equal on every key keep their order.
pub fn sort(records: &mut [Record], keys: &[SortKey]) {
    records.sort_by(|left, right| {
        keys.iter()
            .map(|key| key.compare(left, right))
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::source::Provenance;

    fn record(id: &str, field: Option<Value>) -> Record {
        Record {
            provenance: Provenance {
                source: "test",
                id: id.to_owned(),
                version: None,
            },
            fields: field
                .map(|value| ("due".to_owned(), value))
                .into_iter()
                .collect(),
        }
    }

    fn ids(records: &[Record]) -> Vec<&str> {
        records
            .iter()
            .map(|record| record.provenance.id.as_str())
            .collect()
    }

    fn key(order: Order) -> SortKey {
        SortKey {
            field: "due".to_owned(),
            order,
            absent: Absent::Last,
        }
    }

    #[test]
    fn absent_stays_last_in_both_orders() {
        let mut records = [
            record("none", None),
            record("late", Some(json!("2026-12-01"))),
            record("soon", Some(json!("2026-11-10"))),
        ];
        sort(&mut records, &[key(Order::Asc)]);
        assert_eq!(ids(&records), ["soon", "late", "none"]);
        sort(&mut records, &[key(Order::Desc)]);
        assert_eq!(ids(&records), ["late", "soon", "none"]);
    }
}
