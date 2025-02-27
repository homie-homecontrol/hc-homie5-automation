use super::{deserialize_duration, deserialize_optional_duration};
use hc_homie5::ValueCondition;
use homie5::HomieValue;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct TimerDef {
    pub id: String,
    #[serde(deserialize_with = "deserialize_duration")]
    pub duration: Duration,
    #[serde(default, deserialize_with = "deserialize_optional_duration")]
    pub repeat: Option<Duration>,
    #[serde(default)]
    pub triggerbound: bool,
    pub cancelcondition: Option<ValueCondition<HomieValue>>,
}
