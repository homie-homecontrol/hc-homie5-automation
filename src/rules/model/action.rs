use super::{deserialize_optional_duration, TimerDef};
use crate::solar_events::SolarPhase;
use hc_homie5::value::{ValueMappingList, ValueMatcher};
use homie5::PropertyRef;
// use hc_homie5::{impl_value_matcher_for, AsMatchStr, ValueMappingList};
use homie5::client::QoS;
use homie5::HomieValue;
use serde::Deserialize;
use std::borrow::Cow;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum RuleAction {
    #[serde(rename = "set")] // Explicitly rename the "Set" variant to "set"
    Set {
        #[serde(alias = "targets")]
        target: ActionTargets,
        value: HomieValue,
        timer: Option<TimerDef>,
    },
    #[serde(rename = "map_set")] // Explicitly rename the "Set" variant to "set"
    MapSet {
        #[serde(alias = "targets")]
        target: ActionTargets,
        mapping: ValueMappingList<MapSetFrom<'static>, HomieValue>,
        timer: Option<TimerDef>,
    },
    #[serde(rename = "toggle")] // Explicitly rename the "Set" variant to "set"
    Toggle {
        #[serde(alias = "targets")]
        target: ActionTargets,
    },
    #[serde(rename = "run")] // Explicitly rename the "Set" variant to "set"
    Run {
        script: String,
        timer: Option<TimerDef>,
        /// Optional execution budget for this script. Overrides the global
        /// `lua_script_timeout` setting. Duration string, e.g. `500ms`, `10s`.
        #[serde(default, deserialize_with = "deserialize_optional_duration")]
        script_timeout: Option<Duration>,
    },
    #[serde(rename = "timer")] // Explicitly rename the "Set" variant to "set"
    Timer { timer: TimerDef },
    #[serde(rename = "cancel_timer")] // Explicitly rename the "Set" variant to "set"
    CancelTimer { timer_id: String },
    #[serde(rename = "mqtt")] // Explicitly rename the "Set" variant to "set"
    Mqtt {
        topic: String,
        value: String,
        #[serde(default)]
        qos: QoS,
        #[serde(default)]
        retain: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ActionTargets {
    Single(PropertyRef),
    Multiple(Vec<PropertyRef>),
}

impl ActionTargets {
    pub fn iter(&self) -> impl Iterator<Item = &PropertyRef> {
        match self {
            ActionTargets::Single(target) => std::slice::from_ref(target).iter(),
            ActionTargets::Multiple(targets) => targets.iter(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, PartialOrd)]
pub enum MapSetFrom<'a> {
    HomieValue(Cow<'a, HomieValue>),
    String(Cow<'a, String>),
    SolarPhase(Cow<'a, SolarPhase>),
}

impl Default for MapSetFrom<'_> {
    fn default() -> Self {
        Self::HomieValue(Cow::Owned(HomieValue::default()))
    }
}

impl ValueMatcher for MapSetFrom<'_> {
    fn as_match_str(&self) -> &str {
        match self {
            MapSetFrom::HomieValue(cow) => cow.as_match_str(),
            MapSetFrom::String(cow) => cow.as_match_str(),
            MapSetFrom::SolarPhase(cow) => cow.as_match_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_count(yaml: &str) -> usize {
        let action: RuleAction = serde_yaml_ng::from_str(yaml).expect("action should deserialize");

        match action {
            RuleAction::Set { target, .. } | RuleAction::MapSet { target, .. } | RuleAction::Toggle { target } => {
                target.iter().count()
            }
            _ => panic!("expected target action"),
        }
    }

    #[test]
    fn set_accepts_target_and_targets_fields() {
        assert_eq!(
            target_count(
                r#"
type: set
target: dev-a/switch/state
value:
  Bool: true
"#,
            ),
            1
        );
        assert_eq!(
            target_count(
                r#"
type: set
targets:
  - dev-a/switch/state
  - dev-b/switch/state
value:
  Bool: true
"#,
            ),
            2
        );
    }

    #[test]
    fn map_set_accepts_target_and_targets_fields() {
        let mapping = r#"
mapping:
  - from:
      HomieValue:
        Bool: true
    to:
      Bool: false
  - to:
      Bool: true
"#;

        assert_eq!(
            target_count(&format!(
                r#"
type: map_set
target: dev-a/switch/state
{mapping}
"#
            )),
            1
        );
        assert_eq!(
            target_count(&format!(
                r#"
type: map_set
targets:
  - dev-a/switch/state
  - dev-b/switch/state
{mapping}
"#
            )),
            2
        );
    }

    #[test]
    fn toggle_accepts_target_and_targets_fields() {
        assert_eq!(
            target_count(
                r#"
type: toggle
target: dev-a/switch/state
"#,
            ),
            1
        );
        assert_eq!(
            target_count(
                r#"
type: toggle
targets:
  - dev-a/switch/state
  - dev-b/switch/state
"#,
            ),
            2
        );
    }
}
