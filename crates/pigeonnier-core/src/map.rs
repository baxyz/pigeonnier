// SPDX-License-Identifier: AGPL-3.0-or-later

//! The second step: align each source's fields on common names. A mapping is data.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::source::{Provenance, RawItem};

/// Pairs of common name and JSON pointer into the source's payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn null_and_missing_fields_are_absent() {
        let raw = RawItem {
            provenance: Provenance {
                source: "test",
                id: "1".to_owned(),
                version: None,
            },
            payload: json!({ "content": "x", "due": null, "owner": { "login": "me" } }),
        };
        let mapping = Mapping::new(&[
            ("title", "/content"),
            ("due", "/due/date"),
            ("nothing", "/nothing"),
            ("owner", "/owner/login"),
        ]);
        let record = mapping.apply(&raw);
        assert_eq!(record.fields.len(), 2);
        assert_eq!(record.fields["title"], "x");
        assert_eq!(record.fields["owner"], "me");
        assert_eq!(record.provenance.id, "1");
    }
}
