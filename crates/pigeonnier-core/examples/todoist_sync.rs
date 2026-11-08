// SPDX-License-Identifier: AGPL-3.0-or-later

//! A full Todoist sync, then an incremental one.
//!
//! `TODOIST_TOKEN=… cargo run -p pigeonnier-core --example todoist_sync`
//!
//! `TODOIST_API_BASE` points it at another server. `TODOIST_WAIT_SECS` waits between the two syncs,
//! to change a task by hand and see it come back.

mod common;

use std::time::{Duration, Instant};

use pigeonnier_core::filter::Predicate;
use pigeonnier_core::map::Mapping;
use pigeonnier_core::sort::{Absent, Order, SortKey};
use pigeonnier_core::source::todoist::{SyncResponse, SyncState, TodoistClient};
use pigeonnier_core::view::View;

fn view() -> View {
    View {
        mapping: Mapping::new(&[
            ("title", "/content"),
            ("section", "/section_id"),
            ("priority", "/priority"),
            ("due", "/due/date"),
            ("deadline", "/deadline/date"),
            ("checked", "/checked"),
        ]),
        filter: Some(Predicate::is("checked", false)),
        sort: vec![
            SortKey::new("due", Order::Ascending, Absent::Last),
            SortKey::new("priority", Order::Descending, Absent::Last),
        ],
    }
}

fn report(label: &str, elapsed: Duration, response: &SyncResponse) {
    println!(
        "{label}: {elapsed:?}, full_sync {}, generated {:?}, {} items, {} sections, {} projects, {} labels",
        response.full_sync,
        response.full_sync_date_utc,
        response.items.len(),
        response.sections.len(),
        response.projects.len(),
        response.labels.len(),
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("TODOIST_TOKEN")
        .map_err(|_| "TODOIST_TOKEN is not set (Todoist settings, Integrations, Developer)")?;
    let client = match std::env::var("TODOIST_API_BASE") {
        Ok(base) => TodoistClient::with_base(token, base),
        Err(_) => TodoistClient::new(token),
    };
    let mut state = SyncState::default();

    let started = Instant::now();
    let full = client.sync(state.next_token()).await?;
    report("full", started.elapsed(), &full);
    common::print_census("items", &full.items);
    common::print_census("sections", &full.sections);
    state.apply(full);

    let result = view().run(&state.raw_items());
    println!(
        "view: {} open, {} undecided, {} checked",
        result.kept.len(),
        result.unknown.len(),
        result.rejected
    );
    let no_due = result
        .kept
        .iter()
        .filter(|record| !record.fields.contains_key("due"))
        .count();
    println!("  of the open ones, {no_due} have no due date");

    if let Ok(seconds) = std::env::var("TODOIST_WAIT_SECS") {
        println!("waiting {seconds} s: change a task now");
        tokio::time::sleep(Duration::from_secs(seconds.parse()?)).await;
    }

    let started = Instant::now();
    let incremental = client.sync(state.next_token()).await?;
    report("incremental", started.elapsed(), &incremental);
    common::print_census("changed items", &incremental.items);
    state.apply(incremental);
    println!("state: {} items known", state.items.len());
    Ok(())
}
