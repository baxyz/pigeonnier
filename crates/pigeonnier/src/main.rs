// SPDX-License-Identifier: AGPL-3.0-or-later

mod mcp;

use std::io::Read as _;

use anyhow::{Context as _, Result, anyhow, bail};
use chrono::{DateTime, FixedOffset, Local};
use clap::{Parser, Subcommand, ValueEnum};
use pigeonnier_core::cache::Cache;
use pigeonnier_core::config::Config;
use pigeonnier_core::credentials;
use pigeonnier_core::discover;
use pigeonnier_core::engine::{Engine, NewAction, connect};
use pigeonnier_core::item::Priority;
use pigeonnier_core::paths;
use pigeonnier_core::source::github::GithubClient;
use pigeonnier_core::source::todoist::{FULL_SYNC, TodoistClient};
use pigeonnier_core::viewmodel::{ItemVm, Snapshot};
use pigeonnier_core::when::parse_until;

/// What is waiting for you, project by project.
#[derive(Parser)]
#[command(name = "pigeonnier", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List what is waiting, grouped by project
    List {
        /// A view: all, action, today, or one of views.json
        #[arg(long, default_value = "all")]
        view: String,
        /// Print the data the UIs draw from, as JSON
        #[arg(long)]
        json: bool,
        /// Do not call the services, show the last known state
        #[arg(long)]
        cached: bool,
        /// Show the ids that `done` and `snooze` take
        #[arg(long)]
        ids: bool,
    },
    /// Mark a Todoist task done
    Done { id: String },
    /// Report an item: 2h, tomorrow, monday, 2026-12-01
    Snooze { id: String, until: String },
    /// Create a manual action in a project's Todoist section
    Add {
        project: String,
        title: String,
        /// In Todoist's language, such as "friday"
        #[arg(long)]
        due: Option<String>,
        #[arg(long, value_enum)]
        priority: Option<PriorityArg>,
        #[arg(long)]
        pr_url: Option<String>,
    },
    /// Propose a configuration from your GitHub owners and Todoist sections
    Init {
        /// The Todoist project holding one section per project
        #[arg(long)]
        todoist_parent: Option<String>,
        /// Write it to the configuration file instead of printing it
        #[arg(long)]
        write: bool,
    },
    /// Save a Todoist API token, read from standard input, in the system keyring
    Login,
    /// Remove the Todoist token from the keyring and forget its cache
    Logout,
    /// List the available views
    Views,
    /// Serve `list_items` and `add_manual_action` to an agent, over MCP on standard input and output
    Mcp,
}

#[derive(Clone, Copy, ValueEnum)]
enum PriorityArg {
    P1,
    P2,
    P3,
    P4,
}

impl From<PriorityArg> for Priority {
    fn from(priority: PriorityArg) -> Self {
        match priority {
            PriorityArg::P1 => Self::P1,
            PriorityArg::P2 => Self::P2,
            PriorityArg::P3 => Self::P3,
            PriorityArg::P4 => Self::P4,
        }
    }
}

fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

fn engine() -> Result<Engine> {
    let config = Config::load(&paths::config_file())?;
    let cache = Cache::open(&paths::cache_file())?;
    Ok(connect(config, cache))
}

fn print_item(item: &ItemVm, ids: bool) {
    let mut details = vec![item.context.clone()];
    details.extend(item.state_label.clone());
    details.extend(item.due_label.clone());
    let details: Vec<String> = details
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect();
    println!(
        "  [{}] {} ({})",
        item.priority,
        item.title,
        details.join(" · ")
    );
    if ids {
        println!("       {}", item.id);
    }
}

fn print_snapshot(snapshot: &Snapshot, ids: bool) {
    for issue in &snapshot.issues {
        eprintln!("! {} : {}", issue.source, issue.message);
    }
    if snapshot.projects.is_empty() {
        println!("Rien à faire.");
    }
    for project in &snapshot.projects {
        println!("{} ({} à traiter)", project.name, project.action_count);
        for item in &project.items {
            print_item(item, ids);
        }
        if let Some(bots) = &project.bots {
            println!("  {}", bots.label);
            if ids {
                for item in &bots.items {
                    print_item(item, true);
                }
            }
        }
    }
}

async fn init(todoist_parent: Option<String>, write: bool) -> Result<()> {
    let token = credentials::github_token().context("no GitHub token: run `gh auth login`")?;
    let owners = GithubClient::new(token).discover_owners().await?;
    if owners.len() <= 1 {
        eprintln!(
            "warning: no organization visible. A token limited to one owner cannot list them, so \
             this proposal may be missing projects."
        );
    }
    let sections = match (credentials::todoist_token(), &todoist_parent) {
        (Some(token), Some(parent)) => {
            let response = TodoistClient::new(token).sync(FULL_SYNC).await?;
            todoist_sections(&response.projects, &response.sections, parent)?
        }
        (None, Some(_)) => bail!("no Todoist token: run `pigeonnier login`"),
        (_, None) => Vec::new(),
    };
    let proposal = discover::propose(&owners, &sections, todoist_parent.as_deref());
    if write {
        let path = paths::config_file();
        if path.exists() {
            bail!(
                "{} already exists; print the proposal and merge by hand",
                path.display()
            );
        }
        proposal.save(&path)?;
        println!("written to {}", path.display());
    } else {
        let text = proposal.to_toml()?;
        print!("{text}");
        eprintln!("add --write to save it");
    }
    Ok(())
}

fn todoist_sections(
    projects: &[serde_json::Value],
    sections: &[serde_json::Value],
    parent: &str,
) -> Result<Vec<String>> {
    let wanted = pigeonnier_core::config::normalize_name(parent);
    let parent_id = projects
        .iter()
        .find(|project| {
            project["name"]
                .as_str()
                .is_some_and(|name| pigeonnier_core::config::normalize_name(name) == wanted)
        })
        .and_then(|project| project["id"].as_str())
        .ok_or_else(|| anyhow!("no Todoist project named {parent}"))?;
    Ok(sections
        .iter()
        .filter(|section| section["project_id"].as_str() == Some(parent_id))
        .filter_map(|section| section["name"].as_str().map(str::to_owned))
        .collect())
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::List {
            view,
            json,
            cached,
            ids,
        } => {
            let engine = engine()?;
            if !cached {
                engine.refresh().await?;
            }
            let snapshot = engine.snapshot(&view, now())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            } else {
                print_snapshot(&snapshot, ids);
            }
        }
        Command::Done { id } => {
            engine()?.complete(&id).await?;
            println!("{id} : fait");
        }
        Command::Snooze { id, until } => {
            let at = parse_until(&until, now()).ok_or_else(|| anyhow!("cannot read {until}"))?;
            engine()?.snooze(&id, at).await?;
            println!("{id} : reporté au {}", at.format("%Y-%m-%d %H:%M"));
        }
        Command::Add {
            project,
            title,
            due,
            priority,
            pr_url,
        } => {
            engine()?
                .add_manual_action(NewAction {
                    project,
                    title,
                    due,
                    priority: priority.map(Into::into),
                    pr_url,
                })
                .await?;
            println!("ajouté");
        }
        Command::Init {
            todoist_parent,
            write,
        } => init(todoist_parent, write).await?,
        Command::Login => {
            let mut token = String::new();
            std::io::stdin().read_to_string(&mut token)?;
            let token = token.trim();
            if token.is_empty() {
                bail!("no token on standard input");
            }
            credentials::store(credentials::TODOIST, token)?;
            println!("token saved in the system keyring");
        }
        Command::Logout => {
            credentials::forget(credentials::TODOIST)?;
            engine()?.purge_service("todoist.")?;
            println!("disconnected from Todoist");
        }
        Command::Mcp => mcp::serve(engine()?).await?,
        Command::Views => {
            for view in engine()?.views() {
                println!("{}  {}", view.id, view.title);
            }
        }
    }
    Ok(())
}
