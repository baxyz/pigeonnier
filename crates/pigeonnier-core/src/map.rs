// SPDX-License-Identifier: AGPL-3.0-or-later

//! The second step: align each source's fields on common names. A mapping is data.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::source::{Provenance, RawItem};

/// Pairs of common name and JSON pointer into the source's payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping(pub Vec<(String, String)>);

impl Mapping {
    #[must_use]
    pub fn new(pairs: &[(&str, &str)]) -> Self {
        Self(
            pairs
                .iter()
                .map(|(name, path)| ((*name).to_owned(), (*path).to_owned()))
                .collect(),
        )
    }

    /// Adds a pair whose path goes through a name the user chose, such as a project field.
    pub fn push_named(&mut self, name: &str, prefix: &str, user_name: &str, suffix: &str) {
        let escaped = user_name.replace('~', "~0").replace('/', "~1");
        self.0
            .push((name.to_owned(), format!("{prefix}/{escaped}{suffix}")));
    }

    /// A field that is missing or `null` in the source stays absent from the record.
    #[must_use]
    pub fn apply(&self, raw: &RawItem) -> Record {
        let fields = self
            .0
            .iter()
            .filter_map(|(name, path)| {
                let value = raw.payload.pointer(path).filter(|value| !value.is_null())?;
                Some((name.clone(), value.clone()))
            })
            .collect();
        Record {
            provenance: raw.provenance.clone(),
            fields,
        }
    }
}

/// An item under common field names, with where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub provenance: Provenance,
    pub fields: BTreeMap<String, Value>,
}

impl Record {
    #[must_use]
    pub fn text(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(Value::as_str)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn raw(payload: Value) -> RawItem {
        RawItem {
            provenance: Provenance {
                source: "test",
                id: "1".to_owned(),
                version: None,
            },
            payload,
        }
    }

    #[test]
    fn null_and_missing_fields_are_absent() {
        let mapping = Mapping::new(&[
            ("title", "/content"),
            ("due", "/due/date"),
            ("nothing", "/nothing"),
            ("owner", "/owner/login"),
        ]);
        let record = mapping.apply(&raw(
            json!({ "content": "x", "due": null, "owner": { "login": "me" } }),
        ));
        assert_eq!(record.fields.len(), 2);
        assert_eq!(record.fields["title"], "x");
        assert_eq!(record.fields["owner"], "me");
        assert_eq!(record.provenance.id, "1");
    }

    #[test]
    fn a_field_name_with_a_slash_still_maps() {
        let mut mapping = Mapping::default();
        mapping.push_named("status", "/fields", "Status/Etat", "/name");
        let record = mapping.apply(&raw(
            json!({ "fields": { "Status/Etat": { "name": "Todo" } } }),
        ));
        assert_eq!(record.fields["status"], "Todo");
    }
}
