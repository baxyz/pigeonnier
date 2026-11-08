// SPDX-License-Identifier: AGPL-3.0-or-later

//! Which notifications to send, as a pure decision: the daemon shows them and records the keys.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Days, FixedOffset, NaiveTime, Utc, Weekday};

use crate::config::Config;
use crate::item::{Due, Item, Kind, OTHERS, Reason, State};

/// Above this many in one pass, notifications merge into one.
const BURST: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub key: String,
    pub item_id: Option<String>,
    pub url: Option<String>,
    pub title: String,
    pub body: String,
    /// Pairs of action id and label.
    pub actions: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub send: Vec<Notification>,
    /// Keys to record as sent, including what a merged notification covers.
    pub mark: Vec<String>,
}

fn action(id: &str, label: &str) -> (String, String) {
    (id.to_owned(), label.to_owned())
}

fn pr_notification(item: &Item, reason: Reason) -> Notification {
    let title = match reason {
        Reason::ToReview => "Review demandée",
        Reason::ChangesRequested => "Changements demandés",
        Reason::CiFailed => "CI rouge",
        Reason::Conflicts => "Conflits",
        Reason::ToMerge => "Prête à merger",
    };
    Notification {
        key: format!("{}|{reason:?}", item.id),
        item_id: Some(item.id.clone()),
        url: Some(item.url.clone()),
        title: title.to_owned(),
        body: format!("{} ({})", item.title, item.context),
        actions: vec![
            action("open", "Ouvrir"),
            action("snooze:2h", "Reporter 2 h"),
        ],
    }
}

fn timed_notification(item: &Item, at: DateTime<Utc>) -> Notification {
    Notification {
        key: format!("{}|at|{}", item.id, at.timestamp()),
        item_id: Some(item.id.clone()),
        url: Some(item.url.clone()),
        title: item.title.clone(),
        body: item.context.clone(),
        actions: vec![
            action("complete", "Fait"),
            action("snooze:1h", "Reporter 1 h"),
            action("snooze:tomorrow", "Demain"),
        ],
    }
}

fn deadline_notification(item: &Item, moment: &str, when: &str) -> Notification {
    let date = item
        .deadline
        .map(|date| date.to_string())
        .unwrap_or_default();
    Notification {
        key: format!("{}|deadline-{moment}|{date}", item.id),
        item_id: Some(item.id.clone()),
        url: Some(item.url.clone()),
        title: format!("Échéance {when}"),
        body: format!("{} ({})", item.title, item.context),
        actions: vec![
            action("open", "Ouvrir"),
            action("snooze:tomorrow", "Reporter"),
        ],
    }
}

fn digest(items: &[Item], config: &Config, now: DateTime<FixedOffset>) -> Notification {
    let mut per_project: BTreeMap<&str, (u32, u32, u32)> = BTreeMap::new();
    for item in items {
        let counts = per_project.entry(item.project.as_str()).or_default();
        if item.kind == Kind::Review {
            counts.0 += 1;
        }
        if item.kind == Kind::ClaudeAction {
            counts.1 += 1;
        }
        let today = item
            .effective_due()
            .is_some_and(|due| due.local_date(*now.offset()) == now.date_naive());
        if today {
            counts.2 += 1;
        }
    }
    let name_of = |id: &str| {
        if id == OTHERS {
            return "Autres".to_owned();
        }
        config
            .projects
            .iter()
            .find(|project| project.id == id)
            .map_or_else(|| id.to_owned(), |project| project.name.clone())
    };
    let plural = |count: u32, singular: &str, several: &str| match count {
        1 => format!("1 {singular}"),
        count => format!("{count} {several}"),
    };
    let lines: Vec<String> = per_project
        .into_iter()
        .filter_map(|(id, (reviews, claude, today))| {
            let mut parts = Vec::new();
            if reviews > 0 {
                parts.push(plural(reviews, "review", "reviews"));
            }
            if claude > 0 {
                parts.push(plural(claude, "action Claude", "actions Claude"));
            }
            if today > 0 {
                parts.push(plural(
                    today,
                    "échéance aujourd'hui",
                    "échéances aujourd'hui",
                ));
            }
            (!parts.is_empty()).then(|| format!("{} : {}", name_of(id), parts.join(", ")))
        })
        .collect();
    Notification {
        key: format!("digest|{}", now.date_naive()),
        item_id: None,
        url: None,
        title: "Ce qui t'attend aujourd'hui".to_owned(),
        body: if lines.is_empty() {
            "Rien de particulier.".to_owned()
        } else {
            lines.join(" · ")
        },
        actions: vec![action("sidebar", "Ouvrir la sidebar")],
    }
}

/// Decides what to send now. `already_sent` says if a key was already recorded. With `seed`, on the very
/// first run, everything current is recorded and nothing is shown: what already waited is not news.
/// During quiet hours nothing is recorded either, so that it goes out when they end.
#[must_use]
pub fn plan(
    items: &[Item],
    config: &Config,
    now: DateTime<FixedOffset>,
    already_sent: &dyn Fn(&str) -> bool,
    seed: bool,
) -> Plan {
    let settings = &config.notifications;
    let time = now.time();
    if !settings.enabled || (!seed && settings.is_quiet(time)) {
        return Plan::default();
    }
    let muted = |project: &str| {
        config
            .projects
            .iter()
            .any(|known| known.id == project && known.muted)
    };
    let visible: Vec<Item> = items
        .iter()
        .filter(|item| !item.bot && !muted(&item.project) && !item.is_snoozed(now))
        .cloned()
        .collect();

    let mut candidates = Vec::new();
    let today = now.date_naive();
    let evening = NaiveTime::from_hms_opt(18, 0, 0).unwrap_or_default();
    let digest_time = settings.digest_time();
    for item in &visible {
        if let State::ActionRequired(reason) = item.state {
            candidates.push(pr_notification(item, reason));
        }
        if let Some(Due::DateTime(at)) = item.due
            && at <= now.with_timezone(&Utc)
            && at.with_timezone(now.offset()).date_naive() == today
        {
            candidates.push(timed_notification(item, at));
        }
        if let Some(deadline) = item.deadline {
            if Some(deadline) == today.checked_add_days(Days::new(1)) && time >= evening {
                candidates.push(deadline_notification(item, "eve", "demain"));
            }
            if deadline == today && time >= digest_time {
                candidates.push(deadline_notification(item, "day", "aujourd'hui"));
            }
        }
    }
    let working_day = !matches!(now.weekday(), Weekday::Sat | Weekday::Sun);
    if working_day && time >= digest_time {
        candidates.push(digest(&visible, config, now));
    }

    candidates.retain(|candidate| !already_sent(&candidate.key));
    let mark = candidates
        .iter()
        .map(|candidate| candidate.key.clone())
        .collect();
    if seed {
        return Plan {
            send: Vec::new(),
            mark,
        };
    }
    let shown = if candidates.len() > BURST {
        vec![Notification {
            key: format!("burst|{}", now.timestamp()),
            item_id: None,
            url: None,
            title: format!("{} nouveaux éléments", candidates.len()),
            body: String::new(),
            actions: vec![action("sidebar", "Ouvrir la sidebar")],
        }]
    } else {
        candidates
    };
    Plan { send: shown, mark }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::NaiveDate;

    use super::*;
    use crate::item::Priority;
    use crate::source::Provenance;

    /// A Monday.
    fn at(time: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(&format!("2026-11-09T{time}:00+01:00")).unwrap()
    }

    fn item(id: &str, state: State) -> Item {
        Item {
            id: id.to_owned(),
            project: "a".to_owned(),
            kind: Kind::MyPr,
            title: id.to_owned(),
            context: "ctx".to_owned(),
            url: format!("https://x/{id}"),
            priority: Priority::P2,
            due: None,
            deadline: None,
            state,
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
            [notifications]
            digest = "09:00"
            [[project]]
            id = "a"
            name = "A"
            [[project]]
            id = "muted"
            name = "Muted"
            muted = true
            "#,
            "test",
        )
        .unwrap()
    }

    fn never(_: &str) -> bool {
        false
    }

    fn ci_failed(id: &str) -> Item {
        item(id, State::ActionRequired(Reason::CiFailed))
    }

    fn keys(plan: &Plan) -> Vec<&str> {
        plan.send
            .iter()
            .map(|notification| notification.key.as_str())
            .collect()
    }

    #[test]
    fn a_state_notifies_once_and_a_new_state_again() {
        let config = config();
        let item = ci_failed("1");
        let first = plan(
            std::slice::from_ref(&item),
            &config,
            at("08:30"),
            &never,
            false,
        );
        assert_eq!(keys(&first), ["1|CiFailed"]);

        let sent: HashSet<String> = first.mark.into_iter().collect();
        let known = |key: &str| sent.contains(key);
        let again = plan(
            std::slice::from_ref(&item),
            &config,
            at("08:35"),
            &known,
            false,
        );
        assert_eq!(again.send, []);

        let merged = crate::item::Item {
            state: State::ActionRequired(Reason::ToMerge),
            ..item
        };
        let moved = plan(&[merged], &config, at("08:40"), &known, false);
        assert_eq!(keys(&moved), ["1|ToMerge"]);
    }

    #[test]
    fn waiting_bots_muted_and_snoozed_stay_silent() {
        let config = config();
        let waiting = item("w", State::Waiting);
        let mut bot = ci_failed("b");
        bot.bot = true;
        let mut muted = ci_failed("m");
        muted.project = "muted".to_owned();
        let mut snoozed = ci_failed("s");
        snoozed.snoozed_until = Some(at("10:00").with_timezone(&Utc));
        let plan = plan(
            &[waiting, bot, muted, snoozed],
            &config,
            at("08:30"),
            &never,
            false,
        );
        assert_eq!(plan.send, []);
    }

    #[test]
    fn quiet_hours_delay_without_losing() {
        let config = config();
        let item = ci_failed("1");
        let night = plan(
            std::slice::from_ref(&item),
            &config,
            at("22:00"),
            &never,
            false,
        );
        assert_eq!(night, Plan::default());
        let morning = plan(&[item], &config, at("08:30"), &never, false);
        assert_eq!(morning.send.len(), 1);
    }

    #[test]
    fn a_burst_merges_into_one_and_covers_them_all() {
        let config = config();
        let items: Vec<Item> = (0..5).map(|index| ci_failed(&index.to_string())).collect();
        let plan = plan(&items, &config, at("08:30"), &never, false);
        assert_eq!(plan.send.len(), 1);
        assert_eq!(plan.send[0].title, "5 nouveaux éléments");
        assert_eq!(plan.mark.len(), 5);
    }

    #[test]
    fn the_first_run_records_without_showing() {
        let config = config();
        let plan = plan(&[ci_failed("1")], &config, at("08:30"), &never, true);
        assert_eq!(plan.send, []);
        assert_eq!(plan.mark, ["1|CiFailed"]);
    }

    #[test]
    fn a_timed_task_notifies_at_its_time() {
        let config = config();
        let mut task = item("td:1", State::Planned);
        task.due = Some(Due::DateTime(at("08:15").with_timezone(&Utc)));
        let early = plan(
            std::slice::from_ref(&task),
            &config,
            at("08:10"),
            &never,
            false,
        );
        assert_eq!(early.send, []);
        let plan = plan(&[task], &config, at("08:20"), &never, false);
        assert_eq!(plan.send.len(), 1);
        assert!(plan.send[0].actions.iter().any(|(id, _)| id == "complete"));
    }

    #[test]
    fn a_deadline_warns_the_evening_before_and_on_the_day() {
        let config = config();
        let mut task = item("td:1", State::Planned);
        task.deadline = NaiveDate::from_ymd_opt(2026, 11, 10);
        let eve = plan(
            std::slice::from_ref(&task),
            &config,
            at("18:30"),
            &never,
            false,
        );
        assert!(keys(&eve).iter().any(|key| key.contains("deadline-eve")));
        assert!(
            !keys(&plan(
                std::slice::from_ref(&task),
                &config,
                at("17:00"),
                &never,
                false
            ))
            .iter()
            .any(|key| key.contains("deadline"))
        );

        task.deadline = NaiveDate::from_ymd_opt(2026, 11, 9);
        let day = plan(&[task], &config, at("09:30"), &never, false);
        assert!(keys(&day).iter().any(|key| key.contains("deadline-day")));
    }

    #[test]
    fn the_digest_sums_up_by_project_on_working_days() {
        let config = config();
        let mut review = item("r", State::ActionRequired(Reason::ToReview));
        review.kind = Kind::Review;
        let done = |key: &str| key.starts_with("r|");
        let monday = plan(&[review.clone()], &config, at("09:30"), &done, false);
        assert_eq!(monday.send.len(), 1);
        assert_eq!(monday.send[0].key, "digest|2026-11-09");
        assert_eq!(monday.send[0].body, "A : 1 review");

        let sunday = DateTime::parse_from_rfc3339("2026-11-08T09:30:00+01:00").unwrap();
        assert_eq!(plan(&[review], &config, sunday, &done, false).send, []);
    }
}
