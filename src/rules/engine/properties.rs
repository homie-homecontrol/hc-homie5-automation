use crate::rules::RuleTrigger;
use hc_homie5::logging::{chain, truncate, PAYLOAD_MAX};
use hc_homie5::model::DiscoveryAction;
use hc_homie5::store::DeviceStore;
use homie5::{HomieValue, PropertyRef};

use super::{run_rule_actions, while_condition::match_whilecondition_set, RuleContext};

pub async fn run_subject_rules(event: &DiscoveryAction, ctx: &RuleContext<'_>) {
    match event {
        DiscoveryAction::DevicePropertyValueChanged { prop, from, to, .. } => {
            match ctx.vdm.update_member_value_prop(prop, to).await {
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(
                        device = %prop.device_id(),
                        node = %prop.node_id(),
                        property = %prop.prop_id(),
                        value = truncate(&to.to_string(), PAYLOAD_MAX),
                        error = chain(err.as_ref()),
                        "cannot update virtual devices from property value"
                    );
                }
            }
            if from.is_none() {
                return;
            }
            let devices = ctx.dm.read().await;
            for (hash, rule) in ctx.rules.iter().filter(|(_, rule)| {
                rule.triggers
                    .iter()
                    .any(|trigger| match_prop_change(prop, trigger, from.as_ref(), to, &devices))
            }) {
                if let Ok(event) = event.try_into() {
                    run_rule_actions(*hash, rule, event, ctx).await;
                }
            }
        }
        DiscoveryAction::DevicePropertyValueTriggered { prop, value } => {
            match ctx.vdm.update_member_value_prop(prop, value).await {
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(
                        device = %prop.device_id(),
                        node = %prop.node_id(),
                        property = %prop.prop_id(),
                        value = truncate(&value.to_string(), PAYLOAD_MAX),
                        error = chain(err.as_ref()),
                        "cannot update virtual devices from property value"
                    );
                }
            }
            let devices = ctx.dm.read().await;
            for (hash, rule) in ctx.rules.iter().filter(|(_, rule)| {
                rule.triggers
                    .iter()
                    .any(|trigger| match_prop_trigger(prop, trigger, value, &devices))
            }) {
                if let Ok(event) = event.try_into() {
                    run_rule_actions(*hash, rule, event, ctx).await;
                }
            }
        }
        _ => {}
    };
}

fn match_prop_trigger(prop: &PropertyRef, trigger: &RuleTrigger, value: &HomieValue, devices: &DeviceStore) -> bool {
    match trigger {
        RuleTrigger::PropertyTriggered {
            properties,
            queries,
            trigger_value,
            r#while,
        } => {
            // Check if either properties or queries are non-empty
            let properties_match = !properties.is_empty() && properties.iter().any(|p| p == prop);
            let queries_match = !queries.is_empty() && queries.iter().any(|query| query.match_query(prop));

            // If both are empty, return false
            if properties.is_empty() && queries.is_empty() {
                return false;
            }

            // If neither properties nor queries match, return false
            if !(properties_match || queries_match) {
                return false;
            }

            if !trigger_value.evaluate(value) {
                return false;
            }
            match_whilecondition_set(r#while.as_ref(), devices)
        }
        _ => false,
    }
}
fn match_prop_change(
    prop: &PropertyRef,
    trigger: &RuleTrigger,
    from: Option<&HomieValue>,
    to: &HomieValue,
    devices: &DeviceStore,
) -> bool {
    match trigger {
        RuleTrigger::PropertyChanged {
            properties,
            queries,
            changed,
            r#while,
        } => {
            // Check if either properties or queries are non-empty
            let properties_match = !properties.is_empty() && properties.iter().any(|p| p == prop);
            let queries_match = !queries.is_empty() && queries.iter().any(|query| query.match_query(prop));

            // If both are empty, return false
            if properties.is_empty() && queries.is_empty() {
                return false;
            }

            // If neither properties nor queries match, return false
            if !(properties_match || queries_match) {
                return false;
            }

            let from = match (from, &changed.from) {
                (Some(from), Some(rule_from)) => rule_from.evaluate(from),
                (_, None) => true,
                _ => false,
            };
            let to = match &changed.to {
                Some(rule_to) => rule_to.evaluate(to),
                None => true,
            };
            if !(from && to) {
                return false;
            }
            match_whilecondition_set(r#while.as_ref(), devices)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ChangedTrigger;
    use hc_homie5::value::ValueCondition;
    use std::str::FromStr;

    fn prop(s: &str) -> PropertyRef {
        PropertyRef::from_str(s).unwrap()
    }

    fn triggered(properties: Vec<PropertyRef>, trigger_value: ValueCondition<HomieValue>) -> RuleTrigger {
        RuleTrigger::PropertyTriggered {
            properties,
            queries: Vec::new(),
            trigger_value,
            r#while: None,
        }
    }

    fn changed(properties: Vec<PropertyRef>, changed: ChangedTrigger) -> RuleTrigger {
        RuleTrigger::PropertyChanged {
            properties,
            queries: Vec::new(),
            changed,
            r#while: None,
        }
    }

    // --- match_prop_trigger ---

    #[test]
    fn trigger_property_and_value_match() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let trigger = triggered(vec![p.clone()], ValueCondition::Value(HomieValue::Bool(true)));
        assert!(match_prop_trigger(&p, &trigger, &HomieValue::Bool(true), &devices));
    }

    #[test]
    fn trigger_value_condition_fails() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let trigger = triggered(vec![p.clone()], ValueCondition::Value(HomieValue::Bool(true)));
        // Property matches but value condition (expects true) fails on false.
        assert!(!match_prop_trigger(&p, &trigger, &HomieValue::Bool(false), &devices));
    }

    #[test]
    fn trigger_empty_properties_and_queries_returns_false() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let trigger = triggered(Vec::new(), ValueCondition::Value(HomieValue::Bool(true)));
        assert!(!match_prop_trigger(&p, &trigger, &HomieValue::Bool(true), &devices));
    }

    #[test]
    fn trigger_non_matching_property_returns_false() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let other = prop("homie/dev/node/other");
        let trigger = triggered(vec![other], ValueCondition::Value(HomieValue::Bool(true)));
        assert!(!match_prop_trigger(&p, &trigger, &HomieValue::Bool(true), &devices));
    }

    #[test]
    fn trigger_wrong_variant_returns_false() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        // A PropertyChanged trigger never matches in match_prop_trigger.
        let trigger = changed(vec![p.clone()], ChangedTrigger { from: None, to: None });
        assert!(!match_prop_trigger(&p, &trigger, &HomieValue::Bool(true), &devices));
    }

    // --- match_prop_change ---

    #[test]
    fn change_to_only_matches() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let trigger = changed(
            vec![p.clone()],
            ChangedTrigger {
                from: None,
                to: Some(ValueCondition::Value(HomieValue::Bool(true))),
            },
        );
        // from event is None, but rule sets no `from` condition -> from passes.
        assert!(match_prop_change(&p, &trigger, None, &HomieValue::Bool(true), &devices));
    }

    #[test]
    fn change_from_set_but_event_from_none_returns_false() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let trigger = changed(
            vec![p.clone()],
            ChangedTrigger {
                from: Some(ValueCondition::Value(HomieValue::Bool(false))),
                to: None,
            },
        );
        // Rule requires a `from` value but the event carries no previous value.
        assert!(!match_prop_change(&p, &trigger, None, &HomieValue::Bool(true), &devices));
    }

    #[test]
    fn change_both_from_and_to_match() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let trigger = changed(
            vec![p.clone()],
            ChangedTrigger {
                from: Some(ValueCondition::Value(HomieValue::Bool(false))),
                to: Some(ValueCondition::Value(HomieValue::Bool(true))),
            },
        );
        assert!(match_prop_change(&p, &trigger, Some(&HomieValue::Bool(false)), &HomieValue::Bool(true), &devices));
        // `to` mismatch -> false.
        assert!(!match_prop_change(&p, &trigger, Some(&HomieValue::Bool(false)), &HomieValue::Bool(false), &devices));
    }

    #[test]
    fn change_empty_properties_and_queries_returns_false() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let trigger = changed(Vec::new(), ChangedTrigger { from: None, to: None });
        assert!(!match_prop_change(&p, &trigger, None, &HomieValue::Bool(true), &devices));
    }

    #[test]
    fn change_non_matching_property_returns_false() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        let other = prop("homie/dev/node/other");
        let trigger = changed(vec![other], ChangedTrigger { from: None, to: None });
        assert!(!match_prop_change(&p, &trigger, None, &HomieValue::Bool(true), &devices));
    }

    #[test]
    fn change_wrong_variant_returns_false() {
        let devices = DeviceStore::new();
        let p = prop("homie/dev/node/prop");
        // A PropertyTriggered trigger never matches in match_prop_change.
        let trigger = triggered(vec![p.clone()], ValueCondition::Value(HomieValue::Bool(true)));
        assert!(!match_prop_change(&p, &trigger, None, &HomieValue::Bool(true), &devices));
    }
}
