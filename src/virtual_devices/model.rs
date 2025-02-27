#![allow(dead_code)]

use crate::rules::{deserialize_optional_duration, Subject};
use std::collections::BTreeMap;
use std::time::Duration;

use hc_homie5::{QueryDefinition, ValueMappingIO, ValueMappingList, ValueMatcher};
use hc_homie5_smarthome::button_node::ButtonNodeConfig;
use hc_homie5_smarthome::colorlight_node::ColorlightNodeConfig;
use hc_homie5_smarthome::dimmer_node::DimmerNodeConfig;
use hc_homie5_smarthome::light_scene_node::LightSceneNodeConfig;
use hc_homie5_smarthome::maintenance_node::MaintenanceNodeConfig;
use hc_homie5_smarthome::motion_node::MotionNodeConfig;
use hc_homie5_smarthome::shutter_node::ShutterNodeConfig;
use hc_homie5_smarthome::switch_node::SwitchNodeConfig;
use hc_homie5_smarthome::thermostat_node::ThermostatNodeConfig;
use hc_homie5_smarthome::vibration_node::VibrationNodeConfig;
use hc_homie5_smarthome::weather_node::WeatherNodeConfig;
use homie5::client::QoS;
use homie5::device_description::{
    serde_default_list, serde_default_retained, serde_default_settable, HomieNodeDescription, HomiePropertyDescription,
    HomiePropertyFormat,
};
use homie5::{HomieDataType, HomieID, HomieValue};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct VirtualDeviceSpec {
    pub id: HomieID,
    pub name: Option<String>,
    pub version: Option<i64>,
    #[serde(default = "serde_default_list")]
    pub children: Vec<HomieID>,
    pub parent: Option<HomieID>,
    #[serde(default = "serde_default_list")]
    pub extensions: Vec<String>,
    pub nodes: Vec<VirtualNodeSpec>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct VirtualNodeSpec {
    pub id: HomieID,
    pub name: Option<String>,
    pub r#type: Option<String>,
    pub from_smarthome: Option<SmarthomeSpec>,
    pub pass_through: Option<bool>,
    pub property_opts: Option<VirtualPropertyOptions>,
    #[serde(default = "serde_default_list")]
    pub properties: Vec<VirtualPropertySpec>,
}

impl VirtualNodeSpec {
    pub fn split_into(self) -> (HomieID, VirtualNodeDescription, Vec<VirtualPropertySpec>, VirtualNodeConfig) {
        // Create VirtualNodeDescription with descriptive fields
        let description = VirtualNodeDescription {
            name: self.name,
            r#type: self.r#type,
        };

        // Create VirtualNodeConfig with configuration-specific fields
        let config = VirtualNodeConfig {
            from_smarthome: self.from_smarthome,
            pass_through: self.pass_through,
            property_opts: self.property_opts,
        };

        (self.id, description, self.properties, config)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct VirtualNodeDescription {
    pub name: Option<String>,
    pub r#type: Option<String>,
}

impl VirtualNodeDescription {
    pub fn patch(&self, desc: &mut HomieNodeDescription) {
        if let Some(name) = &self.name {
            desc.name = Some(name.clone());
        }
        if let Some(r#type) = &self.r#type {
            desc.r#type = Some(r#type.clone());
        }
    }
}
impl From<VirtualNodeDescription> for HomieNodeDescription {
    fn from(value: VirtualNodeDescription) -> Self {
        Self {
            name: value.name,
            r#type: value.r#type,
            properties: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Deserialize, Clone)]
pub struct VirtualNodeConfig {
    pub from_smarthome: Option<SmarthomeSpec>,
    pub pass_through: Option<bool>,
    pub property_opts: Option<VirtualPropertyOptions>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config", rename_all = "lowercase")] // Use "type" to determine the variant, "config" for optional data
pub enum SmarthomeSpec {
    // Types with optional configuration
    Button(Option<ButtonNodeConfig>),
    ColorLight(Option<ColorlightNodeConfig>),
    Dimmer(Option<DimmerNodeConfig>),
    LightScene(Option<LightSceneNodeConfig>),
    Maintenance(Option<MaintenanceNodeConfig>),
    Motion(Option<MotionNodeConfig>),
    Shutter(Option<ShutterNodeConfig>),
    Switch(Option<SwitchNodeConfig>),
    Thermostat(Option<ThermostatNodeConfig>),
    Vibration(Option<VibrationNodeConfig>),
    Weather(Option<WeatherNodeConfig>),

    // Types without configuration
    Contact,
    Numeric,
    Orientation,
    WaterSensor,
    Tilt,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct VirtualPropertySpec {
    pub id: HomieID,
    pub name: Option<String>,
    pub datatype: Option<HomieDataType>,
    pub format: Option<String>,
    pub settable: Option<bool>,
    pub retained: Option<bool>,
    pub unit: Option<String>,
    pub pass_through: Option<bool>,
    pub property_opts: Option<VirtualPropertyOptions>,
    pub compound_spec: Option<CompoundSpec>,
}

impl VirtualPropertySpec {
    pub fn split_into(self) -> (HomieID, VirtualPropertyDescription, VirtualPropertyConfig) {
        // Create VirtualPropertyDescription from shared fields
        let description = VirtualPropertyDescription {
            name: self.name,
            datatype: self.datatype,
            format: self.format,
            settable: self.settable,
            retained: self.retained,
            unit: self.unit,
        };

        // Create VirtualPropertyConfig from config-specific fields
        let config = VirtualPropertyConfig {
            pass_through: self.pass_through,
            propert_opts: self.property_opts,
            compound_spec: self.compound_spec,
        };

        (self.id, description, config)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct VirtualPropertyDescription {
    pub name: Option<String>,
    pub datatype: Option<HomieDataType>,
    pub format: Option<String>,
    pub settable: Option<bool>,
    pub retained: Option<bool>,
    pub unit: Option<String>,
}

impl VirtualPropertyDescription {
    pub fn patch(&self, desc: &mut HomiePropertyDescription) {
        if let Some(name) = &self.name {
            desc.name = Some(name.clone());
        }
        if let Some(datatype) = self.datatype {
            desc.datatype = datatype;
        }
        if let Some(format) = &self.format {
            // TODO: Add a check if the format fits the datatype and create a mitigation handling
            desc.format = HomiePropertyFormat::parse(format, &desc.datatype).unwrap_or(HomiePropertyFormat::Empty);
        }
        if let Some(settable) = self.settable {
            desc.settable = settable;
        }
        if let Some(retained) = self.retained {
            desc.retained = retained;
        }
        if let Some(unit) = &self.unit {
            desc.unit = Some(unit.clone());
        }
    }
}

impl From<VirtualPropertyDescription> for HomiePropertyDescription {
    fn from(value: VirtualPropertyDescription) -> Self {
        let datatype = value.datatype.unwrap_or_default();
        Self {
            name: value.name,
            unit: value.unit,
            format: HomiePropertyFormat::parse(&value.format.unwrap_or_default(), &datatype)
                .unwrap_or(HomiePropertyFormat::Empty),
            datatype: value.datatype.unwrap_or_default(),
            settable: value.settable.unwrap_or(serde_default_settable()),
            retained: value.retained.unwrap_or(serde_default_retained()),
        }
    }
}
#[derive(Debug, Default, Deserialize, Clone)]
pub struct VirtualPropertyConfig {
    pub pass_through: Option<bool>,
    pub propert_opts: Option<VirtualPropertyOptions>,
    pub compound_spec: Option<CompoundSpec>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct VirtualPropertyOptions {
    #[serde(default)]
    pub read_from_mqtt: bool,
    #[serde(deserialize_with = "deserialize_optional_duration")]
    pub read_timeout: Option<Duration>,
}

// ================ compound spec
//
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct CompoundSpec {
    pub members: Vec<MemberSpec>,
    pub mapping: Option<ValueMappingIO<HomieValue, HomieValue>>,
    #[serde(default)]
    pub aggregate_function: AggregateFunctionType,
    #[serde(deserialize_with = "deserialize_optional_duration", default)]
    pub aggregation_debounce: Option<Duration>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged, deny_unknown_fields)]
pub enum MemberSpec {
    Subject(Subject),
    SubjectMember {
        subject: Subject,
        mapping: ValueMappingIO<HomieValue, HomieValue>,
    },
    QueryMember {
        query: Box<QueryDefinition>,
        mapping: Option<ValueMappingIO<HomieValue, HomieValue>>,
    },
    MqttMember {
        mqtt_input: MqttWrapperChannel<String, HomieValue>,
        mqtt_output: Option<MqttWrapperChannel<HomieValue, String>>,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct MqttWrapperChannel<FROM, TO>
where
    FROM: ValueMatcher + PartialEq + PartialOrd + Default + std::fmt::Debug,
    TO: PartialEq + PartialOrd + Default + std::fmt::Debug,
{
    pub topic: String,
    pub mapping: ValueMappingList<FROM, TO>,
    #[serde(default)]
    pub retained: bool,
    #[serde(default)]
    pub qos: QoS,
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum AggregateFunctionType {
    #[default]
    Equal,
    Or,
    And,
    Nor,
    Nand,
    Avg,
    AvgCeil,
    Max,
    Min,
}
