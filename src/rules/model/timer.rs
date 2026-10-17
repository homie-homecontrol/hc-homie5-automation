use super::{deserialize_duration, deserialize_optional_duration};
use hc_homie5::value::ValueCondition;
use homie5::HomieValue;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_rejects_unknown_field() {
        let err =
            serde_yaml_ng::from_str::<TimerDef>("id: t\nduration: 10s\ncancel_condition: { operator: matchAlways }\n")
                .unwrap_err();
        assert!(err.to_string().contains("cancel_condition"), "{err}");
    }

    #[test]
    fn timer_with_cancelcondition_parses() {
        let timer: TimerDef =
            serde_yaml_ng::from_str("id: t\nduration: 10s\ncancelcondition: { operator: matchAlways }\n").unwrap();
        assert!(timer.cancelcondition.is_some());
    }
}
