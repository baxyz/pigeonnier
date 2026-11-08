// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::BTreeMap;

use serde_json::Value;

/// How often each field holds a value, walking nested objects. Field names and counts only, so the
/// output can be pasted into `docs/spikes.md` without personal data.
pub fn print_census<'a>(title: &str, items: impl IntoIterator<Item = &'a Value>) {
    let mut counts = BTreeMap::new();
    let mut total = 0;
    for item in items {
        total += 1;
        walk("", item, &mut counts);
    }
    println!("  census of {title} ({total} items):");
    for (path, (present, null)) in counts {
        println!("    {path}: {present} set, {null} null");
    }
}

fn walk(prefix: &str, value: &Value, counts: &mut BTreeMap<String, (u32, u32)>) {
    let Value::Object(fields) = value else {
        return;
    };
    for (name, value) in fields {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        let entry = counts.entry(path.clone()).or_default();
        if value.is_null() {
            entry.1 += 1;
        } else {
            entry.0 += 1;
        }
        walk(&path, value, counts);
    }
}
