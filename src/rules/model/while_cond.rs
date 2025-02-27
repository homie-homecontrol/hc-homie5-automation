use chrono::{Datelike, Local, NaiveTime, Weekday as ChronoWeekday};
use hc_homie5::ValueCondition;
use homie5::HomieValue;
use serde::Deserialize;

use super::Subject;

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
    pub subject: Subject,
    pub condition: ValueCondition<HomieValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)] // Deserialize based on field structure, without a "type" tag
pub enum TimeWhileCondition {
    // All fields are required
    Standard {
        after: NaiveTime,
        before: NaiveTime,
        weekdays: Vec<Weekday>,
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
    /// Evaluates the condition based on the current date and time.
    pub fn evaluate(&self) -> bool {
        let now = Local::now();
        let current_time = now.time();
        let current_weekday = now.weekday();

        match self {
            // Standard: all fields are required
            TimeWhileCondition::Standard {
                after,
                before,
                weekdays,
            } => current_time >= *after && current_time <= *before && weekdays.contains(&map_weekday(current_weekday)),

            // Before: only `before` is required
            TimeWhileCondition::Before { before, weekdays } => {
                current_time <= *before
                    && weekdays
                        .as_ref()
                        .map_or(true, |days| days.contains(&map_weekday(current_weekday)))
            }

            // After: only `after` is required
            TimeWhileCondition::After { after, weekdays } => {
                current_time >= *after
                    && weekdays
                        .as_ref()
                        .map_or(true, |days| days.contains(&map_weekday(current_weekday)))
            }

            // Weekdays: only `weekdays` is required
            TimeWhileCondition::Weekdays { weekdays } => weekdays.contains(&map_weekday(current_weekday)),
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
