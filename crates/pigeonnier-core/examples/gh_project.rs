// SPDX-License-Identifier: AGPL-3.0-or-later

//! The items of a GitHub Project v2.
//!
//! `cargo run -p pigeonnier-core --example gh_project -- helpers4 1`

mod common;

use std::process::ExitCode;

use pigeonnier_core::filter::Predicate;
use pigeonnier_core::map::Mapping;
use pigeonnier_core::sort::{Absent, Order, SortKey};
use pigeonnier_core::source::github::{GithubClient, ProjectRead, ProjectSnapshot};
use pigeonnier_core::view::View;
use serde_json::Value;

/// The defaults the spec gives for `[project.roadmap]`.
fn view() -> View {
    View {
        mapping: Mapping::new(&[
            ("title", "/content/title"),
            ("kind", "/content/__typename"),
            ("state", "/content/state"),
            ("archived", "/isArchived"),
            ("status", "/fields/Status/name"),
            ("priority", "/fields/Priority/name"),
            ("due", "/fields/Target date/date"),
            ("iteration", "/fields/Iteration/title"),
        ]),
        filter: Some(Predicate::And(vec![
            Predicate::is("archived", false),
            Predicate::is("state", "CLOSED").negated(),
            Predicate::is("state", "MERGED").negated(),
            Predicate::is("status", "Done").negated(),
        ])),
        sort: vec![
            SortKey::new("due", Order::Ascending, Absent::Last),
            SortKey::new("priority", Order::Ascending, Absent::Last),
        ],
    }
}

fn report(project: &ProjectSnapshot) {
    println!(
        "{:?}: closed {}, {} items of {}",
        project.title,
        project.closed,
        project.items.len(),
        project.total_count
    );
    println!("fields:");
    for field in &project.fields {
        let options = field["options"].as_array().map_or(0, Vec::len);
        println!(
            "  {} ({}, {options} options)",
            field["name"].as_str().unwrap_or("?"),
            field["dataType"].as_str().unwrap_or("?")
        );
    }
    let result = view().run(&project.items);
    println!(
        "view: {} kept, {} unknown (draft issues have no state, some items no status), {} rejected",
        result.kept.len(),
        result.unknown.len(),
        result.rejected
    );
    let line = |record: &pigeonnier_core::map::Record| {
        let text = |field: &str| {
            record
                .fields
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or("-")
        };
        println!(
            "  {} [{} | {} | due {} | {}] {}",
            text("kind"),
            text("status"),
            text("priority"),
            text("due"),
            text("iteration"),
            text("title")
        );
    };
    result.kept.iter().chain(&result.unknown).for_each(line);
    common::print_census("items", project.items.iter().map(|item| &item.payload));
}

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let owner = arguments.next().unwrap_or_else(|| "helpers4".to_owned());
    let number = arguments.next().map_or(Ok(1), |number| number.parse())?;

    let client = GithubClient::from_gh_cli()?;
    let (read, cost) = client.read_project(&owner, number).await?;
    println!(
        "{} request(s), {} point(s), {} left",
        cost.requests, cost.points, cost.remaining
    );
    match read {
        ProjectRead::Found(project) => {
            report(&project);
            Ok(ExitCode::SUCCESS)
        }
        ProjectRead::NotFound => {
            eprintln!("{owner} has no project number {number}");
            Ok(ExitCode::from(2))
        }
        ProjectRead::Inaccessible => {
            eprintln!(
                "{owner}/{number} exists but this token may not read it. A classic or OAuth token \
                 needs read:project. A fine-grained one must be owned by the organization, with the \
                 organization permission Projects; it cannot read a project owned by a user."
            );
            Ok(ExitCode::from(3))
        }
    }
}
