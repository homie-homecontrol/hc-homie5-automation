use chrono::{Datelike, Local, NaiveTime, Weekday as ChronoWeekday};
use hc_homie5::value::ValueCondition;
use homie5::HomieValue;
use serde::Deserialize;

use homie5::PropertyRef;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WhileConditionSet {
    Single(WhileCondition),
    Multiple(Vec<WhileCondition>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WhileCondition {
    PropertyWhileCondition(PropertyWhileCondition),
    TimeWhileCondition(TimeWhileCondition),
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyWhileCondition {
    pub property: PropertyRef,
    pub condition: ValueCondition<HomieValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)] // Deserialize based on field structure, without a "type" tag
pub enum TimeWhileCondition {
    // `after` and `before` are required; `weekdays` is optional
    Standard {
        after: NaiveTime,
        before: NaiveTime,
        weekdays: Option<Vec<Weekday>>,
    },

    // Only `before` is required; `after` is not allowed
    Before {
        before: NaiveTime,
        weekdays: Option<Vec<Weekday>>, // Optional
    },

    // Only `after` is required; `before` is not allowed
    After {
        after: NaiveTime,
        weekdays: Option<Vec<Weekday>>, // Optional
    },

    // Only `weekdays` is required
    Weekdays {
        weekdays: Vec<Weekday>,
    },
}

impl TimeWhileCondition {
    /// Evaluates the condition based on the current local date and time.
    pub fn evaluate(&self) -> bool {
        let now = Local::now();
        self.evaluate_at(now.time(), now.weekday())
    }

    /// Evaluates the condition against an explicit time and weekday (pure, testable core).
    ///
    /// For `Standard` conditions the time window may cross midnight: if
    /// `after > before` (e.g. `after: 22:00, before: 06:00`) the window wraps
    /// around midnight and matches `t >= after || t <= before`.
    ///
    /// Note on weekdays with midnight-crossing windows: the weekday filter is
    /// always checked against the *current* local day. For a 22:00–06:00
    /// window the post-midnight half belongs to the *next* weekday, i.e. a
    /// `weekdays: [fri]` filter matches Friday 23:00 but the Saturday 05:00
    /// portion of the same night requires `sat` to be listed.
    pub fn evaluate_at(&self, now_time: NaiveTime, now_weekday: ChronoWeekday) -> bool {
        match self {
            // Standard: `after` and `before` required, `weekdays` optional.
            // Supports midnight-crossing windows (after > before).
            TimeWhileCondition::Standard {
                after,
                before,
                weekdays,
            } => {
                let in_window = if after <= before {
                    now_time >= *after && now_time <= *before
                } else {
                    now_time >= *after || now_time <= *before
                };
                in_window
                    && weekdays
                        .as_ref()
                        .is_none_or(|days| days.contains(&map_weekday(now_weekday)))
            }

            // Before: only `before` is required
            TimeWhileCondition::Before { before, weekdays } => {
                now_time <= *before
                    && weekdays
                        .as_ref()
                        .is_none_or(|days| days.contains(&map_weekday(now_weekday)))
            }

            // After: only `after` is required
            TimeWhileCondition::After { after, weekdays } => {
                now_time >= *after
                    && weekdays
                        .as_ref()
                        .is_none_or(|days| days.contains(&map_weekday(now_weekday)))
            }

            // Weekdays: only `weekdays` is required
            TimeWhileCondition::Weekdays { weekdays } => weekdays.contains(&map_weekday(now_weekday)),
        }
    }
}

/// Maps `chrono::Weekday` to the `Weekday` enum used in `TimeWhileCondition`.
fn map_weekday(weekday: ChronoWeekday) -> Weekday {
    match weekday {
        ChronoWeekday::Mon => Weekday::Mon,
        ChronoWeekday::Tue => Weekday::Tue,
        ChronoWeekday::Wed => Weekday::Wed,
        ChronoWeekday::Thu => Weekday::Thu,
        ChronoWeekday::Fri => Weekday::Fri,
        ChronoWeekday::Sat => Weekday::Sat,
        ChronoWeekday::Sun => Weekday::Sun,
    }
}
// Weekday enum to match the fixed weekday values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")] // Match the TypeScript short strings
pub enum Weekday {
    Sun,
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn after_before_deserializes_as_standard_variant() {
        // Previously misparsed as `Before`, silently dropping `after`.
        let cond: TimeWhileCondition = serde_yaml_ng::from_str("after: 22:00:00\nbefore: 06:00:00\n").unwrap();
        match cond {
            TimeWhileCondition::Standard {
                after,
                before,
                weekdays,
            } => {
                assert_eq!(after, t(22, 0));
                assert_eq!(before, t(6, 0));
                assert!(weekdays.is_none());
            }
            other => panic!("expected Standard variant, got {:?}", other),
        }
    }

    #[test]
    fn after_before_weekdays_deserializes_as_standard_variant() {
        let cond: TimeWhileCondition =
            serde_yaml_ng::from_str("after: 08:00:00\nbefore: 17:00:00\nweekdays: [mon, tue]\n").unwrap();
        match cond {
            TimeWhileCondition::Standard { weekdays, .. } => {
                assert_eq!(weekdays, Some(vec![Weekday::Mon, Weekday::Tue]));
            }
            other => panic!("expected Standard variant, got {:?}", other),
        }
    }

    #[test]
    fn standard_midnight_crossing_window() {
        let cond = TimeWhileCondition::Standard {
            after: t(22, 0),
            before: t(6, 0),
            weekdays: None,
        };
        assert!(cond.evaluate_at(t(23, 0), ChronoWeekday::Mon));
        assert!(cond.evaluate_at(t(5, 0), ChronoWeekday::Mon));
        assert!(cond.evaluate_at(t(22, 0), ChronoWeekday::Mon)); // inclusive lower bound
        assert!(cond.evaluate_at(t(6, 0), ChronoWeekday::Mon)); // inclusive upper bound
        assert!(!cond.evaluate_at(t(12, 0), ChronoWeekday::Mon));
        assert!(!cond.evaluate_at(t(21, 59), ChronoWeekday::Mon));
    }

    #[test]
    fn standard_non_crossing_window() {
        let cond = TimeWhileCondition::Standard {
            after: t(8, 0),
            before: t(17, 0),
            weekdays: None,
        };
        assert!(cond.evaluate_at(t(12, 0), ChronoWeekday::Mon));
        assert!(cond.evaluate_at(t(8, 0), ChronoWeekday::Mon));
        assert!(cond.evaluate_at(t(17, 0), ChronoWeekday::Mon));
        assert!(!cond.evaluate_at(t(18, 0), ChronoWeekday::Mon));
        assert!(!cond.evaluate_at(t(7, 59), ChronoWeekday::Mon));
    }

    #[test]
    fn standard_weekdays_none_ignores_weekday() {
        let cond = TimeWhileCondition::Standard {
            after: t(8, 0),
            before: t(17, 0),
            weekdays: None,
        };
        for wd in [
            ChronoWeekday::Mon,
            ChronoWeekday::Tue,
            ChronoWeekday::Wed,
            ChronoWeekday::Thu,
            ChronoWeekday::Fri,
            ChronoWeekday::Sat,
            ChronoWeekday::Sun,
        ] {
            assert!(cond.evaluate_at(t(12, 0), wd));
        }
    }

    #[test]
    fn standard_weekdays_some_filters() {
        let cond = TimeWhileCondition::Standard {
            after: t(8, 0),
            before: t(17, 0),
            weekdays: Some(vec![Weekday::Mon, Weekday::Fri]),
        };
        assert!(cond.evaluate_at(t(12, 0), ChronoWeekday::Mon));
        assert!(cond.evaluate_at(t(12, 0), ChronoWeekday::Fri));
        assert!(!cond.evaluate_at(t(12, 0), ChronoWeekday::Tue));
        assert!(!cond.evaluate_at(t(12, 0), ChronoWeekday::Sun));
    }

    #[test]
    fn standard_midnight_crossing_with_weekdays_checks_current_day() {
        // Weekday is checked against the *current* day: the post-midnight half
        // of a fri 22:00-06:00 window falls on saturday.
        let cond = TimeWhileCondition::Standard {
            after: t(22, 0),
            before: t(6, 0),
            weekdays: Some(vec![Weekday::Fri]),
        };
        assert!(cond.evaluate_at(t(23, 0), ChronoWeekday::Fri));
        assert!(!cond.evaluate_at(t(5, 0), ChronoWeekday::Sat));
    }

    #[test]
    fn before_only_variant() {
        let cond: TimeWhileCondition = serde_yaml_ng::from_str("before: 06:00:00\n").unwrap();
        assert!(matches!(cond, TimeWhileCondition::Before { .. }));
        assert!(cond.evaluate_at(t(5, 0), ChronoWeekday::Mon));
        assert!(!cond.evaluate_at(t(7, 0), ChronoWeekday::Mon));
    }

    #[test]
    fn after_only_variant() {
        let cond: TimeWhileCondition = serde_yaml_ng::from_str("after: 22:00:00\n").unwrap();
        assert!(matches!(cond, TimeWhileCondition::After { .. }));
        assert!(cond.evaluate_at(t(23, 0), ChronoWeekday::Mon));
        assert!(!cond.evaluate_at(t(21, 0), ChronoWeekday::Mon));
    }

    #[test]
    fn weekdays_only_variant() {
        let cond: TimeWhileCondition = serde_yaml_ng::from_str("weekdays: [sat, sun]\n").unwrap();
        assert!(matches!(cond, TimeWhileCondition::Weekdays { .. }));
        assert!(cond.evaluate_at(t(12, 0), ChronoWeekday::Sat));
        assert!(!cond.evaluate_at(t(12, 0), ChronoWeekday::Wed));
    }
}
