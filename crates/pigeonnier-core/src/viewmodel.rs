// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the UIs draw: lists already sorted and grouped by project, labels ready to show, semantic
//! colors and the actions each item offers. A new state or sort rule changes this crate only.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::item::{Due, Item, Kind, OTHERS, Priority, Reason, State};
use crate::view::View;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// No project is configured yet: the UI offers the first-run setup.
    pub needs_setup: bool,
    pub github_connected: bool,
    pub todoist_connected: bool,
    pub view: String,
    pub views: Vec<ViewChoice>,
    pub projects: Vec<ProjectVm>,
    pub badge: Badge,
    pub issues: Vec<Issue>,
    pub refreshed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewChoice {
    pub id: String,
    pub title: String,
}

/// The counter of the top bar: what is P1 or late. Red only when something is late.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    pub count: u32,
    pub alert: bool,
}

/// A source that failed: the rest of the list stays usable, and comes from the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub source: String,
    pub message: String,
    pub reconnect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectVm {
    pub id: String,
    pub name: String,
    pub color: String,
    pub pinned: bool,
    /// "Autres" starts folded.
    pub collapsed: bool,
    pub action_count: u32,
    pub overdue_count: u32,
    pub items: Vec<ItemVm>,
    pub bots: Option<BotGroup>,
}

/// Dependency updates, on one line that can be unfolded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotGroup {
    pub label: String,
    pub items: Vec<ItemVm>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemVm {
    pub id: String,
    pub kind: String,
    pub icon: String,
    pub title: String,
    pub context: String,
    pub url: String,
    pub priority: String,
    /// `red`, `orange`, `blue`, or nothing for P4.
    pub flag_color: Option<String>,
    pub flag_label: String,
    pub due_label: Option<String>,
    /// `overdue`, `today`, `soon` or `later`.
    pub due_urgency: Option<String>,
    pub state_label: Option<String>,
    pub waiting: bool,
    pub action_required: bool,
    pub actions: Vec<String>,
}

const WEEKDAYS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
const MONTHS: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

fn date_label(date: NaiveDate, today: NaiveDate) -> String {
    let days = (date - today).num_days();
    match days {
        0 => "aujourd'hui".to_owned(),
        1 => "demain".to_owned(),
        -1 => "hier".to_owned(),
        _ if days < 0 => format!("en retard de {} j", -days),
        _ if days < 7 => WEEKDAYS[date.weekday().num_days_from_monday() as usize].to_owned(),
        _ => format!("{} {}", date.day(), MONTHS[date.month0() as usize]),
    }
}

fn due_text(due: Due, now: DateTime<FixedOffset>) -> String {
    let offset = *now.offset();
    let date = date_label(due.local_date(offset), now.date_naive());
    match due {
        Due::Date(_) => date,
        Due::DateTime(time) => {
            let local = time.with_timezone(&offset);
            format!("{date} {:02}:{:02}", local.hour(), local.minute())
        }
    }
}

fn due_label(item: &Item, now: DateTime<FixedOffset>) -> Option<String> {
    let offset = *now.offset();
    match (item.due, item.deadline) {
        (Some(due), Some(deadline)) if due.local_date(offset) != deadline => Some(format!(
            "prévu {} · limite {}",
            due_text(due, now),
            date_label(deadline, now.date_naive())
        )),
        (_, Some(deadline)) => Some(date_label(deadline, now.date_naive())),
        (Some(due), None) => Some(due_text(due, now)),
        (None, None) => None,
    }
}

fn due_urgency(item: &Item, now: DateTime<FixedOffset>) -> Option<String> {
    let due = item.effective_due()?;
    let days = (due.local_date(*now.offset()) - now.date_naive()).num_days();
    let urgency = match days {
        _ if item.is_overdue(now) => "overdue",
        0 => "today",
        1..=3 => "soon",
        _ => "later",
    };
    Some(urgency.to_owned())
}

fn state_label(item: &Item, now: DateTime<FixedOffset>) -> Option<String> {
    match item.state {
        State::ActionRequired(Reason::ToReview) => {
            let days = item
                .requested_at
                .map_or(0, |time| (now.with_timezone(&Utc) - time).num_days());
            Some(if days > 0 {
                format!("à reviewer depuis {days} j")
            } else {
                "à reviewer".to_owned()
            })
        }
        State::ActionRequired(Reason::ChangesRequested) => Some("à corriger".to_owned()),
        State::ActionRequired(Reason::CiFailed) => Some("CI rouge".to_owned()),
        State::ActionRequired(Reason::Conflicts) => Some("conflits".to_owned()),
        State::ActionRequired(Reason::ToMerge) => Some("à merger".to_owned()),
        State::Waiting => Some("en attente".to_owned()),
        State::Planned => None,
    }
}

fn kind_name(kind: Kind) -> (&'static str, &'static str) {
    match kind {
        Kind::Review => ("review", "avatar-default-symbolic"),
        Kind::MyPr => ("pr", "document-edit-symbolic"),
        Kind::ClaudeAction => ("claude", "dialog-information-symbolic"),
        Kind::Todo => ("todo", "object-select-symbolic"),
        Kind::Roadmap => ("roadmap", "x-office-calendar-symbolic"),
    }
}

fn item_vm(item: &Item, now: DateTime<FixedOffset>) -> ItemVm {
    let (kind, icon) = kind_name(item.kind);
    let (flag_color, flag_label) = match item.priority {
        Priority::P1 => (Some("red"), "Priorité 1"),
        Priority::P2 => (Some("orange"), "Priorité 2"),
        Priority::P3 => (Some("blue"), "Priorité 3"),
        Priority::P4 => (None, "Priorité 4"),
    };
    let mut actions = vec!["open".to_owned(), "snooze".to_owned()];
    if item.id.starts_with("td:") {
        actions.push("complete".to_owned());
    }
    if item.state == State::ActionRequired(Reason::ToMerge) && item.kind == Kind::MyPr {
        actions.push("merge".to_owned());
    }
    ItemVm {
        id: item.id.clone(),
        kind: kind.to_owned(),
        icon: icon.to_owned(),
        title: item.title.clone(),
        context: item.context.clone(),
        url: item.url.clone(),
        priority: format!("{:?}", item.priority),
        flag_color: flag_color.map(str::to_owned),
        flag_label: flag_label.to_owned(),
        due_label: due_label(item, now),
        due_urgency: due_urgency(item, now),
        state_label: state_label(item, now),
        waiting: item.state == State::Waiting,
        action_required: matches!(item.state, State::ActionRequired(_)),
        actions,
    }
}

/// Builds what a UI draws for one view, with projects in the order of the configuration, the pinned
/// ones first, "Autres" last, and neither muted nor empty ones.
#[must_use]
pub fn build(
    items: &[Item],
    config: &Config,
    views: &[View],
    view_id: &str,
    now: DateTime<FixedOffset>,
    issues: Vec<Issue>,
    refreshed_at: Option<DateTime<Utc>>,
) -> Snapshot {
    let view = views
        .iter()
        .find(|view| view.id == view_id)
        .or_else(|| views.first());
    let records = items.iter().map(|item| item.to_record(now)).collect();
    let kept = view.map_or_else(Vec::new, |view| view.run(records).kept);
    let by_id: HashMap<&str, &Item> = items.iter().map(|item| (item.id.as_str(), item)).collect();

    let mut groups: HashMap<&str, (Vec<ItemVm>, Vec<ItemVm>)> = HashMap::new();
    for record in &kept {
        let Some(item) = by_id.get(record.provenance.id.as_str()) else {
            continue;
        };
        let group = groups.entry(item.project.as_str()).or_default();
        let list = if item.bot { &mut group.1 } else { &mut group.0 };
        list.push(item_vm(item, now));
    }

    let mut configured: Vec<_> = config
        .projects
        .iter()
        .filter(|project| !project.muted)
        .collect();
    configured.sort_by_key(|project| !project.pinned);
    let mut projects: Vec<ProjectVm> = Vec::new();
    let mut add = |id: &str, name: &str, color: &str, pinned: bool, collapsed: bool| {
        let Some((list, bots)) = groups.remove(id) else {
            return;
        };
        let live = || {
            items
                .iter()
                .filter(|item| item.project == id && !item.is_snoozed(now))
        };
        projects.push(ProjectVm {
            id: id.to_owned(),
            name: name.to_owned(),
            color: color.to_owned(),
            pinned,
            collapsed,
            action_count: count(
                live().filter(|item| matches!(item.state, State::ActionRequired(_)) && !item.bot),
            ),
            overdue_count: count(live().filter(|item| item.is_overdue(now))),
            items: list,
            bots: bot_group(bots),
        });
    };
    for project in configured {
        add(
            &project.id,
            &project.name,
            &project.color,
            project.pinned,
            false,
        );
    }
    add(OTHERS, "Autres", "gray", false, true);

    Snapshot {
        needs_setup: config.projects.is_empty(),
        github_connected: false,
        todoist_connected: false,
        view: view.map_or_else(String::new, |view| view.id.clone()),
        views: views
            .iter()
            .map(|view| ViewChoice {
                id: view.id.clone(),
                title: view.title.clone(),
            })
            .collect(),
        badge: badge(items, config, now),
        projects,
        issues,
        refreshed_at,
    }
}

fn count<'a>(items: impl Iterator<Item = &'a Item>) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

fn bot_group(bots: Vec<ItemVm>) -> Option<BotGroup> {
    if bots.is_empty() {
        return None;
    }
    let label = match bots.len() {
        1 => "1 mise à jour de dépendance".to_owned(),
        count => format!("{count} mises à jour de dépendances"),
    };
    Some(BotGroup { label, items: bots })
}

fn badge(items: &[Item], config: &Config, now: DateTime<FixedOffset>) -> Badge {
    let muted = |project: &str| {
        config
            .projects
            .iter()
            .any(|known| known.id == project && known.muted)
    };
    let live = || {
        items
            .iter()
            .filter(|item| !item.is_snoozed(now) && !item.bot && !muted(&item.project))
    };
    Badge {
        count: count(live().filter(|item| item.priority == Priority::P1 || item.is_overdue(now))),
        alert: live().any(|item| item.is_overdue(now)),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::source::Provenance;
    use crate::view::default_views;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-08T10:00:00+01:00").unwrap()
    }

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn item(id: &str, project: &str) -> Item {
        Item {
            id: id.to_owned(),
            project: project.to_owned(),
            kind: Kind::Todo,
            title: id.to_owned(),
            context: String::new(),
            url: String::new(),
            priority: Priority::P4,
            due: None,
            deadline: None,
            state: State::Planned,
            updated_at: DateTime::default(),
            requested_at: None,
            bot: false,
            snoozed_until: None,
            provenance: Provenance {
                source: "test",
                id: id.to_owned(),
                version: None,
            },
        }
    }

    fn config() -> Config {
        Config::parse(
            r#"
            [[project]]
            id = "a"
            name = "A"
            [[project]]
            id = "b"
            name = "B"
            pinned = true
            [[project]]
            id = "muted"
            name = "Muted"
            muted = true
            [[project]]
            id = "empty"
            name = "Empty"
            "#,
            "test",
        )
        .unwrap()
    }

    fn snapshot(items: &[Item], view: &str) -> Snapshot {
        build(
            items,
            &config(),
            &default_views(),
            view,
            now(),
            Vec::new(),
            None,
        )
    }

    #[test]
    fn projects_are_ordered_and_hidden_as_configured() {
        let items = [
            item("1", "a"),
            item("2", "b"),
            item("3", "muted"),
            item("4", OTHERS),
        ];
        let snapshot = snapshot(&items, "all");
        let ids: Vec<_> = snapshot
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect();
        assert_eq!(ids, ["b", "a", OTHERS]);
        assert!(snapshot.projects[2].collapsed);
    }

    #[test]
    fn the_sort_of_the_view_reaches_each_project() {
        let mut low = item("low", "a");
        low.priority = Priority::P3;
        let mut late = item("late", "a");
        late.due = Some(Due::Date(date("2026-11-01")));
        let mut high = item("high", "a");
        high.priority = Priority::P1;
        let mut waiting = item("waiting", "a");
        waiting.priority = Priority::P1;
        waiting.state = State::Waiting;
        let snapshot = snapshot(&[low, waiting, high, late], "all");
        let ids: Vec<_> = snapshot.projects[0]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["late", "high", "low", "waiting"]);
    }

    #[test]
    fn bots_fold_on_one_line() {
        let mut bots: Vec<Item> = ["x", "y"].iter().map(|id| item(id, "a")).collect();
        for bot in &mut bots {
            bot.bot = true;
        }
        bots.push(item("person", "a"));
        let project = &snapshot(&bots, "all").projects[0];
        assert_eq!(project.items.len(), 1);
        assert_eq!(
            project.bots.as_ref().unwrap().label,
            "2 mises à jour de dépendances"
        );
    }

    #[test]
    fn snoozed_items_leave_the_list_until_their_time() {
        let mut snoozed = item("s", "a");
        snoozed.snoozed_until = Some(now().with_timezone(&Utc) + Duration::hours(1));
        assert_eq!(snapshot(std::slice::from_ref(&snoozed), "all").projects, []);
        snoozed.snoozed_until = Some(now().with_timezone(&Utc) - Duration::hours(1));
        assert_eq!(snapshot(&[snoozed], "all").projects.len(), 1);
    }

    #[test]
    fn the_badge_counts_p1_and_late_and_turns_red_when_late() {
        let mut p1 = item("p1", "a");
        p1.priority = Priority::P1;
        assert_eq!(
            snapshot(std::slice::from_ref(&p1), "all").badge,
            Badge {
                count: 1,
                alert: false
            }
        );
        let mut late = item("late", "a");
        late.due = Some(Due::Date(date("2026-11-01")));
        assert_eq!(
            snapshot(&[p1, late], "all").badge,
            Badge {
                count: 2,
                alert: true
            }
        );
    }

    #[test]
    fn today_shows_late_and_today_only() {
        let mut late = item("late", "a");
        late.due = Some(Due::Date(date("2026-11-01")));
        let mut today = item("today", "a");
        today.due = Some(Due::Date(date("2026-11-08")));
        let mut later = item("later", "a");
        later.due = Some(Due::Date(date("2026-11-20")));
        let undated = item("undated", "a");
        let snapshot = snapshot(&[late, today, later, undated], "today");
        let ids: Vec<_> = snapshot.projects[0]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["late", "today"]);
    }

    #[test]
    fn labels_read_naturally() {
        let today = date("2026-11-08");
        assert_eq!(date_label(today, today), "aujourd'hui");
        assert_eq!(date_label(date("2026-11-09"), today), "demain");
        assert_eq!(date_label(date("2026-11-05"), today), "en retard de 3 j");
        assert_eq!(date_label(date("2026-11-13"), today), "ven.");
        assert_eq!(date_label(date("2026-12-24"), today), "24 déc.");

        let mut both = item("x", "a");
        both.due = Some(Due::Date(date("2026-11-09")));
        both.deadline = Some(date("2026-11-13"));
        assert_eq!(
            due_label(&both, now()).unwrap(),
            "prévu demain · limite ven."
        );

        let mut review = item("x", "a");
        review.state = State::ActionRequired(Reason::ToReview);
        review.requested_at = Some(now().with_timezone(&Utc) - Duration::days(3));
        assert_eq!(
            state_label(&review, now()).unwrap(),
            "à reviewer depuis 3 j"
        );
    }

    #[test]
    fn only_todoist_tasks_can_be_completed() {
        let task = item_vm(&item("td:1", "a"), now());
        assert!(task.actions.contains(&"complete".to_owned()));
        let pr = item_vm(&item("gh-pr:a/b#1", "a"), now());
        assert!(!pr.actions.contains(&"complete".to_owned()));
    }

    #[test]
    fn only_my_prs_ready_to_merge_can_be_merged() {
        let mut ready = item("gh-pr:a/b#1", "a");
        ready.kind = Kind::MyPr;
        ready.state = State::ActionRequired(Reason::ToMerge);
        assert!(item_vm(&ready, now()).actions.contains(&"merge".to_owned()));
        ready.state = State::Waiting;
        assert!(!item_vm(&ready, now()).actions.contains(&"merge".to_owned()));
    }

    #[test]
    fn a_snapshot_survives_json() {
        let snapshot = snapshot(&[item("1", "a")], "all");
        let text = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(serde_json::from_str::<Snapshot>(&text).unwrap(), snapshot);
    }
}
