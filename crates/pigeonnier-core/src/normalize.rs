// SPDX-License-Identifier: AGPL-3.0-or-later

//! From a mapped record to an [`Item`]: the rules of each kind of item, and which project it goes to.
//! The mappings say where the fields are; this module only reads common names.

use std::sync::LazyLock;

use chrono::{DateTime, Days, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;

use crate::config::{Config, Project, Roadmap};
use crate::item::{Due, Item, Kind, OTHERS, Priority, Reason, State};
use crate::map::{Mapping, Record};
use crate::source::RawItem;
use crate::source::todoist::SyncState;
use crate::when::working_days_between;

/// What is around an item when it is normalized.
pub struct Context<'a> {
    pub config: &'a Config,
    /// The signed-in GitHub login, which is what `@me` means.
    pub me: &'a str,
    pub now: DateTime<FixedOffset>,
}

static PR_MAPPING: LazyLock<Mapping> = LazyLock::new(|| {
    Mapping::new(&[
        ("title", "/title"),
        ("url", "/url"),
        ("number", "/number"),
        ("repo", "/repository/nameWithOwner"),
        ("repo_name", "/repository/name"),
        ("owner", "/repository/owner/login"),
        ("due", "/milestone/dueOn"),
        ("updated", "/updatedAt"),
        ("created", "/createdAt"),
        ("draft", "/isDraft"),
        ("review", "/reviewDecision"),
        ("mergeable", "/mergeable"),
        ("ci", "/statusCheckRollup/state"),
        ("labels", "/labels/nodes"),
        ("author", "/author/login"),
        ("author_type", "/author/__typename"),
        ("review_events", "/timelineItems/nodes"),
    ])
});

static TODOIST_MAPPING: LazyLock<Mapping> = LazyLock::new(|| {
    Mapping::new(&[
        ("id", "/id"),
        ("title", "/content"),
        ("section", "/section_id"),
        ("project", "/project_id"),
        ("priority", "/priority"),
        ("due", "/due/date"),
        ("deadline", "/deadline/date"),
        ("labels", "/labels"),
        ("checked", "/checked"),
        ("updated", "/updated_at"),
        ("added", "/added_at"),
    ])
});

fn roadmap_mapping(roadmap: &Roadmap) -> Mapping {
    let mut mapping = Mapping::new(&[
        ("id", "/id"),
        ("title", "/content/title"),
        ("url", "/content/url"),
        ("number", "/content/number"),
        ("repo_name", "/content/repository/name"),
        ("state", "/content/state"),
        ("assignees", "/content/assignees/nodes"),
        ("archived", "/isArchived"),
        ("updated", "/updatedAt"),
    ]);
    mapping.push_named("status", "/fields", &roadmap.status_field, "/name");
    mapping.push_named("priority", "/fields", &roadmap.priority_field, "/name");
    mapping.push_named("date", "/fields", &roadmap.date_field, "/date");
    mapping.push_named("iteration", "/fields", &roadmap.iteration_field, "/title");
    mapping.push_named(
        "iteration_start",
        "/fields",
        &roadmap.iteration_field,
        "/startDate",
    );
    mapping.push_named(
        "iteration_days",
        "/fields",
        &roadmap.iteration_field,
        "/duration",
    );
    mapping
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

/// A Todoist date is a plain date, a floating local time, or a UTC time.
#[must_use]
pub fn parse_due(text: &str, offset: FixedOffset) -> Option<Due> {
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(Due::Date(date));
    }
    if let Some(time) = parse_time(text) {
        return Some(Due::DateTime(time));
    }
    let floating = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").ok()?;
    offset
        .from_local_datetime(&floating)
        .single()
        .map(|time| Due::DateTime(time.with_timezone(&Utc)))
}

fn names(record: &Record, field: &str, key: Option<&str>) -> Vec<String> {
    let values = record.fields.get(field).and_then(Value::as_array);
    values
        .into_iter()
        .flatten()
        .filter_map(|value| match key {
            Some(key) => value[key].as_str(),
            None => value.as_str(),
        })
        .map(str::to_owned)
        .collect()
}

/// The urgent first, as listed by the spec: only the first state that applies counts. A review
/// decision that stays empty means the repository requires no review, so a green and mergeable PR
/// is ready. `UNKNOWN` mergeability is GitHub still computing it, which is neither of the states it
/// could become.
fn my_pr_state(record: &Record) -> State {
    let ci = record.text("ci");
    let review = record.text("review");
    let mergeable = record.text("mergeable");
    if review == Some("CHANGES_REQUESTED") {
        return State::ActionRequired(Reason::ChangesRequested);
    }
    if matches!(ci, Some("FAILURE" | "ERROR")) {
        return State::ActionRequired(Reason::CiFailed);
    }
    if mergeable == Some("CONFLICTING") {
        return State::ActionRequired(Reason::Conflicts);
    }
    let draft = record.fields.get("draft") == Some(&Value::Bool(true));
    let approved = matches!(review, None | Some("APPROVED"));
    let green = matches!(ci, None | Some("SUCCESS"));
    if !draft && approved && green && mergeable == Some("MERGEABLE") {
        return State::ActionRequired(Reason::ToMerge);
    }
    State::Waiting
}

fn is_bot(record: &Record) -> bool {
    record.text("author_type") == Some("Bot")
        || record.text("author").is_some_and(|login| {
            let login = login.to_lowercase();
            login.ends_with("[bot]") || login.contains("dependabot") || login.contains("renovate")
        })
}

fn pr_priority(state: State, bot: bool, labels: &[String]) -> Priority {
    let urgent = labels.iter().any(|label| {
        label.eq_ignore_ascii_case("urgent") || label.eq_ignore_ascii_case("priority:high")
    });
    match state {
        _ if urgent => Priority::P1,
        _ if bot => Priority::P4,
        State::ActionRequired(Reason::ToReview | Reason::CiFailed | Reason::ChangesRequested) => {
            Priority::P2
        }
        State::ActionRequired(Reason::ToMerge | Reason::Conflicts) => Priority::P3,
        State::Waiting | State::Planned => Priority::P4,
    }
}

/// Normalizes a pull request. `kind` says which search found it: a review, or one of my own.
#[must_use]
pub fn pull_request(raw: &RawItem, kind: Kind, context: &Context<'_>) -> Item {
    let record = PR_MAPPING.apply(raw);
    let repo = record.text("repo").unwrap_or_default();
    let number = record
        .fields
        .get("number")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let bot = is_bot(&record);
    let labels = names(&record, "labels", Some("name"));
    let state = if kind == Kind::Review {
        State::ActionRequired(Reason::ToReview)
    } else {
        my_pr_state(&record)
    };

    let requested_at = names(&record, "review_events", Some("createdAt"))
        .iter()
        .filter_map(|text| parse_time(text))
        .max()
        .or_else(|| record.text("created").and_then(parse_time));

    let mut priority = pr_priority(state, bot, &labels);
    if kind == Kind::Review && priority == Priority::P2 {
        let threshold = context.config.defaults.review_escalation_days;
        let waited = requested_at.map_or(0, |time| {
            let from = time.with_timezone(context.now.offset()).date_naive();
            working_days_between(from, context.now.date_naive())
        });
        if waited > threshold {
            priority = Priority::P1;
        }
    }

    let project = context
        .config
        .project_for_repo(record.text("owner").unwrap_or_default(), repo, context.me)
        .map_or(OTHERS, |project| project.id.as_str());

    Item {
        id: format!("gh-pr:{repo}#{number}"),
        project: project.to_owned(),
        kind,
        title: record.text("title").unwrap_or_default().to_owned(),
        context: format!("{}#{number}", record.text("repo_name").unwrap_or(repo)),
        url: record.text("url").unwrap_or_default().to_owned(),
        priority,
        due: record
            .text("due")
            .and_then(|text| parse_due(text, *context.now.offset())),
        deadline: None,
        state,
        updated_at: record
            .text("updated")
            .and_then(parse_time)
            .unwrap_or_default(),
        requested_at: if kind == Kind::Review {
            requested_at
        } else {
            None
        },
        bot,
        snoozed_until: None,
        provenance: record.provenance,
    }
}

fn roadmap_priority(option: Option<&str>) -> Priority {
    match option.map(str::to_uppercase).as_deref() {
        Some("P0") => Priority::P1,
        Some("P1") => Priority::P2,
        Some("P2") => Priority::P3,
        _ => Priority::P4,
    }
}

/// Normalizes a project item, or says it should not be listed: archived, closed, done, or assigned
/// to someone else when the project only wants mine.
#[must_use]
pub fn roadmap_item(raw: &RawItem, project: &Project, context: &Context<'_>) -> Option<Item> {
    let default_roadmap = Roadmap::default();
    let roadmap = project.roadmap.as_ref().unwrap_or(&default_roadmap);
    let record = roadmap_mapping(roadmap).apply(raw);

    if record.fields.get("archived") == Some(&Value::Bool(true)) {
        return None;
    }
    if record.text("state").is_some_and(|state| state != "OPEN") {
        return None;
    }
    let status = record.text("status");
    if status.is_some_and(|status| roadmap.done_values.iter().any(|done| done == status)) {
        return None;
    }
    if roadmap.only_assigned {
        let assignees = names(&record, "assignees", Some("login"));
        if !assignees
            .iter()
            .any(|login| login.eq_ignore_ascii_case(context.me))
        {
            return None;
        }
    }

    let iteration_end = || {
        let start = NaiveDate::parse_from_str(record.text("iteration_start")?, "%Y-%m-%d").ok()?;
        let days = record.fields.get("iteration_days")?.as_u64()?;
        start.checked_add_days(Days::new(days.saturating_sub(1)))
    };
    let date = record
        .text("date")
        .and_then(|text| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
        .or_else(iteration_end);

    let context_label = match (record.text("repo_name"), record.fields.get("number")) {
        (Some(repo), Some(number)) => format!("{repo}#{number}"),
        _ => record
            .text("iteration")
            .or(status)
            .unwrap_or("brouillon")
            .to_owned(),
    };

    Some(Item {
        id: format!("gh-pv2:{}", record.text("id").unwrap_or_default()),
        project: project.id.clone(),
        kind: Kind::Roadmap,
        title: record.text("title").unwrap_or_default().to_owned(),
        context: context_label,
        url: record.text("url").unwrap_or_default().to_owned(),
        priority: roadmap_priority(record.text("priority")),
        due: date.map(Due::Date),
        deadline: None,
        state: State::Planned,
        updated_at: record
            .text("updated")
            .and_then(parse_time)
            .unwrap_or_default(),
        requested_at: None,
        bot: false,
        snoozed_until: None,
        provenance: record.provenance,
    })
}

fn todoist_priority(value: Option<&Value>) -> Priority {
    // The API counts upward: its 4 is what the apps show as p1.
    match value.and_then(Value::as_u64) {
        Some(4) => Priority::P1,
        Some(3) => Priority::P2,
        Some(2) => Priority::P3,
        _ => Priority::P4,
    }
}

/// Normalizes a Todoist task, or says it should not be listed because it is done.
#[must_use]
pub fn todoist_item(raw: &RawItem, sync: &SyncState, context: &Context<'_>) -> Option<Item> {
    let record = TODOIST_MAPPING.apply(raw);
    if record.fields.get("checked") == Some(&Value::Bool(true)) {
        return None;
    }
    let id = record.text("id").unwrap_or_default();
    let name_of = |resources: &std::collections::BTreeMap<String, Value>, field: &str| {
        record
            .text(field)
            .and_then(|id| resources.get(id))
            .and_then(|resource| resource["name"].as_str())
            .map(str::to_owned)
    };
    let section = name_of(&sync.sections, "section");
    let todoist_project = name_of(&sync.projects, "project").unwrap_or_default();
    let project = section
        .as_deref()
        .and_then(|section| {
            context
                .config
                .project_for_section(section, &todoist_project)
        })
        .map_or(OTHERS, |project| project.id.as_str());
    let labels = names(&record, "labels", None);
    let claude = labels
        .iter()
        .any(|label| label == &context.config.defaults.claude_label);
    let offset = *context.now.offset();

    Some(Item {
        id: format!("td:{id}"),
        project: project.to_owned(),
        kind: if claude {
            Kind::ClaudeAction
        } else {
            Kind::Todo
        },
        title: record.text("title").unwrap_or_default().to_owned(),
        context: section.unwrap_or(todoist_project),
        url: format!("https://app.todoist.com/app/task/{id}"),
        priority: todoist_priority(record.fields.get("priority")),
        due: record.text("due").and_then(|text| parse_due(text, offset)),
        deadline: record
            .text("deadline")
            .and_then(|text| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()),
        state: State::Planned,
        updated_at: record
            .text("updated")
            .or_else(|| record.text("added"))
            .and_then(parse_time)
            .unwrap_or_default(),
        requested_at: None,
        bot: false,
        snoozed_until: None,
        provenance: record.provenance,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::source::github::{raw_project_item, raw_pull_request};
    use crate::source::todoist::SyncResponse;

    fn config() -> Config {
        Config::parse(
            r#"
            [defaults]
            todoist_parent = "Dev"
            [[project]]
            id = "acme"
            name = "Acme"
            github = { owner = "acme", project = 1 }
            todoist = { section = "Acme" }
            [[project]]
            id = "perso"
            name = "Perso"
            github = { owner = "@me" }
            "#,
            "test",
        )
        .unwrap()
    }

    fn now() -> DateTime<FixedOffset> {
        // A Sunday.
        DateTime::parse_from_rfc3339("2026-11-08T10:00:00+01:00").unwrap()
    }

    fn pr(extra: &Value) -> Value {
        let mut base = json!({
            "id": "PR_1", "number": 42, "title": "Fix", "url": "https://x/42",
            "updatedAt": "2026-11-07T10:00:00Z", "createdAt": "2026-11-06T10:00:00Z",
            "isDraft": false, "reviewDecision": null, "mergeable": "MERGEABLE",
            "repository": { "name": "tool", "nameWithOwner": "acme/tool", "owner": { "login": "acme" } },
            "statusCheckRollup": { "state": "SUCCESS" },
            "labels": { "nodes": [] }, "milestone": null,
            "author": { "login": "me", "__typename": "User" },
            "timelineItems": { "nodes": [] },
        });
        for (key, value) in extra.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn normalize_pr(extra: &Value, kind: Kind) -> Item {
        let config = config();
        let context = Context {
            config: &config,
            me: "me",
            now: now(),
        };
        pull_request(&raw_pull_request(&pr(extra)), kind, &context)
    }

    #[test]
    fn a_pr_goes_to_the_project_of_its_owner() {
        let item = normalize_pr(&json!({}), Kind::MyPr);
        assert_eq!(item.id, "gh-pr:acme/tool#42");
        assert_eq!(item.project, "acme");
        assert_eq!(item.context, "tool#42");

        let other = json!({ "repository": {
            "name": "x", "nameWithOwner": "stranger/x", "owner": { "login": "stranger" } } });
        assert_eq!(normalize_pr(&other, Kind::MyPr).project, OTHERS);

        let mine = json!({ "repository": {
            "name": "x", "nameWithOwner": "me/x", "owner": { "login": "me" } } });
        assert_eq!(normalize_pr(&mine, Kind::MyPr).project, "perso");
    }

    #[test]
    fn my_pr_states_follow_the_order_of_the_spec() {
        let state = |extra: Value| normalize_pr(&extra, Kind::MyPr).state;
        let action = State::ActionRequired;
        assert_eq!(
            state(json!({ "reviewDecision": "CHANGES_REQUESTED",
                          "statusCheckRollup": { "state": "FAILURE" } })),
            action(Reason::ChangesRequested)
        );
        assert_eq!(
            state(json!({ "statusCheckRollup": { "state": "ERROR" }, "mergeable": "CONFLICTING" })),
            action(Reason::CiFailed)
        );
        assert_eq!(
            state(json!({ "mergeable": "CONFLICTING" })),
            action(Reason::Conflicts)
        );
        assert_eq!(
            state(json!({ "reviewDecision": "APPROVED" })),
            action(Reason::ToMerge)
        );
    }

    #[test]
    fn green_and_mergeable_is_ready_when_no_review_is_required() {
        let state = |extra: Value| normalize_pr(&extra, Kind::MyPr).state;
        assert_eq!(state(json!({})), State::ActionRequired(Reason::ToMerge));
        assert_eq!(
            state(json!({ "statusCheckRollup": null })),
            State::ActionRequired(Reason::ToMerge)
        );
        assert_eq!(
            state(json!({ "reviewDecision": "REVIEW_REQUIRED" })),
            State::Waiting
        );
        assert_eq!(state(json!({ "isDraft": true })), State::Waiting);
        assert_eq!(
            state(json!({ "statusCheckRollup": { "state": "PENDING" } })),
            State::Waiting
        );
    }

    #[test]
    fn unknown_mergeability_triggers_nothing() {
        let item = normalize_pr(&json!({ "mergeable": "UNKNOWN" }), Kind::MyPr);
        assert_eq!(item.state, State::Waiting);
    }

    #[test]
    fn pr_priorities() {
        let priority = |extra: Value, kind| normalize_pr(&extra, kind).priority;
        let urgent = json!({ "labels": { "nodes": [{ "name": "priority:high" }] } });
        assert_eq!(priority(urgent, Kind::MyPr), Priority::P1);
        let failed = json!({ "statusCheckRollup": { "state": "FAILURE" } });
        assert_eq!(priority(failed, Kind::MyPr), Priority::P2);
        assert_eq!(priority(json!({}), Kind::MyPr), Priority::P3);
        assert_eq!(
            priority(json!({ "isDraft": true }), Kind::MyPr),
            Priority::P4
        );
    }

    #[test]
    fn a_review_waiting_too_long_moves_up() {
        // Up to this Sunday: Wednesday leaves two working days, Tuesday three.
        let waited = |date: &str| {
            normalize_pr(
                &json!({ "timelineItems": { "nodes": [{ "createdAt": date }] } }),
                Kind::Review,
            )
            .priority
        };
        assert_eq!(waited("2026-11-04T10:00:00Z"), Priority::P2);
        assert_eq!(waited("2026-11-03T10:00:00Z"), Priority::P1);
    }

    #[test]
    fn bots_are_recognized_and_stay_low() {
        let bot = normalize_pr(
            &json!({ "author": { "login": "dependabot", "__typename": "Bot" } }),
            Kind::Review,
        );
        assert!(bot.bot);
        assert_eq!(bot.priority, Priority::P4);
    }

    fn roadmap_raw(extra: &Value) -> RawItem {
        let mut base = json!({
            "id": "PVTI_1", "isArchived": false, "updatedAt": "2026-11-07T10:00:00Z",
            "content": { "__typename": "Issue", "title": "Plan", "url": "https://x/1", "state": "OPEN",
                         "number": 7, "repository": { "name": "tool" },
                         "assignees": { "nodes": [] } },
            "fieldValues": { "nodes": [
                { "name": "Todo", "field": { "name": "Status" } },
                { "name": "P0", "field": { "name": "Priority" } },
                { "title": "Sprint 12", "startDate": "2026-11-02", "duration": 14,
                  "field": { "name": "Iteration" } },
            ] },
        });
        for (key, value) in extra.as_object().unwrap() {
            base[key] = value.clone();
        }
        raw_project_item(&base)
    }

    fn normalize_roadmap(raw: &RawItem) -> Option<Item> {
        let config = config();
        let context = Context {
            config: &config,
            me: "me",
            now: now(),
        };
        roadmap_item(raw, &config.projects[0], &context)
    }

    #[test]
    fn a_roadmap_item_takes_its_priority_and_iteration_end() {
        let item = normalize_roadmap(&roadmap_raw(&json!({}))).unwrap();
        assert_eq!(item.id, "gh-pv2:PVTI_1");
        assert_eq!(item.priority, Priority::P1);
        assert_eq!(item.context, "tool#7");
        assert_eq!(
            item.due,
            Some(Due::Date(NaiveDate::from_ymd_opt(2026, 11, 15).unwrap()))
        );
        assert_eq!(item.state, State::Planned);
    }

    #[test]
    fn done_closed_and_archived_items_are_left_out() {
        assert!(normalize_roadmap(&roadmap_raw(&json!({ "isArchived": true }))).is_none());
        let closed =
            json!({ "content": { "__typename": "Issue", "title": "x", "state": "CLOSED" } });
        assert!(normalize_roadmap(&roadmap_raw(&closed)).is_none());
        let done = json!({ "fieldValues": { "nodes": [
            { "name": "Done", "field": { "name": "Status" } }] } });
        assert!(normalize_roadmap(&roadmap_raw(&done)).is_none());
    }

    #[test]
    fn a_draft_with_no_status_is_listed() {
        let draft = json!({
            "content": { "__typename": "DraftIssue", "title": "Idea", "assignees": { "nodes": [] } },
            "fieldValues": { "nodes": [] },
        });
        let item = normalize_roadmap(&roadmap_raw(&draft)).unwrap();
        assert_eq!(item.title, "Idea");
        assert_eq!(item.priority, Priority::P4);
        assert_eq!(item.due, None);
    }

    fn sync_state() -> SyncState {
        let mut state = SyncState::default();
        state.apply(SyncResponse {
            sync_token: "t".to_owned(),
            full_sync: true,
            items: vec![
                json!({ "id": "1", "content": "Do", "section_id": "s1", "project_id": "p1",
                        "priority": 4, "labels": ["claude"], "checked": false,
                        "due": { "date": "2026-11-09" }, "deadline": { "date": "2026-11-12" },
                        "updated_at": "2026-11-07T10:00:00Z" }),
                json!({ "id": "2", "content": "Other", "section_id": null, "project_id": "p1",
                        "priority": 1, "labels": [], "checked": false, "due": null,
                        "added_at": "2026-11-07T10:00:00Z" }),
                json!({ "id": "3", "content": "Done", "checked": true, "labels": [] }),
            ],
            sections: vec![json!({ "id": "s1", "name": "acme" })],
            projects: vec![json!({ "id": "p1", "name": "Dev" })],
            labels: Vec::new(),
        });
        state
    }

    #[test]
    fn todoist_tasks_route_by_section_and_invert_priority() {
        let config = config();
        let context = Context {
            config: &config,
            me: "me",
            now: now(),
        };
        let state = sync_state();
        let items: Vec<Item> = state
            .raw_items()
            .iter()
            .filter_map(|raw| todoist_item(raw, &state, &context))
            .collect();
        assert_eq!(items.len(), 2);
        let claude = &items[0];
        assert_eq!(claude.project, "acme");
        assert_eq!(claude.kind, Kind::ClaudeAction);
        assert_eq!(claude.priority, Priority::P1);
        assert_eq!(claude.context, "acme");
        assert_eq!(
            claude.effective_due(),
            Some(Due::Date(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap()))
        );
        let other = &items[1];
        assert_eq!(other.project, OTHERS);
        assert_eq!(other.kind, Kind::Todo);
        assert_eq!(other.priority, Priority::P4);
    }

    #[test]
    fn three_kinds_of_todoist_dates() {
        let offset = *now().offset();
        assert!(matches!(
            parse_due("2026-11-08", offset),
            Some(Due::Date(_))
        ));
        assert_eq!(
            parse_due("2026-11-08T10:00:00", offset),
            parse_due("2026-11-08T09:00:00Z", offset)
        );
        assert_eq!(parse_due("nonsense", offset), None);
    }
}
