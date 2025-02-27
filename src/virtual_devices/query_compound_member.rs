use super::compound_member::{PropertyCompoundMember, PropertyCompoundMembers};
use hc_homie5::DeviceStore;
use hc_homie5::{QueryDefinition, ValueMappingIO};
use homie5::{device_description::HomieDeviceDescription, DeviceRef, HomieValue, PropertyRef};
use std::collections::{hash_map::Values, HashMap};

#[derive(Debug)]
pub struct QueryCompoundMember {
    pub(crate) query: QueryDefinition,
    pub(crate) mapping: Option<ValueMappingIO<HomieValue, HomieValue>>,
    pub(crate) prop_compound_members: PropertyCompoundMembers,
}

impl QueryCompoundMember {
    pub fn new(
        query: QueryDefinition,
        mapping: Option<ValueMappingIO<HomieValue, HomieValue>>,
        devices: &DeviceStore,
        parent_mapping: Option<&ValueMappingIO<HomieValue, HomieValue>>,
    ) -> Self {
        let mut prop_compound_members: PropertyCompoundMembers = HashMap::new();
        for (domain, id, device) in devices.iter() {
            if let Some(desc) = device.description.as_ref() {
                let props = query.match_query(domain, id, desc);
                for prop in props.into_iter() {
                    let m = PropertyCompoundMember::new(&prop, mapping.clone(), devices, parent_mapping);
                    prop_compound_members.insert(prop, m);
                }
            }
        }
        Self {
            query,
            mapping,
            prop_compound_members,
        }
    }
    pub fn update_compound_members(
        &mut self,
        device_ref: &DeviceRef,
        desc: &HomieDeviceDescription,
        devices: &DeviceStore,
        parent_mapping: Option<&ValueMappingIO<HomieValue, HomieValue>>,
    ) -> bool {
        let mut changed = false;
        self.prop_compound_members.retain(|k, _| {
            if k.device_ref() == device_ref {
                changed = true;
                return false;
            }
            true
        });
        let props = self
            .query
            .match_query(device_ref.homie_domain(), device_ref.device_id(), desc);
        for prop in props.into_iter() {
            let m = PropertyCompoundMember::new(&prop, self.mapping.clone(), devices, parent_mapping);
            changed = true;
            self.prop_compound_members.insert(prop, m);
        }

        changed
    }
    pub fn update_compound_members_removed(&mut self, device_ref: &DeviceRef) -> bool {
        let mut changed = false;
        self.prop_compound_members.retain(|k, _| {
            if k.device_ref() == device_ref {
                changed = true;
                return false;
            }
            true
        });

        changed
    }

    pub fn update_member_value_prop(
        &mut self,
        prop: &PropertyRef,
        value: &HomieValue,
        parent_mapping: Option<&ValueMappingIO<HomieValue, HomieValue>>,
    ) -> bool {
        if let Some(pcm) = self.prop_compound_members.get_mut(prop) {
            pcm.update_value(parent_mapping, value);
            return true;
        }
        false
    }

    pub fn values(&self) -> Values<PropertyRef, PropertyCompoundMember> {
        self.prop_compound_members.values()
    }
}
