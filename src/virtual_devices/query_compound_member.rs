use super::compound_member::{PropertyCompoundMember, PropertyCompoundMembers};
use hc_homie5::query::{MetaAnnotations, QueryDefinition};
use hc_homie5::store::DeviceStore;
use hc_homie5::value::ValueMappingIO;
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
                let props = query.match_query(domain, id, desc, Some(device));
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
    /// Re-evaluates the query for a device after its description or `$meta` overlay changed; the
    /// overlay is read from `devices`.
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
        let meta = devices
            .get_device(device_ref)
            .map(|device| device as &dyn MetaAnnotations);
        let props = self
            .query
            .match_query(device_ref.homie_domain(), device_ref.device_id(), desc, meta);
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

    pub fn values(&self) -> Values<'_, PropertyRef, PropertyCompoundMember> {
        self.prop_compound_members.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use homie5::{HomieDeviceStatus, HomieDomain, HomieID};
    use std::str::FromStr;

    fn id(s: &str) -> HomieID {
        s.to_owned().try_into().unwrap()
    }

    fn add_contact_device(devices: &mut DeviceStore, device_id: &str) -> DeviceRef {
        let device_ref = DeviceRef::new(HomieDomain::Default, id(device_id));
        let description: HomieDeviceDescription = serde_json::from_value(serde_json::json!({
            "homie": "5.0",
            "version": 1,
            "nodes": { "contact": { "properties": { "state": { "datatype": "boolean" } } } }
        }))
        .unwrap();
        devices.add(&device_ref, HomieDeviceStatus::Ready);
        devices.store_description(&device_ref, description);
        device_ref
    }

    fn set_tags(devices: &mut DeviceStore, device_ref: &DeviceRef, tags: &[&str]) {
        let overlay = serde_json::from_value(serde_json::json!({
            "schema": 1,
            "device": { "annotations": { "tags": tags } }
        }))
        .unwrap();
        devices
            .get_device_mut(device_ref)
            .unwrap()
            .set_meta_overlay(id("manual"), overlay);
    }

    fn query() -> QueryDefinition {
        serde_yaml_ng::from_str("meta: { annotations: { tags: { any: window } } }").unwrap()
    }

    fn members(qcm: &QueryCompoundMember) -> Vec<PropertyRef> {
        qcm.prop_compound_members.keys().cloned().collect()
    }

    #[test]
    fn meta_query_member_follows_annotations() {
        let mut devices = DeviceStore::new();
        let window = add_contact_device(&mut devices, "window");
        let door = add_contact_device(&mut devices, "door");
        set_tags(&mut devices, &window, &["sensor", "window"]);
        let window_state = PropertyRef::from_str("homie/window/contact/state").unwrap();
        let door_state = PropertyRef::from_str("homie/door/contact/state").unwrap();

        let mut qcm = QueryCompoundMember::new(query(), None, &devices, None);
        assert_eq!(members(&qcm), vec![window_state.clone()]);

        set_tags(&mut devices, &door, &["window"]);
        let desc = devices.get_device(&door).unwrap().description.clone().unwrap();
        assert!(qcm.update_compound_members(&door, &desc, &devices, None));
        assert!(members(&qcm).contains(&door_state));

        devices
            .get_device_mut(&window)
            .unwrap()
            .remove_meta_overlay(&id("manual"));
        let desc = devices.get_device(&window).unwrap().description.clone().unwrap();
        assert!(qcm.update_compound_members(&window, &desc, &devices, None));
        assert_eq!(members(&qcm), vec![door_state]);
    }
}
