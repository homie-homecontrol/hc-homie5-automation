//! Deleting removed virtual devices from the broker.
//!
//! A virtual device removed from the config is first only disconnected; the deletion of its
//! retained topics is decided a little later, once the config change is applied. If a device with
//! the same ID exists again by then (a file edit, a template re-expansion, a group rebuild), its
//! retained values stay so `read_from_mqtt` can restore them, and only topics the new device no
//! longer has are cleared. Otherwise the device is deleted per Homie 5: alerts, `$state`,
//! `$description` and every retained property value and `$target` are cleared.

use std::collections::{BTreeSet, HashMap, HashSet};

use homie5::{
    client::Publish, device_description::HomieDeviceDescription, Homie5DeviceProtocol, Homie5ProtocolError, HomieID,
};

/// A virtual device that left the device map, waiting for the deletion decision.
#[derive(Debug, Clone)]
pub(crate) struct RemovedDevice {
    proto: Homie5DeviceProtocol,
    /// Descriptions published under this ID since the removal was staged, oldest first.
    descriptions: Vec<HomieDeviceDescription>,
    /// Alerts the removed device(s) had published.
    alerts: BTreeSet<HomieID>,
}

impl RemovedDevice {
    pub(crate) fn new(
        proto: Homie5DeviceProtocol,
        description: HomieDeviceDescription,
        alerts: impl IntoIterator<Item = HomieID>,
    ) -> Self {
        Self {
            proto,
            descriptions: vec![description],
            alerts: alerts.into_iter().collect(),
        }
    }

    /// Adds a later removal of the same ID (changed again before the decision).
    pub(crate) fn merge(&mut self, later: RemovedDevice) {
        self.descriptions.extend(later.descriptions);
        self.alerts.extend(later.alerts);
    }

    /// Publishes that clean up after the removal. `current` is the device with the same ID after
    /// the change (description and active alerts), `None` when the ID is gone.
    pub(crate) fn cleanup_publishes(
        &self,
        current: Option<(&HomieDeviceDescription, &HashMap<HomieID, String>)>,
    ) -> Result<Vec<Publish>, Homie5ProtocolError> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut push = |publish: Publish, out: &mut Vec<Publish>| {
            if seen.insert(publish.topic.clone()) {
                out.push(publish);
            }
        };
        match current {
            None => {
                for alert in &self.alerts {
                    push(self.proto.publish_clear_alert(alert), &mut out);
                }
                for description in &self.descriptions {
                    for publish in self.proto.remove_device(description)? {
                        push(publish, &mut out);
                    }
                }
            }
            Some((description, alerts)) => {
                for alert in self.alerts.iter().filter(|a| !alerts.contains_key(*a)) {
                    push(self.proto.publish_clear_alert(alert), &mut out);
                }
                let kept: HashSet<(&HomieID, &HomieID)> = description
                    .iter()
                    .filter(|(_, _, _, prop)| prop.retained)
                    .map(|(node_id, _, prop_id, _)| (node_id, prop_id))
                    .collect();
                for old in &self.descriptions {
                    for (node_id, _, prop_id, _) in old.iter().filter(|(_, _, _, prop)| prop.retained) {
                        if !kept.contains(&(node_id, prop_id)) {
                            push(self.proto.publish_value(node_id, prop_id, "", true), &mut out);
                            push(self.proto.publish_target(node_id, prop_id, "", true), &mut out);
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use homie5::{device_description::DeviceDescriptionBuilder, HomieDomain};

    use super::*;

    fn id(s: &str) -> HomieID {
        s.to_owned().try_into().unwrap()
    }

    fn proto() -> Homie5DeviceProtocol {
        let (root, _) = Homie5DeviceProtocol::new(id("ctrl"), HomieDomain::Default);
        root.clone_for_child(id("vdev"))
    }

    /// `props`: (node, property, retained)
    fn description(props: &[(&str, &str, bool)]) -> HomieDeviceDescription {
        let nodes: serde_json::Map<String, serde_json::Value> =
            props
                .iter()
                .fold(serde_json::Map::new(), |mut nodes, (node, prop, retained)| {
                    let node = nodes
                        .entry(node.to_string())
                        .or_insert_with(|| serde_json::json!({ "properties": {} }));
                    node["properties"][prop] = serde_json::json!({ "datatype": "boolean", "retained": retained });
                    nodes
                });
        let mut description: HomieDeviceDescription = serde_json::from_value(serde_json::json!({
            "homie": "5.0", "version": 1, "nodes": nodes
        }))
        .unwrap();
        description.root = DeviceDescriptionBuilder::new().root(id("ctrl")).build().root;
        description.parent = Some(id("ctrl"));
        description
    }

    fn topics(publishes: &[Publish]) -> Vec<&str> {
        publishes.iter().map(|p| p.topic.as_str()).collect()
    }

    #[test]
    fn removed_device_is_deleted() {
        let removed = RemovedDevice::new(
            proto(),
            description(&[("switch", "state", true), ("switch", "action", false)]),
            [id("battery")],
        );

        let publishes = removed.cleanup_publishes(None).unwrap();

        assert_eq!(
            topics(&publishes),
            [
                "homie/5/vdev/$alert/battery",
                "homie/5/vdev/$state",
                "homie/5/vdev/$description",
                "homie/5/vdev/switch/state",
                "homie/5/vdev/switch/state/$target",
            ]
        );
        assert!(publishes.iter().all(|p| p.retain && p.payload.is_empty()));
    }

    #[test]
    fn readded_device_keeps_its_values() {
        let desc = description(&[("switch", "state", true)]);
        let removed = RemovedDevice::new(proto(), desc.clone(), []);

        let publishes = removed.cleanup_publishes(Some((&desc, &HashMap::new()))).unwrap();

        assert!(publishes.is_empty(), "{:?}", topics(&publishes));
    }

    #[test]
    fn readded_device_clears_only_what_it_no_longer_has() {
        let mut removed = RemovedDevice::new(
            proto(),
            description(&[("switch", "state", true), ("level", "value", true)]),
            [id("old"), id("kept")],
        );
        removed.merge(RemovedDevice::new(
            proto(),
            description(&[("switch", "state", true), ("color", "rgb", true)]),
            [],
        ));
        let current = description(&[("switch", "state", true)]);
        let alerts = HashMap::from([(id("kept"), "still active".to_owned())]);

        let publishes = removed.cleanup_publishes(Some((&current, &alerts))).unwrap();

        assert_eq!(
            topics(&publishes),
            [
                "homie/5/vdev/$alert/old",
                "homie/5/vdev/level/value",
                "homie/5/vdev/level/value/$target",
                "homie/5/vdev/color/rgb",
                "homie/5/vdev/color/rgb/$target",
            ]
        );
    }
}
