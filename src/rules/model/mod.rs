mod action;
mod subject;
mod timer;
mod trigger;
mod while_cond;

use std::time::Duration;

pub use action::*;
use serde::{de::Visitor, Deserialize, Deserializer};
pub use subject::*;
pub use timer::*;
pub use trigger::*;
pub use while_cond::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub triggers: Vec<RuleTrigger>,
    pub actions: Vec<RuleAction>,
}

struct DurationVisitor;

impl Visitor<'_> for DurationVisitor {
    type Value = Duration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a duration string (e.g., 5s, 2m, 1d, 10ms)")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let duration = if v.ends_with("ms") {
            Duration::from_millis(v.trim_end_matches("ms").parse::<u64>().unwrap())
        } else if v.ends_with("s") {
            Duration::from_secs(v.trim_end_matches("s").parse::<u64>().unwrap())
        } else if v.ends_with("m") {
            Duration::from_secs(v.trim_end_matches("m").parse::<u64>().unwrap() * 60)
        } else if v.ends_with("d") {
            Duration::from_secs(v.trim_end_matches("d").parse::<u64>().unwrap() * 60 * 60 * 24)
        } else {
            return Err(serde::de::Error::custom("Invalid duration unit"));
        };

        Ok(duration)
    }
}

struct OptionalDurationVisitor;

impl Visitor<'_> for OptionalDurationVisitor {
    type Value = Option<Duration>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an optional duration string (e.g., 5s, 2m, 1d, 10ms, or null)")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if v.is_empty() || v == "null" {
            Ok(None)
        } else {
            let duration = DurationVisitor.visit_str(v)?;
            Ok(Some(duration))
        }
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }
}

pub fn deserialize_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_string(DurationVisitor)
}

pub fn deserialize_optional_duration<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_string(OptionalDurationVisitor)
}
