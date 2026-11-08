// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reads what a person types for "until when", and how long ago things happened in working days.

use chrono::{
    DateTime, Datelike, Days, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Weekday,
};

const MORNING: NaiveTime = match NaiveTime::from_hms_opt(8, 0, 0) {
    Some(time) => time,
    None => panic!("8:00 is a valid time"),
};

fn at_morning(date: NaiveDate, now: DateTime<FixedOffset>) -> Option<DateTime<FixedOffset>> {
    now.timezone()
        .from_local_datetime(&date.and_time(MORNING))
        .single()
}

fn next_weekday(from: NaiveDate, weekday: Weekday) -> NaiveDate {
    let today = from.weekday().num_days_from_monday();
    let target = weekday.num_days_from_monday();
    let ahead = (7 + target - today) % 7;
    from + Days::new(u64::from(if ahead == 0 { 7 } else { ahead }))
}

/// Understands `2h`, `30m`, `3d`, `tomorrow`, `monday` and `2026-12-01`.
#[must_use]
pub fn parse_until(text: &str, now: DateTime<FixedOffset>) -> Option<DateTime<FixedOffset>> {
    let text = text.trim().to_lowercase();
    let today = now.date_naive();
    if let Some(duration) = parse_duration(&text) {
        return Some(now + duration);
    }
    if let Ok(date) = NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        return at_morning(date, now);
    }
    let date = match text.as_str() {
        "tomorrow" | "demain" => today + Days::new(1),
        "monday" | "lundi" => next_weekday(today, Weekday::Mon),
        "tuesday" | "mardi" => next_weekday(today, Weekday::Tue),
        "wednesday" | "mercredi" => next_weekday(today, Weekday::Wed),
        "thursday" | "jeudi" => next_weekday(today, Weekday::Thu),
        "friday" | "vendredi" => next_weekday(today, Weekday::Fri),
        "saturday" | "samedi" => next_weekday(today, Weekday::Sat),
        "sunday" | "dimanche" => next_weekday(today, Weekday::Sun),
        _ => return None,
    };
    at_morning(date, now)
}

/// Understands `30s`, `5m`, `2h` and `3d`.
#[must_use]
pub fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    let split = text.len().checked_sub(1)?;
    let (number, unit) = (text.get(..split)?, text.get(split..)?);
    let number: i64 = number.parse().ok()?;
    match unit {
        "s" => Duration::try_seconds(number),
        "m" => Duration::try_minutes(number),
        "h" => Duration::try_hours(number),
        "d" => Duration::try_days(number),
        _ => None,
    }
}

/// Monday-to-Friday days between two dates, not counting the first.
#[must_use]
pub fn working_days_between(from: NaiveDate, to: NaiveDate) -> u32 {
    let mut count = 0;
    let mut day = from;
    while day < to {
        day = day + Days::new(1);
        if !matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-08T10:00:00+01:00").unwrap()
    }

    #[test]
    fn reads_durations_and_days() {
        assert_eq!(
            parse_until("2h", now()).unwrap().to_rfc3339(),
            "2026-11-08T12:00:00+01:00"
        );
        assert_eq!(
            parse_until("tomorrow", now()).unwrap().to_rfc3339(),
            "2026-11-09T08:00:00+01:00"
        );
        assert_eq!(
            parse_until("2026-12-01", now()).unwrap().to_rfc3339(),
            "2026-12-01T08:00:00+01:00"
        );
        assert_eq!(parse_until("someday", now()), None);
    }

    #[test]
    fn a_weekday_is_always_in_the_future() {
        // 2026-11-08 is a Sunday.
        assert_eq!(
            parse_until("sunday", now())
                .unwrap()
                .date_naive()
                .to_string(),
            "2026-11-15"
        );
        assert_eq!(
            parse_until("lundi", now())
                .unwrap()
                .date_naive()
                .to_string(),
            "2026-11-09"
        );
    }

    #[test]
    fn working_days_skip_the_weekend() {
        let friday = NaiveDate::from_ymd_opt(2026, 11, 6).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let tuesday = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        assert_eq!(working_days_between(friday, monday), 1);
        assert_eq!(working_days_between(friday, tuesday), 2);
        assert_eq!(working_days_between(monday, monday), 0);
    }
}
