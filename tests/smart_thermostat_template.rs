//! The office smart thermostat as a template instance expands to the deployed virtual device.
//!
//! `deployed-*` are copies of the smarthome config (71_hchomie_config/10_base) at the time the
//! template was written; `smart-thermostat.tpl.yaml` and `smart-heating.v5device.yaml` replace the
//! deployed virtual device file.

use config_watcher::{Tokenizer, YamlTokenizer};
use hc_homie5::template::EmptyContext;
use hc_homie5_automation::{
    config_item::ItemHash,
    rules::parse_rule,
    templates::{
        parse_config_document, ConfigDocument, TemplateStage, RULE_VERBATIM_KEYS, VIRTUAL_DEVICE_VERBATIM_KEYS,
    },
    virtual_devices::VirtualDeviceSpec,
};
use serde_yaml_ng::Value;

const TEMPLATE: &str = include_str!("fixtures/smart-thermostat/smart-thermostat.tpl.yaml");
const INSTANCE: &str = include_str!("fixtures/smart-thermostat/smart-heating.v5device.yaml");
const DEPLOYED_DEVICE: &str = include_str!("fixtures/smart-thermostat/deployed-smart-heating.v5device.yaml");
const DEPLOYED_RULES: &str = include_str!("fixtures/smart-thermostat/deployed-smart-heating.hc5rule.yaml");

fn parse_vdev(text: &str) -> ConfigDocument<VirtualDeviceSpec> {
    parse_config_document(text, VIRTUAL_DEVICE_VERBATIM_KEYS, |t: &str| serde_yaml_ng::from_str::<VirtualDeviceSpec>(t))
        .unwrap()
}

fn expand_instance() -> Value {
    let mut stage = TemplateStage::new(VIRTUAL_DEVICE_VERBATIM_KEYS);
    let ConfigDocument::Template(template) = parse_vdev(TEMPLATE) else {
        panic!("the template file must hold a template document");
    };
    stage
        .add_template(ItemHash::new(1, 1), "smart-thermostat.tpl.yaml", template)
        .unwrap();
    let ConfigDocument::Dynamic(instance) = parse_vdev(INSTANCE) else {
        panic!("the instance must be a dynamic document");
    };
    stage.add_source(ItemHash::new(2, 1), "smart-heating.v5device.yaml", instance);

    let result = stage.reconcile(&EmptyContext);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.added.len(), 1);
    result.added.into_iter().next().unwrap().doc
}

#[test]
fn smart_thermostat_instance_equals_deployed_spec() {
    let expanded = expand_instance();
    let deployed: Value = serde_yaml_ng::from_str(DEPLOYED_DEVICE).unwrap();

    assert_eq!(expanded, deployed);

    let from_template: VirtualDeviceSpec = serde_yaml_ng::from_value(expanded).unwrap();
    let from_deployed: VirtualDeviceSpec = serde_yaml_ng::from_str(DEPLOYED_DEVICE).unwrap();
    assert_eq!(format!("{from_template:?}"), format!("{from_deployed:?}"));
    assert_eq!(from_template.id.as_str(), "bue-thermostat");
}

#[test]
fn smart_heating_rules_load_unchanged() {
    // The Lua `script` values keep the rules static: they load exactly as before.
    for doc in YamlTokenizer
        .tokenize(DEPLOYED_RULES)
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        let parsed = parse_config_document(doc, RULE_VERBATIM_KEYS, parse_rule).unwrap();
        let ConfigDocument::Static(rule) = parsed else {
            panic!("deployed rule must stay static: {doc}");
        };
        assert!(rule.name.contains("heating"), "{}", rule.name);
    }
}
