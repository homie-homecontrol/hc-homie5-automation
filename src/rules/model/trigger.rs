use std::borrow::Cow;
use std::time::Duration;

use color_eyre::eyre::{self, eyre};
use hc_homie5::DiscoveryAction;
use homie5::client::QoS;
use homie5::{Homie5Message, HomieValue, PropertyRef};
use serde::Deserialize;

use crate::cron_manager::CronEvent;
use crate::mqtt_client::MqttPublishEvent;
use crate::solar_events::{SolarEvent, SolarPhase};
use crate::timer_manager::TimerEvent;
use hc_homie5::MaterializedQuery;
use hc_homie5::ValueCondition;

use super::{deserialize_duration, Subject, WhileConditionSet};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum RuleTrigger {
    SubjectTriggered {
        #[serde(default)]
        subjects: Vec<Subject>,
        #[serde(default)]
        queries: Vec<MaterializedQuery>,
        trigger_value: ValueCondition<HomieValue>,
        r#while: Option<WhileConditionSet>,
    },
    SubjectChanged {
        #[serde(default)]
        subjects: Vec<Subject>,
        #[serde(default)]
        queries: Vec<MaterializedQuery>,
        changed: ChangedTrigger,
        r#while: Option<WhileConditionSet>,
    },
    TimerTrigger {
        timer_id: String,
        r#while: Option<WhileConditionSet>,
    },
    CronTrigger {
        schedule: String,
        r#while: Option<WhileConditionSet>,
    },
    MqttTrigger {
        topic: String,
        #[serde(default)]
        skip_retained: bool,
        #[serde(default)]
        skip_duplicated: bool,
        #[serde(default)]
        check_qos: bool,
        #[serde(default)]
        qos: QoS,
        trigger_value: ValueCondition<String>,
        r#while: Option<WhileConditionSet>,
    },
    SolarEventTriggerAfter {
        sun_phase: SolarPhase,
        #[serde(deserialize_with = "deserialize_duration")]
        min_after: Duration,
        r#while: Option<WhileConditionSet>,
    },
    SolarEventTriggerBefore {
        sun_phase: SolarPhase,
        #[serde(deserialize_with = "deserialize_duration")]
        min_before: Duration,
        r#while: Option<WhileConditionSet>,
    },
    SolarEventTrigger {
        sun_phase: SolarPhase,
        r#while: Option<WhileConditionSet>,
    },
    OnSetEventTrigger {
        #[serde(default)]
        subjects: Vec<Subject>,
        #[serde(default)]
        queries: Vec<MaterializedQuery>,
        set_value: ValueCondition<String>,
        r#while: Option<WhileConditionSet>,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum RuleTriggerEvent<'a> {
    //DiscoverAction(Cow<'a, DiscoveryAction>),
    PropertyChanged {
        prop: Cow<'a, PropertyRef>,
        from: Cow<'a, Option<HomieValue>>,
        to: Cow<'a, HomieValue>,
    },
    PropertyTriggered {
        prop: Cow<'a, PropertyRef>,
        value: Cow<'a, HomieValue>,
    },
    Timer(Cow<'a, TimerEvent>),
    Cron(Cow<'a, CronEvent>),
    Mqtt(Cow<'a, MqttPublishEvent>),
    OnSet {
        prop: Cow<'a, PropertyRef>,
        value: Cow<'a, String>,
    },
    Solar(Cow<'a, SolarEvent>),
    // SystemEvent,
}

impl RuleTriggerEvent<'_> {
    pub fn property_ref(&self) -> Option<&PropertyRef> {
        if let Self::PropertyChanged { prop, .. } | Self::PropertyTriggered { prop, .. } | Self::OnSet { prop, .. } =
            self
        {
            Some(prop)
        } else {
            None
        }
    }

    pub fn value(&self) -> Option<&HomieValue> {
        if let Self::PropertyChanged { to, .. } | Self::PropertyTriggered { value: to, .. } = self {
            Some(to)
        } else {
            None
        }
    }

    pub fn on_set_value(&self) -> Option<&str> {
        if let Self::OnSet { value, .. } = self {
            Some(value)
        } else {
            None
        }
    }

    pub fn from(&self) -> Option<&HomieValue> {
        if let Self::PropertyChanged { from, .. } = self {
            from.as_ref().as_ref()
        } else {
            None
        }
    }

    pub fn timer_id(&self) -> Option<&str> {
        if let RuleTriggerEvent::Timer(event) = self {
            Some(event.id.as_str())
        } else {
            None
        }
    }

    pub fn trigger_type(&self) -> &str {
        match self {
            RuleTriggerEvent::PropertyChanged { .. } => "changed",
            RuleTriggerEvent::PropertyTriggered { .. } => "trigered",
            RuleTriggerEvent::Timer(_) => "timer",
            RuleTriggerEvent::Cron(_) => "cron",
            RuleTriggerEvent::Solar(_) => "solar",
            RuleTriggerEvent::OnSet { .. } => "onset",
            RuleTriggerEvent::Mqtt(_) => "mqtt",
        }
    }

    pub fn to_owned(&self) -> RuleTriggerEvent<'static> {
        match self {
            RuleTriggerEvent::PropertyChanged { prop, from, to } => RuleTriggerEvent::PropertyChanged {
                prop: Cow::Owned(prop.clone().into_owned()),
                from: Cow::Owned(from.clone().into_owned()),
                to: Cow::Owned(to.clone().into_owned()),
            },
            RuleTriggerEvent::PropertyTriggered { prop, value } => RuleTriggerEvent::PropertyTriggered {
                prop: Cow::Owned(prop.clone().into_owned()),
                value: Cow::Owned(value.clone().into_owned()),
            },
            RuleTriggerEvent::Timer(data) => RuleTriggerEvent::Timer(Cow::Owned(data.clone().into_owned())),
            RuleTriggerEvent::Cron(data) => RuleTriggerEvent::Cron(Cow::Owned(data.clone().into_owned())),
            RuleTriggerEvent::Mqtt(data) => RuleTriggerEvent::Mqtt(Cow::Owned(data.clone().into_owned())),
            RuleTriggerEvent::Solar(data) => RuleTriggerEvent::Solar(Cow::Owned(data.clone().into_owned())),
            RuleTriggerEvent::OnSet { prop, value } => RuleTriggerEvent::OnSet {
                prop: Cow::Owned(prop.clone().into_owned()),
                value: Cow::Owned(value.clone().into_owned()),
            },
        }
    }
}

impl TryFrom<DiscoveryAction> for RuleTriggerEvent<'_> {
    type Error = eyre::Error;
    fn try_from(action: DiscoveryAction) -> Result<Self, Self::Error> {
        match action {
            DiscoveryAction::DevicePropertyValueChanged { prop, from, to } => Ok(RuleTriggerEvent::PropertyChanged {
                prop: Cow::Owned(prop),
                from: Cow::Owned(from),
                to: Cow::Owned(to),
            }),
            DiscoveryAction::DevicePropertyValueTriggered { prop, value } => Ok(RuleTriggerEvent::PropertyTriggered {
                prop: Cow::Owned(prop),
                value: Cow::Owned(value),
            }),
            _ => Err(eyre!("Cannot convert this variant of DiscoverAction to RuleTriggerEvent")),
        }
    }
}

impl<'a> TryFrom<&'a DiscoveryAction> for RuleTriggerEvent<'a> {
    type Error = eyre::Error;
    fn try_from(action: &'a DiscoveryAction) -> Result<Self, Self::Error> {
        match action {
            DiscoveryAction::DevicePropertyValueChanged { prop, from, to } => Ok(RuleTriggerEvent::PropertyChanged {
                prop: Cow::Borrowed(prop),
                from: Cow::Borrowed(from),
                to: Cow::Borrowed(to),
            }),
            DiscoveryAction::DevicePropertyValueTriggered { prop, value } => Ok(RuleTriggerEvent::PropertyTriggered {
                prop: Cow::Borrowed(prop),
                value: Cow::Borrowed(value),
            }),
            _ => Err(eyre!("Cannot convert this variant of DiscoverAction to RuleTriggerEvent")),
        }
    }
}

impl<'a> TryFrom<&'a Homie5Message> for RuleTriggerEvent<'a> {
    type Error = eyre::Error;

    fn try_from(value: &'a Homie5Message) -> Result<Self, Self::Error> {
        match value {
            Homie5Message::PropertySet { property, set_value } => Ok(RuleTriggerEvent::OnSet {
                prop: Cow::Borrowed(property),
                value: Cow::Borrowed(set_value),
            }),
            _ => Err(eyre!("Cannot convert this variant of Homie5Message to RuleTriggerEvent")),
        }
    }
}

impl From<TimerEvent> for RuleTriggerEvent<'_> {
    fn from(event: TimerEvent) -> Self {
        RuleTriggerEvent::Timer(Cow::Owned(event))
    }
}

impl<'a> From<&'a TimerEvent> for RuleTriggerEvent<'a> {
    fn from(event: &'a TimerEvent) -> Self {
        RuleTriggerEvent::Timer(Cow::Borrowed(event))
    }
}

impl From<CronEvent> for RuleTriggerEvent<'_> {
    fn from(event: CronEvent) -> Self {
        RuleTriggerEvent::Cron(Cow::Owned(event))
    }
}

impl<'a> From<&'a CronEvent> for RuleTriggerEvent<'a> {
    fn from(event: &'a CronEvent) -> Self {
        RuleTriggerEvent::Cron(Cow::Borrowed(event))
    }
}

impl From<MqttPublishEvent> for RuleTriggerEvent<'_> {
    fn from(event: MqttPublishEvent) -> Self {
        RuleTriggerEvent::Mqtt(Cow::Owned(event))
    }
}

impl<'a> From<&'a MqttPublishEvent> for RuleTriggerEvent<'a> {
    fn from(event: &'a MqttPublishEvent) -> Self {
        RuleTriggerEvent::Mqtt(Cow::Borrowed(event))
    }
}

impl From<SolarEvent> for RuleTriggerEvent<'_> {
    fn from(event: SolarEvent) -> Self {
        RuleTriggerEvent::Solar(Cow::Owned(event))
    }
}

impl<'a> From<&'a SolarEvent> for RuleTriggerEvent<'a> {
    fn from(event: &'a SolarEvent) -> Self {
        RuleTriggerEvent::Solar(Cow::Borrowed(event))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangedTrigger {
    #[serde(default)]
    pub from: Option<ValueCondition<HomieValue>>,
    #[serde(default)]
    pub to: Option<ValueCondition<HomieValue>>,
}
