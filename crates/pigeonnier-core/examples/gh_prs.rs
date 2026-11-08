// SPDX-License-Identifier: AGPL-3.0-or-later

//! The two GitHub searches of the spec, with their cost.
//!
//! `cargo run -p pigeonnier-core --example gh_prs`

mod common;

use std::collections::BTreeMap;

use pigeonnier_core::map::Mapping;
use pigeonnier_core::sort::{Absent, Order, SortKey};
use pigeonnier_core::source::github::{GithubClient, PrFields, PrSearch};
use pigeonnier_core::view::View;
use serde_json::Value;

const REVIEWS: &str = "is:pr is:open archived:false review-requested:@me";
const MINE: &str = "is:pr is:open archived:false author:@me";

fn view() -> View {
    View {
        mapping: Mapping::new(&[
            ("title", "/title"),
            ("repo", "/repository/nameWithOwner"),
            ("owner", "/repository/owner/login"),
            ("number", "/number"),
            ("due", "/milestone/dueOn"),
            ("updated", "/updatedAt"),
            ("draft", "/isDraft"),
            ("review", "/reviewDecision"),
            ("mergeable", "/mergeable"),
            ("ci", "/statusCheckRollup/state"),
        ]),
        filter: None,
        sort: vec![
            SortKey::new("due", Order::Ascending, Absent::Last),
            SortKey::new("updated", Order::Descending, Absent::Last),
        ],
    }
}

fn text<'a>(record: &'a pigeonnier_core::map::Record, field: &str) -> &'a str {
    record
        .fields
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("-")
}

fn report(label: &str, search: &PrSearch) {
    let cost = search.cost;
    println!(
        "{label}: {} results, {} fetched, {} request(s), {} point(s), {} left",
        search.issue_count,
        search.items.len(),
        cost.requests,
        cost.points,
        cost.remaining
    );
    let result = view().run(&search.items);
    for record in &result.kept {
        println!(
            "  {} #{} [ci {} | review {} | mergeable {} | due {}] {}",
            text(record, "repo"),
            record.fields.get("number").unwrap_or(&Value::Null),
            text(record, "ci"),
            text(record, "review"),
            text(record, "mergeable"),
            text(record, "due"),
            text(record, "title"),
        );
    }
    let mut owners = BTreeMap::new();
    for record in &result.kept {
        *owners.entry(text(record, "owner").to_owned()).or_insert(0) += 1;
    }
    println!("  by owner: {owners:?}");
    common::print_census(label, search.items.iter().map(|item| &item.payload));
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = GithubClient::from_gh_cli()?;

    report(
        "reviews",
        &client.search_pull_requests(REVIEWS, PrFields::Spec).await?,
    );
    report(
        "mine",
        &client.search_pull_requests(MINE, PrFields::Spec).await?,
    );

    let (counts, cost) = client
        .count_in_one_request(&[REVIEWS, MINE], PrFields::Spec)
        .await?;
    println!("both in one request: {counts:?}, {} point(s)", cost.points);

    let extended = client
        .search_pull_requests(REVIEWS, PrFields::Extended)
        .await?;
    println!(
        "reviews with the extra fields: {} point(s)",
        extended.cost.points
    );
    common::print_census(
        "reviews, extended",
        extended.items.iter().map(|item| &item.payload),
    );
    let extended = client
        .search_pull_requests(MINE, PrFields::Extended)
        .await?;
    println!(
        "mine with the extra fields: {} point(s)",
        extended.cost.points
    );
    common::print_census(
        "mine, extended",
        extended.items.iter().map(|item| &item.payload),
    );
    Ok(())
}
