use std::collections::HashMap;

use crate::{rules::Rule, virtual_devices::VirtualDevice};
use hc_homie5::{DeviceStore, HomieDeviceCore};
use homie5::DeviceRef;

pub fn queries_init_materialized(rule: &mut Rule, devices: &DeviceStore, vd: &HashMap<DeviceRef, VirtualDevice>) {
    for trigger in rule.triggers.iter_mut() {
        match trigger {
            crate::rules::RuleTrigger::SubjectTriggered { ref mut queries, .. } => {
                for query in queries.iter_mut() {
                    for (domain, id, device) in devices.iter() {
                        if let Some(desc) = device.description.as_ref() {
                            query.add_materialized(domain, id, desc);
                        }
                    }
                }
            }
            crate::rules::RuleTrigger::SubjectChanged { ref mut queries, .. } => {
                for query in queries.iter_mut() {
                    for (domain, id, device) in devices.iter() {
                        if let Some(desc) = device.description.as_ref() {
                            query.add_materialized(domain, id, desc);
                        }
                    }
                }
            }
            crate::rules::RuleTrigger::OnSetEventTrigger {
                queries: ref mut on_set_queries,
                ..
            } => {
                for query in on_set_queries.iter_mut() {
                    for (dev_ref, device) in vd.iter() {
                        query.add_materialized(dev_ref.homie_domain(), dev_ref.device_id(), device.description());
                    }
                }
            }

            _ => {}
        }
    }
}
// pub fn queries_remove_init_materialized(rule: &mut Rule, devices: &DeviceStore) {
//     for trigger in rule.trigger.iter_mut() {
//         match trigger {
//             crate::rules::RuleTrigger::SubjectTriggered { ref mut queries, .. } => {
//                 for query in queries.iter_mut() {
//                     for (domain, id, device) in devices.iter() {
//                         if let Some(desc) = device.description.as_ref() {
//                             query.remove_materialized(domain, id, desc);
//                         }
//                     }
//                 }
//             }
//             crate::rules::RuleTrigger::SubjectChanged { ref mut queries, .. } => {
//                 for query in queries.iter_mut() {
//                     for (domain, id, device) in devices.iter() {
//                         if let Some(desc) = device.description.as_ref() {
//                             query.remove_materialized(domain, id, desc);
//                         }
//                     }
//                 }
//             }
//
//             _ => {}
//         }
//     }
// }
