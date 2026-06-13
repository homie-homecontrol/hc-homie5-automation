mod action;
mod timer;
mod trigger;
mod while_cond;

use std::time::Duration;

pub use action::*;
use serde::{de::Visitor, Deserialize, Deserializer};
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

/// Parses a duration string of the form `<integer><unit>`.
///
/// Supported units, checked in the order `ms, s, m, h, d` (so that the `ms`
/// suffix is matched before the bare `s` suffix):
///
/// - `ms` — milliseconds
/// - `s`  — seconds
/// - `m`  — minutes
/// - `h`  — hours
/// - `d`  — days
///
/// The numeric portion must be a valid `u64`. Any malformed input (unknown
/// unit, non-integer number, embedded whitespace, empty string, …) returns an
/// error message — this function never panics.
///
/// Public (serde-free) variant so non-serde callers (e.g. the binary's
/// settings parsing) can reuse the exact same duration syntax.
pub fn parse_duration(v: &str) -> Result<Duration, String> {
    let parse_num = |num: &str| -> Result<u64, String> {
        num.parse::<u64>()
            .map_err(|_| format!("invalid duration value: '{}'", v))
    };

    // Order matters: `ms` must be checked before the bare `s` suffix.
    if let Some(num) = v.strip_suffix("ms") {
        Ok(Duration::from_millis(parse_num(num)?))
    } else if let Some(num) = v.strip_suffix('s') {
        Ok(Duration::from_secs(parse_num(num)?))
    } else if let Some(num) = v.strip_suffix('m') {
        Ok(Duration::from_secs(parse_num(num)? * 60))
    } else if let Some(num) = v.strip_suffix('h') {
        Ok(Duration::from_secs(parse_num(num)? * 60 * 60))
    } else if let Some(num) = v.strip_suffix('d') {
        Ok(Duration::from_secs(parse_num(num)? * 60 * 60 * 24))
    } else {
        Err(format!("invalid duration unit in '{}' (expected one of: ms, s, m, h, d)", v))
    }
}

/// serde adapter around [`parse_duration`].
fn parse_duration_str<E>(v: &str) -> Result<Duration, E>
where
    E: serde::de::Error,
{
    parse_duration(v).map_err(E::custom)
}

struct DurationVisitor;

impl Visitor<'_> for DurationVisitor {
    type Value = Duration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a duration string (e.g., 5s, 2m, 1h, 1d, 10ms)")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        parse_duration_str(v)
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

#[cfg(test)]
mod duration_tests {
    use super::*;
    use serde::de::value::{Error as ValueError, StrDeserializer};
    use serde::de::IntoDeserializer;

    fn parse(v: &str) -> Result<Duration, ValueError> {
        super::parse_duration_str::<ValueError>(v)
    }

    fn parse_optional(v: &str) -> Result<Option<Duration>, ValueError> {
        let de: StrDeserializer<ValueError> = v.into_deserializer();
        deserialize_optional_duration(de)
    }

    #[test]
    fn parses_all_valid_units() {
        assert_eq!(parse("10ms").unwrap(), Duration::from_millis(10));
        assert_eq!(parse("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse("3h").unwrap(), Duration::from_secs(3 * 3600));
        assert_eq!(parse("1d").unwrap(), Duration::from_secs(86400));
        assert_eq!(parse("0s").unwrap(), Duration::from_secs(0));
    }

    #[test]
    fn ms_takes_precedence_over_bare_s() {
        // "100ms" must be 100 milliseconds, not parsed via the bare `s` suffix.
        assert_eq!(parse("100ms").unwrap(), Duration::from_millis(100));
    }

    #[test]
    fn malformed_inputs_error_without_panicking() {
        // Each of these used to panic via `.unwrap()` on parse().
        assert!(parse("1.5s").is_err());
        assert!(parse("fasts").is_err());
        assert!(parse("s").is_err());
        assert!(parse("").is_err());
        assert!(parse("5 s").is_err());
        assert!(parse("ms").is_err());
        assert!(parse("10x").is_err());
        assert!(parse("-5s").is_err());
    }

    #[test]
    fn optional_treats_empty_and_null_as_none() {
        assert_eq!(parse_optional("").unwrap(), None);
        assert_eq!(parse_optional("null").unwrap(), None);
    }

    #[test]
    fn optional_parses_valid_and_rejects_malformed() {
        assert_eq!(parse_optional("5s").unwrap(), Some(Duration::from_secs(5)));
        assert_eq!(parse_optional("1h").unwrap(), Some(Duration::from_secs(3600)));
        assert!(parse_optional("1.5s").is_err());
        assert!(parse_optional("fasts").is_err());
    }
}
