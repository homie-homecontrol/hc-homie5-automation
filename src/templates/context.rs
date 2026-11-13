use hc_homie5::{model::Device, query::QueryDefinition, store::DeviceStore, template::ExpandContext};
use homie5::extensions::meta::MetaValue;
use serde_yaml_ng::{Mapping, Value};

/// Expansion context over the discovered devices: `for_each` queries yield device snapshots.
pub struct DeviceContext<'a> {
    devices: &'a DeviceStore,
}

impl<'a> DeviceContext<'a> {
    pub fn new(devices: &'a DeviceStore) -> Self {
        Self { devices }
    }
}

impl ExpandContext for DeviceContext<'_> {
    /// Every device with a description and at least one property matching the query (device,
    /// node, property and `$meta` conditions).
    fn query(&self, query: &QueryDefinition) -> Result<Vec<Value>, String> {
        let mut items = Vec::new();
        for (domain, id, device) in self.devices.iter() {
            let Some(description) = device.description.as_ref() else {
                continue;
            };
            let matched = query.match_query(domain, id, description, Some(device));
            if !matched.is_empty() {
                let matched = matched.iter().map(|p| Value::String(p.to_string())).collect();
                items.push(device_snapshot(device, matched));
            }
        }
        Ok(items)
    }
}

fn entry(m: &mut Mapping, key: &str, value: impl Into<Value>) {
    m.insert(key.into(), value.into());
}

fn opt(value: Option<&String>) -> Value {
    value.map_or(Value::Null, |s| Value::String(s.clone()))
}

/// First non-empty values (`meta`) and full lists (`annotations`) of annotation entries.
fn annotation_maps<'a>(entries: impl IntoIterator<Item = (&'a String, &'a MetaValue)>) -> (Mapping, Mapping) {
    let mut first = Mapping::new();
    let mut all = Mapping::new();
    for (key, value) in entries {
        let values: Vec<String> = match value {
            MetaValue::Text(text) => vec![text.clone()],
            MetaValue::List(list) => list.clone(),
        };
        if let Some(value) = values.iter().find(|v| !v.trim().is_empty()) {
            entry(&mut first, key, value.clone());
        }
        entry(&mut all, key, values.into_iter().map(Value::String).collect::<Vec<_>>());
    }
    (first, all)
}

/// What expressions see of a `for_each` element: identity, names, `$meta` and descriptions
/// (never property values), plus the properties that matched the query.
///
/// ```yaml
/// id: dev-1
/// domain: homie
/// ref: homie/dev-1
/// name: Thermostat 1          # Homie $name or null
/// label: Büro                 # meta friendly_name, else $name, else id
/// meta: { room: Büro }        # first non-empty value per device annotation
/// annotations: { room: [Büro] }
/// matched: [homie/dev-1/thermostat/set-temperature]
/// nodes:
///   thermostat:
///     name: Thermostat
///     type: hc-smarthome/v2/cap/thermostat
///     meta: {}
///     properties:
///       set-temperature: { name, datatype, format, unit, settable, retained }
/// ```
pub fn device_snapshot(device: &Device, matched: Vec<Value>) -> Value {
    let mut m = Mapping::new();
    entry(&mut m, "id", device.ident.device_id().to_string());
    entry(&mut m, "domain", device.ident.homie_domain().to_string());
    entry(&mut m, "ref", device.ident.to_string());

    let homie_name = device
        .description
        .as_ref()
        .and_then(|d| d.name.as_ref())
        .filter(|name| !name.trim().is_empty());
    entry(&mut m, "name", opt(homie_name));

    let overlay = device.merged_meta_overlay().device.as_ref();
    let (meta, annotations) = annotation_maps(overlay.and_then(|d| d.annotations.as_ref()).into_iter().flatten());
    let label = meta
        .get("friendly_name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| homie_name.cloned())
        .unwrap_or_else(|| device.ident.device_id().to_string());
    entry(&mut m, "label", label);
    entry(&mut m, "meta", meta);
    entry(&mut m, "annotations", annotations);
    entry(&mut m, "matched", Value::Sequence(matched));

    let mut nodes = Mapping::new();
    if let Some(description) = device.description.as_ref() {
        for (node_id, node) in &description.nodes {
            let mut n = Mapping::new();
            entry(&mut n, "name", opt(node.name.as_ref()));
            entry(&mut n, "type", opt(node.r#type.as_ref()));
            let node_meta = overlay
                .and_then(|d| d.nodes.as_ref())
                .and_then(|nodes| nodes.get(node_id.as_str()))
                .and_then(|n| n.annotations.as_ref());
            entry(&mut n, "meta", annotation_maps(node_meta.into_iter().flatten()).0);
            let mut properties = Mapping::new();
            for (prop_id, prop) in &node.properties {
                let mut p = Mapping::new();
                entry(&mut p, "name", opt(prop.name.as_ref()));
                entry(&mut p, "datatype", serde_yaml_ng::to_value(prop.datatype).unwrap_or(Value::Null));
                entry(&mut p, "format", serde_yaml_ng::to_value(&prop.format).unwrap_or(Value::Null));
                entry(&mut p, "unit", opt(prop.unit.as_ref()));
                entry(&mut p, "settable", prop.settable);
                entry(&mut p, "retained", prop.retained);
                properties.insert(prop_id.to_string().into(), Value::Mapping(p));
            }
            entry(&mut n, "properties", properties);
            nodes.insert(node_id.to_string().into(), Value::Mapping(n));
        }
    }
    entry(&mut m, "nodes", nodes);
    Value::Mapping(m)
}

#[cfg(test)]
mod tests {
    use homie5::{device_description::HomieDeviceDescription, DeviceRef, HomieDeviceStatus, HomieDomain, HomieID};

    use super::*;
    use crate::{
        config_item::ItemHash,
        rules::parse_rule,
        templates::{TemplateStage, RULE_VERBATIM_KEYS, VIRTUAL_DEVICE_VERBATIM_KEYS},
        virtual_devices::VirtualDeviceSpec,
    };

    fn id(s: &str) -> HomieID {
        s.to_owned().try_into().unwrap()
    }

    fn yaml(text: &str) -> Value {
        serde_yaml_ng::from_str(text).unwrap()
    }

    fn add_contact_device(devices: &mut DeviceStore, device_id: &str) -> DeviceRef {
        let device_ref = DeviceRef::new(HomieDomain::Default, id(device_id));
        let description: HomieDeviceDescription = serde_json::from_value(serde_json::json!({
            "homie": "5.0",
            "version": 1,
            "name": format!("Contact {device_id}"),
            "nodes": { "contact": { "type": "hc-smarthome/v2/cap/contact",
                "properties": { "state": { "datatype": "boolean" } } } }
        }))
        .unwrap();
        devices.add(&device_ref, HomieDeviceStatus::Ready);
        devices.store_description(&device_ref, description);
        device_ref
    }

    fn set_annotations(devices: &mut DeviceStore, device_ref: &DeviceRef, annotations: serde_json::Value) {
        let overlay = serde_json::from_value(serde_json::json!({
            "schema": 1,
            "device": { "annotations": annotations }
        }))
        .unwrap();
        devices
            .get_device_mut(device_ref)
            .unwrap()
            .set_meta_overlay(id("manual"), overlay);
    }

    const WINDOWS_DEVICE: &str = r#"
id: windows
name: Windows
nodes:
  - for_each:
      query: { meta: { annotations: { tags: { any: window } } } }
      sort_by: label
    item:
      id: "{{ item.id }}"
      name: "{{ item.label }}"
      properties:
        - id: state
          datatype: boolean
          compound_spec:
            members: ["{{ item.matched[0] }}"]
"#;

    const WINDOWS_RULE: &str = r#"
name: windows-open
triggers:
  - properties:
      - for_each:
          query: { meta: { annotations: { tags: { any: window } } } }
        item: "{{ item.ref }}/contact/state"
    changed: {}
actions:
  - for_each:
      query: { meta: { annotations: { tags: { any: window } } } }
    item:
      type: set
      target: "{{ item.ref }}/contact/state"
      value: { Bool: false }
  - type: run
    script: |
      local t = {{a=1},{b=2}}
"#;

    #[test]
    fn for_each_follows_meta_changes() {
        let mut devices = DeviceStore::new();
        let window = add_contact_device(&mut devices, "window");
        let door = add_contact_device(&mut devices, "door");
        set_annotations(&mut devices, &window, serde_json::json!({ "tags": ["window"], "friendly_name": "Fenster" }));

        let mut stage = TemplateStage::new(VIRTUAL_DEVICE_VERBATIM_KEYS);
        stage.add_source(ItemHash::new(1, 1), "windows.yaml", yaml(WINDOWS_DEVICE));
        assert!(stage.uses_devices());

        let first = stage.reconcile(&DeviceContext::new(&devices));
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        assert_eq!(first.added.len(), 1);
        let spec: VirtualDeviceSpec = serde_yaml_ng::from_value(first.added[0].doc.clone()).unwrap();
        assert_eq!(spec.nodes.len(), 1);
        assert_eq!(spec.nodes[0].id.as_str(), "window");
        assert_eq!(first.added[0].doc["nodes"][0]["name"], yaml("Fenster"));
        assert_eq!(
            first.added[0].doc["nodes"][0]["properties"][0]["compound_spec"]["members"][0],
            yaml("homie/window/contact/state")
        );

        // An unrelated annotation change keeps the result and its hash.
        set_annotations(&mut devices, &door, serde_json::json!({ "tags": ["door"] }));
        let unchanged = stage.reconcile(&DeviceContext::new(&devices));
        assert!(unchanged.added.is_empty() && unchanged.removed.is_empty());

        // The door is annotated as a window: the device is replaced with two nodes.
        set_annotations(&mut devices, &door, serde_json::json!({ "tags": ["window"] }));
        let changed = stage.reconcile(&DeviceContext::new(&devices));
        assert_eq!(changed.removed, vec![first.added[0].hash]);
        assert_eq!(changed.added.len(), 1);
        let spec: VirtualDeviceSpec = serde_yaml_ng::from_value(changed.added[0].doc.clone()).unwrap();
        let nodes: Vec<&str> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(nodes, vec!["door", "window"], "sorted by label: Contact door < Fenster");
    }

    #[test]
    fn for_each_in_trigger_and_action_lists() {
        let mut devices = DeviceStore::new();
        for device_id in ["w1", "w2"] {
            let device_ref = add_contact_device(&mut devices, device_id);
            set_annotations(&mut devices, &device_ref, serde_json::json!({ "tags": ["window"] }));
        }
        let mut stage = TemplateStage::new(RULE_VERBATIM_KEYS);
        stage.add_source(ItemHash::new(1, 1), "windows.yaml", yaml(WINDOWS_RULE));

        let result = stage.reconcile(&DeviceContext::new(&devices));
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let doc = &result.added[0].doc;
        assert_eq!(doc["triggers"][0]["properties"], yaml("[homie/w1/contact/state, homie/w2/contact/state]"));
        assert_eq!(doc["actions"].as_sequence().unwrap().len(), 3);
        assert_eq!(doc["actions"][2]["script"], Value::String("local t = {{a=1},{b=2}}\n".into()));

        let rule = parse_rule(&serde_yaml_ng::to_string(doc).unwrap()).unwrap();
        assert_eq!(rule.actions.len(), 3);
    }
}
