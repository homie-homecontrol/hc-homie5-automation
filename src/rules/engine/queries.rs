use std::collections::HashMap;

use crate::{rules::Rule, virtual_devices::VirtualDevice};
use hc_homie5::device::HomieDeviceCore;
use hc_homie5::model::Device;
use hc_homie5::store::DeviceStore;
use homie5::DeviceRef;

pub fn queries_init_materialized(rule: &mut Rule, devices: &DeviceStore, vd: &HashMap<DeviceRef, VirtualDevice>) {
    for (_, _, device) in devices.iter() {
        queries_device_updated(rule, device);
    }
    for trigger in rule.triggers.iter_mut() {
        if let crate::rules::RuleTrigger::OnSetEventTrigger {
            queries: ref mut on_set_queries,
            ..
        } = trigger
        {
            for query in on_set_queries.iter_mut() {
                for (dev_ref, device) in vd.iter() {
                    // virtual devices carry no $meta overlay
                    query.add_materialized(dev_ref.homie_domain(), dev_ref.device_id(), device.description(), None);
                }
            }
        }
    }
}

/// Re-evaluates the rule's property queries against a discovered device; call it when the
/// device's description or `$meta` overlay changes. A device without description is skipped.
pub fn queries_device_updated(rule: &mut Rule, device: &Device) {
    let Some(desc) = device.description.as_ref() else {
        return;
    };
    let device_ref = &device.ident;
    for trigger in rule.triggers.iter_mut() {
        match trigger {
            crate::rules::RuleTrigger::PropertyTriggered { ref mut queries, .. }
            | crate::rules::RuleTrigger::PropertyChanged { ref mut queries, .. } => {
                for query in queries.iter_mut() {
                    query.add_materialized(device_ref.homie_domain(), device_ref.device_id(), desc, Some(device));
                }
            }
            _ => {}
        }
    }
}
