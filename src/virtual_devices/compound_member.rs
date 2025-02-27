#![allow(dead_code)]
use hc_homie5::{DeviceStore, ValueMatcher};
use homie5::{device_description::SETTABLE_DEFAULT, HomieValue, PropertyRef};
use std::collections::HashMap;

use hc_homie5::{MappingResult, ValueMappingIO, ValueMappingList};

use super::MqttWrapperChannel;

pub type PropertyCompoundMembers = HashMap<PropertyRef, PropertyCompoundMember>;
pub type MqttCompoundMembers = HashMap<String, MqttCompoundMember>;

#[derive(Debug)]
pub struct PropertyCompoundMember {
    pub(crate) prop: PropertyRef,
    pub(crate) value: Option<HomieValue>,
    pub(crate) mapping: Option<ValueMappingIO<HomieValue, HomieValue>>,
    pub(crate) settable: bool,
}

impl PropertyCompoundMember {
    pub fn new(
        prop: &PropertyRef,
        mapping: Option<ValueMappingIO<HomieValue, HomieValue>>,
        devices: &DeviceStore,
        parent_mapping: Option<&ValueMappingIO<HomieValue, HomieValue>>,
    ) -> Self {
        let value_entry = devices.get_value_entry(prop);

        let settable = devices
            .get_device(prop.device_ref())
            .and_then(|d| {
                d.description
                    .as_ref()
                    .and_then(|desc| desc.with_property(prop, |p| p.settable))
            })
            .unwrap_or(SETTABLE_DEFAULT);

        let value = map_value_list(
            value_entry.and_then(|ve| ve.value.as_ref()),
            mapping.as_ref().map(|m| &m.input),
            parent_mapping.as_ref().map(|m| &m.input),
        )
        .unwrap();
        Self {
            prop: prop.clone(),
            value,
            mapping,
            settable,
        }
    }

    pub fn update_value(
        &mut self,
        parent_mapping: Option<&ValueMappingIO<HomieValue, HomieValue>>,
        value: &HomieValue,
    ) {
        self.value = map_value_list(
            Some(value),
            self.mapping.as_ref().map(|m| &m.input),
            parent_mapping.as_ref().map(|m| &m.input),
        )
        .unwrap();
    }
}

pub fn map_value_list<FROM, TO>(
    value: Option<&FROM>,
    direct_mapping: Option<&ValueMappingList<FROM, TO>>,
    overall_mapping: Option<&ValueMappingList<FROM, TO>>,
) -> MappingResult<Option<FROM>, Option<TO>>
where
    TO: PartialEq + PartialOrd + std::fmt::Debug + Clone,
    FROM: ValueMatcher + PartialEq + PartialOrd + std::fmt::Debug + Clone,
{
    if let Some(value) = value {
        let result = if let Some(dm) = direct_mapping {
            dm.map_to(value).cloned()
        } else if let Some(om) = overall_mapping {
            om.map_to(value).cloned()
        } else {
            MappingResult::Unmapped(value.clone())
        };
        result.into_option_wrap()
    } else {
        MappingResult::Unmapped(value.cloned())
    }
}

#[derive(Debug)]
pub struct MqttCompoundMember {
    pub(crate) value: Option<HomieValue>,
    pub(crate) input: MqttWrapperChannel<String, HomieValue>,
    pub(crate) output: Option<MqttWrapperChannel<HomieValue, String>>,
}
