use super::{virtual_property::VirtualProperty, VirtualDevice};
use homie5::{DeviceRef, PropertyPointer, PropertyRef};
use std::collections::{HashMap, HashSet};

pub type PropertyPointerIndexMap = HashMap<PropertyPointer, HashSet<PropertyRef>>;
pub type PropertyIndex = HashMap<DeviceRef, PropertyPointerIndexMap>;

pub struct PropertyIndexer(PropertyIndex);

impl PropertyIndexer {
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn add_index(&mut self, prop_ref: &PropertyRef, virtual_prop_ref: PropertyRef) {
        if let Some(dev_entry) = self.0.get_mut(prop_ref.device_ref()) {
            if let Some(prop_entry) = dev_entry.get_mut(prop_ref.prop_pointer()) {
                prop_entry.insert(virtual_prop_ref);
            } else {
                let mut hs = HashSet::new();
                hs.insert(virtual_prop_ref);
                dev_entry.insert(prop_ref.prop_pointer().to_owned(), hs);
            }
        } else {
            let mut hs = HashSet::new();
            hs.insert(virtual_prop_ref);
            let mut hmprop = HashMap::new();
            hmprop.insert(prop_ref.prop_pointer().to_owned(), hs);
            self.0.insert(prop_ref.device_ref().to_owned(), hmprop);
        }
    }

    // pub fn remove_index(&mut self, virtual_prop_ref: &PropertyRef) {
    //     self.0.retain(|_, dev_entry| {
    //         dev_entry.retain(|_, prop_entry| {
    //             prop_entry.remove(virtual_prop_ref);
    //             !prop_entry.is_empty() // Keep only non-empty entries
    //         });
    //         !dev_entry.is_empty() // Keep only non-empty entries
    //     });
    // }

    pub fn lookup_index(&self, prop_ref: &PropertyRef) -> Option<&HashSet<PropertyRef>> {
        self.0
            .get(prop_ref.device_ref())
            .and_then(|dev_entry| dev_entry.get(prop_ref.prop_pointer()))
    }

    pub fn add_index_for_virtual_device(&mut self, vd: &VirtualDevice) {
        for (_, vprop) in vd.properties.iter() {
            for cp in vprop.compound_properties() {
                self.add_index(&cp.prop, vprop.prop_ref.clone());
            }
        }
    }
    pub fn remove_indexes_for_virtual_device(&mut self, vd: &DeviceRef) {
        self.0.retain(|_, dev_entry| {
            dev_entry.retain(|_, prop_entry| {
                prop_entry.retain(|vprop_ref| vprop_ref != vd);
                !prop_entry.is_empty()
            });
            !dev_entry.is_empty()
        });
    }

    pub fn update_index_for_virtual_prop(&mut self, vprop: &VirtualProperty) {
        // remove virtual_prop_ref from all indexes
        self.0.retain(|_, dev_entry| {
            dev_entry.retain(|_, prop_entry| {
                prop_entry.retain(|vprop_ref| vprop_ref != &vprop.prop_ref);
                !prop_entry.is_empty()
            });
            !dev_entry.is_empty()
        });
        for cp in vprop.compound_properties() {
            self.add_index(&cp.prop, vprop.prop_ref.clone());
        }
    }
}
