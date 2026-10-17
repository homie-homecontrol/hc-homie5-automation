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

/// Parses one rule document. On failure the error names the rule and the first trigger that
/// does not parse, since the untagged trigger enum alone only reports "did not match any variant".
pub fn parse_rule(doc: &str) -> Result<Rule, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(doc).map_err(|err| {
        let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(doc) else {
            return err;
        };
        let mut context = match value.get("name").and_then(|name| name.as_str()) {
            Some(name) => format!("rule \"{name}\""),
            None => "rule without name".to_string(),
        };
        let failing_trigger = value
            .get("triggers")
            .and_then(|triggers| triggers.as_sequence())
            .and_then(|triggers| {
                triggers
                    .iter()
                    .enumerate()
                    .find(|(_, trigger)| RuleTrigger::deserialize((*trigger).clone()).is_err())
            });
        if let Some((i, trigger)) = failing_trigger {
            let keys: Vec<&str> = trigger
                .as_mapping()
                .map(|map| map.keys().filter_map(|key| key.as_str()).collect())
                .unwrap_or_default();
            context.push_str(&format!(", trigger {} (keys: {})", i + 1, keys.join(", ")));
        }
        serde::de::Error::custom(format!("{context}: {err}"))
    })
}

impl Rule {
    /// Returns problems that let the rule load but keep parts of it from ever working.
    pub fn validate(&self) -> Vec<String> {
        self.triggers
            .iter()
            .enumerate()
            .filter_map(|(i, trigger)| match trigger {
                RuleTrigger::PropertyTriggered {
                    properties, queries, ..
                }
                | RuleTrigger::PropertyChanged {
                    properties, queries, ..
                }
                | RuleTrigger::OnSetEventTrigger {
                    properties, queries, ..
                } if properties.is_empty() && queries.is_empty() => {
                    Some(format!("trigger {} has neither properties nor queries and never fires", i + 1))
                }
                _ => None,
            })
            .collect()
    }
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

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn trigger_error_names_rule_and_trigger() {
        let doc = "name: kitchen-light\ntriggers:\n  - timer_id: t\n  - properties: [dev/node/prop]\n    chnged: {}\nactions: []\n";
        let err = parse_rule(doc).unwrap_err().to_string();
        assert!(err.contains("kitchen-light"), "{err}");
        assert!(err.contains("trigger 2"), "{err}");
        assert!(err.contains("chnged"), "{err}");
    }

    #[test]
    fn valid_rule_parses() {
        let rule = parse_rule("name: r\ntriggers:\n  - timer_id: t\nactions: []\n").unwrap();
        assert_eq!(rule.name, "r");
    }

    #[test]
    fn unknown_field_rejects_trigger_variant() {
        assert!(serde_yaml_ng::from_str::<RuleTrigger>("timer_id: t\n").is_ok());
        assert!(serde_yaml_ng::from_str::<RuleTrigger>("timer_id: t\nextra: 1\n").is_err());
    }
}

#[cfg(test)]
mod validate_tests {
    use super::*;

    fn rule(trigger: &str) -> Rule {
        serde_yaml_ng::from_str(&format!("name: r\ntriggers:\n  - {trigger}\nactions:\n  - type: run\n    script: x\n"))
            .unwrap()
    }

    #[test]
    fn warns_on_property_triggers_without_properties_or_queries() {
        for trigger in [
            "trigger_value: { operator: matchAlways }",
            "changed: {}",
            "set_value: { operator: matchAlways }",
        ] {
            let warnings = rule(trigger).validate();
            assert_eq!(warnings.len(), 1, "{trigger}: {warnings:?}");
        }
    }

    #[test]
    fn no_warning_for_property_trigger_with_properties() {
        assert!(rule("{ properties: [dev/node/prop], trigger_value: { operator: matchAlways } }")
            .validate()
            .is_empty());
        assert!(rule("{ timer_id: t }").validate().is_empty());
    }
}

#[cfg(test)]
mod doc_example_tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Triggers {
        #[allow(dead_code)]
        triggers: Vec<RuleTrigger>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Actions {
        #[allow(dead_code)]
        actions: Vec<RuleAction>,
    }

    #[derive(Deserialize)]
    struct While {
        #[allow(dead_code)]
        r#while: WhileConditionSet,
    }

    /// Parses a YAML example by its first key. Placeholder templates (`<...>`) and fragments
    /// that are not a rule, trigger, action or while list are skipped.
    fn check_example(block: &str) -> Option<Result<(), serde_yaml_ng::Error>> {
        if block.contains('<') {
            return None;
        }
        let first = block
            .lines()
            .map(str::trim_start)
            .find(|line| !line.is_empty() && !line.starts_with('#'))?;
        Some(if first.starts_with("name:") {
            serde_yaml_ng::from_str::<Rule>(block).map(drop)
        } else if first.starts_with("triggers:") {
            serde_yaml_ng::from_str::<Triggers>(block).map(drop)
        } else if first.starts_with("actions:") {
            serde_yaml_ng::from_str::<Actions>(block).map(drop)
        } else if first.starts_with("while:") {
            serde_yaml_ng::from_str::<While>(block).map(drop)
        } else if first.starts_with("- type:") {
            serde_yaml_ng::from_str::<Vec<RuleAction>>(block).map(drop)
        } else if first.starts_with("- ") {
            serde_yaml_ng::from_str::<Vec<RuleTrigger>>(block).map(drop)
        } else {
            return None;
        })
    }

    fn yaml_blocks(markdown: &str) -> Vec<(usize, String)> {
        let mut blocks = Vec::new();
        let mut current: Option<(usize, String)> = None;
        for (i, line) in markdown.lines().enumerate() {
            let trimmed = line.trim_start();
            match current.as_mut() {
                None if trimmed.starts_with("```yaml") || trimmed.starts_with("```yml") => {
                    current = Some((i + 1, String::new()))
                }
                Some(_) if trimmed.starts_with("```") => blocks.extend(current.take()),
                Some((_, block)) => {
                    block.push_str(line);
                    block.push('\n');
                }
                None => {}
            }
        }
        blocks
    }

    #[test]
    fn rules_doc_examples_parse() {
        let mut failures = Vec::new();
        for (line, block) in yaml_blocks(include_str!("../../../docs/rules.md")) {
            if let Some(Err(err)) = check_example(&block) {
                failures.push(format!("docs/rules.md:{line}: {err}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
