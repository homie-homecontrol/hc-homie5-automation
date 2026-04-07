use crate::rules::RuleTrigger;
use hc_homie5::model::DiscoveryAction;
use hc_homie5::store::DeviceStore;
use homie5::{HomieValue, PropertyRef, ToTopic};

use super::{run_rule_actions, while_condition::match_whilecondition_set, RuleContext};

pub async fn run_subject_rules(event: &DiscoveryAction, ctx: &RuleContext<'_>) {
    match event {
        DiscoveryAction::DevicePropertyValueChanged { prop, from, to, .. } => {
            match ctx.vdm.update_member_value_prop(prop, to).await {
                Ok(_) => {}
                Err(err) => {
                    log::warn!(
                        "Error updating virtual devices with value for {} - {}: {}",
                        prop.to_topic().build(),
                        to,
                        err
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
                    log::warn!(
                        "Error updating virtual devices with value for {} - {}: {}",
                        prop.to_topic().build(),
                        value,
                        err
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
