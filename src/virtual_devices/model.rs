#![allow(dead_code)]

use crate::rules::deserialize_optional_duration;
use homie5::PropertyRef;
use std::collections::BTreeMap;
use std::time::Duration;

use hc_homie5::query::QueryDefinition;
use hc_homie5::value::{ValueMappingIO, ValueMappingList, ValueMatcher};
use hc_homie5_smarthome::air_quality_node::AirQualityNodeConfig;
use hc_homie5_smarthome::alarm_node::AlarmNodeConfig;
use hc_homie5_smarthome::battery_node::BatteryNodeConfig;
use hc_homie5_smarthome::button_node::ButtonNodeConfig;
use hc_homie5_smarthome::camera_node::CameraNodeConfig;
use hc_homie5_smarthome::climate_node::ClimateNodeConfig;
use hc_homie5_smarthome::color_node::ColorNodeConfig;
use hc_homie5_smarthome::daylight_node::DaylightNodeConfig;
use hc_homie5_smarthome::garage_door_node::GarageDoorNodeConfig;
use hc_homie5_smarthome::level_node::LevelNodeConfig;
use hc_homie5_smarthome::link_node::LinkNodeConfig;
use hc_homie5_smarthome::lock_node::LockNodeConfig;
use hc_homie5_smarthome::media_info_node::MediaInfoNodeConfig;
use hc_homie5_smarthome::mediaplayer_node::MediaplayerNodeConfig;
use hc_homie5_smarthome::powermeter_node::PowermeterNodeConfig;
use hc_homie5_smarthome::scene_node::SceneNodeConfig;
use hc_homie5_smarthome::shutter_node::ShutterNodeConfig;
use hc_homie5_smarthome::switch_node::SwitchNodeConfig;
use hc_homie5_smarthome::thermostat_node::ThermostatNodeConfig;
use hc_homie5_smarthome::timer_node::TimerNodeConfig;
use hc_homie5_smarthome::valve_node::ValveNodeConfig;
use hc_homie5_smarthome::vibration_node::VibrationNodeConfig;
use hc_homie5_smarthome::volume_node::VolumeNodeConfig;
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
    /// When set, the device structure (nodes/properties/compounds) is
    /// auto-materialized from the discovered descriptions of the listed
    /// member devices. `nodes` then acts as an overlay (patch/extend/exclude).
    pub group: Option<GroupSpec>,
    #[serde(default = "serde_default_list")]
    pub nodes: Vec<VirtualNodeSpec>,
}

/// Configuration for an auto-materialized group device.
///
/// All nodes and properties of the listed member devices are unioned into
/// this virtual device. Properties with the same node/property ID across
/// members are combined into a single compound property whose members are
/// the source properties. Descriptions are merged (widest number range,
/// union of enum/color values, settable OR) and aggregate functions default
/// by datatype (bool -> or, integer/float -> avg, everything else -> equal).
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct GroupSpec {
    /// Member devices (device IDs in the configured homie domain).
    /// List order defines priority for conflict resolution (first wins).
    pub devices: Vec<HomieID>,
    /// Default aggregation debounce applied to every generated compound with
    /// a non-`equal` aggregate function. No debounce when unset.
    #[serde(deserialize_with = "deserialize_optional_duration", default)]
    pub aggregation_debounce: Option<Duration>,
    /// Entries of the form `"node-id"` or `"node-id/prop-id"` that are
    /// excluded from auto-generation.
    #[serde(default)]
    pub exclude: Vec<GroupExclude>,
    /// Debounce for structure rebuilds triggered by discovery changes of
    /// member devices. Defaults to 1s when unset.
    #[serde(deserialize_with = "deserialize_optional_duration", default)]
    pub rebuild_debounce: Option<Duration>,
}

/// A node or node/property exclusion entry of a [`GroupSpec`].
///
/// Parsed from a string: `"node-id"` excludes a whole node,
/// `"node-id/prop-id"` excludes a single property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupExclude {
    pub node: HomieID,
    pub property: Option<HomieID>,
}

impl GroupExclude {
    /// True when the whole node is excluded.
    pub fn excludes_node(&self, node_id: &HomieID) -> bool {
        self.property.is_none() && &self.node == node_id
    }

    /// True when this entry excludes the given property (either via a
    /// node-level exclusion or an exact node/property match).
    pub fn excludes_property(&self, node_id: &HomieID, prop_id: &HomieID) -> bool {
        &self.node == node_id && self.property.as_ref().is_none_or(|p| p == prop_id)
    }
}

impl std::str::FromStr for GroupExclude {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('/');
        let node = parts
            .next()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| format!("Invalid group exclude entry '{}': missing node id", s))?;
        let property = parts.next();
        if parts.next().is_some() {
            return Err(format!(
                "Invalid group exclude entry '{}': expected 'node-id' or 'node-id/prop-id'",
                s
            ));
        }
        let node = node
            .parse::<HomieID>()
            .map_err(|e| format!("Invalid group exclude entry '{}': invalid node id: {}", s, e))?;
        let property = property
            .map(|p| {
                p.parse::<HomieID>()
                    .map_err(|e| format!("Invalid group exclude entry '{}': invalid property id: {}", s, e))
            })
            .transpose()?;
        Ok(Self { node, property })
    }
}

impl<'de> Deserialize<'de> for GroupExclude {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
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
    /// Overlay-only flag for group devices: when true, the auto-generated
    /// node with this ID is removed. Ignored (with a warning) on non-group
    /// devices.
    pub exclude: Option<bool>,
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
    #[serde(rename = "air-quality")]
    AirQuality(Option<AirQualityNodeConfig>),
    Alarm(Option<AlarmNodeConfig>),
    Battery(Option<BatteryNodeConfig>),
    Button(Option<ButtonNodeConfig>),
    Camera(Option<CameraNodeConfig>),
    Climate(Option<ClimateNodeConfig>),
    Color(Option<ColorNodeConfig>),
    Daylight(Option<DaylightNodeConfig>),
    #[serde(rename = "garage-door")]
    GarageDoor(Option<GarageDoorNodeConfig>),
    Level(Option<LevelNodeConfig>),
    Link(Option<LinkNodeConfig>),
    Lock(Option<LockNodeConfig>),
    #[serde(rename = "media-info")]
    MediaInfo(Option<MediaInfoNodeConfig>),
    Mediaplayer(Option<MediaplayerNodeConfig>),
    Powermeter(Option<PowermeterNodeConfig>),
    Scene(Option<SceneNodeConfig>),
    Shutter(Option<ShutterNodeConfig>),
    Switch(Option<SwitchNodeConfig>),
    Thermostat(Option<ThermostatNodeConfig>),
    Timer(Option<TimerNodeConfig>),
    Valve(Option<ValveNodeConfig>),
    Vibration(Option<VibrationNodeConfig>),
    Volume(Option<VolumeNodeConfig>),

    // Types without configuration
    Co,
    Contact,
    Illuminance,
    Motion,
    Orientation,
    Smoke,
    Text,
    Tilt,
    WaterSensor,
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
    /// Overlay-only flag for group devices: when true, the auto-generated
    /// property with this ID is removed. Ignored (with a warning) on
    /// non-group devices.
    pub exclude: Option<bool>,
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
            property_opts: self.property_opts,
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
    pub property_opts: Option<VirtualPropertyOptions>,
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
    /// May be empty in a group-device overlay (the generated members are
    /// kept); a non-group compound without members is simply inert.
    #[serde(default)]
    pub members: Vec<MemberSpec>,
    pub mapping: Option<ValueMappingIO<HomieValue, HomieValue>>,
    /// `None` means "not specified": defaults to `equal` at evaluation time
    /// and keeps the generated aggregate when used as a group-device overlay.
    pub aggregate_function: Option<AggregateFunctionType>,
    #[serde(deserialize_with = "deserialize_optional_duration", default)]
    pub aggregation_debounce: Option<Duration>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged, deny_unknown_fields)]
pub enum MemberSpec {
    PropertyRef(PropertyRef),
    PropertyRefMember {
        property: PropertyRef,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Result<VirtualDeviceSpec, serde_yaml_ng::Error> {
        serde_yaml_ng::from_str::<VirtualDeviceSpec>(yaml)
    }

    #[test]
    fn legacy_spec_without_group_parses_unchanged() {
        let spec = parse(
            r#"
id: wz-lights
name: Lights Wohnzimmer
nodes:
  - id: switch
    from_smarthome:
      type: switch
      config:
        settable: true
    properties:
      - id: state
        compound_spec:
          members:
            - group-1/switch/state
          aggregate_function: or
          aggregation_debounce: 150ms
"#,
        )
        .expect("legacy spec must parse");
        assert!(spec.group.is_none());
        assert_eq!(spec.nodes.len(), 1);
        assert_eq!(spec.nodes[0].properties.len(), 1);
        assert!(spec.nodes[0].exclude.is_none());
        assert!(spec.nodes[0].properties[0].exclude.is_none());
    }

    #[test]
    fn group_spec_minimal() {
        let spec = parse(
            r#"
id: my-group
name: My Group
group:
  devices:
    - dev-a
    - dev-b
"#,
        )
        .expect("minimal group spec must parse");
        let group = spec.group.expect("group must be set");
        assert_eq!(
            group.devices,
            vec!["dev-a".parse::<HomieID>().unwrap(), "dev-b".parse::<HomieID>().unwrap()]
        );
        assert!(group.aggregation_debounce.is_none());
        assert!(group.rebuild_debounce.is_none());
        assert!(group.exclude.is_empty());
        // nodes are optional for group devices
        assert!(spec.nodes.is_empty());
    }

    #[test]
    fn group_spec_full() {
        let spec = parse(
            r#"
id: my-group
group:
  devices:
    - dev-a
  aggregation_debounce: 150ms
  rebuild_debounce: 2s
  exclude:
    - color
    - switch/action
nodes:
  - id: switch
    properties:
      - id: action
        format: toggle
  - id: color
    exclude: true
    properties: []
  - id: level
    properties:
      - id: action
        exclude: true
"#,
        )
        .expect("full group spec must parse");
        let group = spec.group.expect("group must be set");
        assert_eq!(group.aggregation_debounce, Some(Duration::from_millis(150)));
        assert_eq!(group.rebuild_debounce, Some(Duration::from_secs(2)));
        assert_eq!(group.exclude.len(), 2);
        assert_eq!(group.exclude[0].node, "color".parse::<HomieID>().unwrap());
        assert!(group.exclude[0].property.is_none());
        assert_eq!(group.exclude[1].node, "switch".parse::<HomieID>().unwrap());
        assert_eq!(group.exclude[1].property, Some("action".parse::<HomieID>().unwrap()));
        assert_eq!(spec.nodes[1].exclude, Some(true));
        assert_eq!(spec.nodes[2].properties[0].exclude, Some(true));
    }

    /// The "Device Groups" example from docs/virtual_devices.md must parse.
    #[test]
    fn group_spec_docs_example() {
        let spec = parse(
            r#"
id: wz-stehlampe-papier
name: Stehlampe Papier
group:
    devices:
        - dev-ec-1b-bd-ff-fe-6c-f0-5b
        - dev-ec-1b-bd-ff-fe-31-77-a3
        - dev-ec-1b-bd-ff-fe-71-39-88
    aggregation_debounce: 150ms
nodes:
    - id: switch
      properties:
          - id: action
            format: toggle
"#,
        )
        .expect("docs group example must parse");
        let group = spec.group.expect("group must be set");
        assert_eq!(group.devices.len(), 3);
        assert_eq!(group.aggregation_debounce, Some(Duration::from_millis(150)));
        assert_eq!(spec.nodes.len(), 1);
        assert_eq!(spec.nodes[0].properties[0].format.as_deref(), Some("toggle"));
    }

    #[test]
    fn group_exclude_matching() {
        let node_exclude: GroupExclude = "color".parse().unwrap();
        let prop_exclude: GroupExclude = "switch/action".parse().unwrap();

        let color = "color".parse::<HomieID>().unwrap();
        let switch = "switch".parse::<HomieID>().unwrap();
        let action = "action".parse::<HomieID>().unwrap();
        let state = "state".parse::<HomieID>().unwrap();

        assert!(node_exclude.excludes_node(&color));
        assert!(!node_exclude.excludes_node(&switch));
        // node-level exclusion also excludes all properties of the node
        assert!(node_exclude.excludes_property(&color, &action));

        assert!(!prop_exclude.excludes_node(&switch));
        assert!(prop_exclude.excludes_property(&switch, &action));
        assert!(!prop_exclude.excludes_property(&switch, &state));
        assert!(!prop_exclude.excludes_property(&color, &action));
    }

    #[test]
    fn group_exclude_invalid_forms_are_rejected() {
        assert!("".parse::<GroupExclude>().is_err());
        assert!("a/b/c".parse::<GroupExclude>().is_err());
        assert!("Invalid-Upper".parse::<GroupExclude>().is_err());
        assert!("node/Invalid-Upper".parse::<GroupExclude>().is_err());

        // through serde as well
        let res = parse(
            r#"
id: my-group
group:
  devices: [dev-a]
  exclude:
    - a/b/c
"#,
        );
        assert!(res.is_err());
    }

    #[test]
    fn group_spec_unknown_fields_are_rejected() {
        let res = parse(
            r#"
id: my-group
group:
  devices: [dev-a]
  not-a-field: true
"#,
        );
        assert!(res.is_err());
    }
}
