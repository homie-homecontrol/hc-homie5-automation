use super::{Subject, TimerDef};
use crate::solar_events::SolarPhase;
use hc_homie5::{ValueMappingList, ValueMatcher};
// use hc_homie5::{impl_value_matcher_for, AsMatchStr, ValueMappingList};
use homie5::client::QoS;
use homie5::HomieValue;
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum RuleAction {
    #[serde(rename = "set")] // Explicitly rename the "Set" variant to "set"
    Set {
        target: Subject,
        value: HomieValue,
        timer: Option<TimerDef>,
    },
    #[serde(rename = "map_set")] // Explicitly rename the "Set" variant to "set"
    MapSet {
        target: Subject,
        mapping: ValueMappingList<MapSetFrom<'static>, HomieValue>,
        timer: Option<TimerDef>,
    },
    #[serde(rename = "toggle")] // Explicitly rename the "Set" variant to "set"
    Toggle { target: Subject },
    #[serde(rename = "run")] // Explicitly rename the "Set" variant to "set"
    Run { script: String, timer: Option<TimerDef> },
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
