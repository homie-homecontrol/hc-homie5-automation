use super::{run_rule_actions, while_condition::match_whilecondition_set, RuleContext};
use crate::{
    mqtt_client::{ManagedMqttClient, MqttPublishEvent},
    rules::{Rule, RuleTrigger},
};
use color_eyre::eyre::Result;
use hc_homie5::{DeviceStore, HomieMQTTClient};

pub async fn run_mqtt_rules(event: &MqttPublishEvent, ctx: &RuleContext<'_>) {
    match ctx.vdm.update_member_value_mqtt(&event.topic, &event.payload).await {
        Ok(_) => {}
        Err(err) => {
            log::warn!("Error updating virtual devices with value for {} - {}: {}", event.topic, event.payload, err);
        }
    }
    let devices = ctx.dm.read().await;
    for (hash, rule) in ctx.rules.iter().filter(|(_, rule)| {
        rule.triggers
            .iter()
            .any(|trigger| match_mqtt_trigger(event, trigger, &devices))
    }) {
        run_rule_actions(*hash, rule, event.into(), ctx).await;
    }
}

pub fn match_mqtt_trigger(event: &MqttPublishEvent, trigger: &RuleTrigger, devices: &DeviceStore) -> bool {
    if let RuleTrigger::MqttTrigger {
        topic,
        skip_retained,
        skip_duplicated,
        check_qos,
        qos,
        trigger_value,
        r#while,
    } = trigger
    {
        if mqtt_topic_match(topic, &event.topic) {
            if *skip_retained && event.retain {
                return false;
            }

            if *skip_duplicated && event.duplicate {
                return false;
            }

            if *check_qos && HomieMQTTClient::map_qos(qos) != event.qos {
                return false;
            }

            if !trigger_value.evaluate(&event.payload) {
                return false;
            }

            return match_whilecondition_set(r#while.as_ref(), devices);
        }
    }
    false
}

pub async fn subscribe_mqtt_trigger(rule: &Rule, mqtt_client: &ManagedMqttClient) -> Result<()> {
    for trigger in rule.triggers.iter() {
        if let RuleTrigger::MqttTrigger { topic, qos, .. } = trigger {
            mqtt_client.subscribe(topic, HomieMQTTClient::map_qos(qos)).await?
        }
    }
    Ok(())
}

pub async fn unsubscribe_mqtt_trigger(rule: &Rule, mqtt_client: &ManagedMqttClient) -> Result<()> {
    for trigger in rule.triggers.iter() {
        if let RuleTrigger::MqttTrigger { topic, .. } = trigger {
            mqtt_client.unsubscribe(topic).await?
        }
    }
    Ok(())
}
pub fn mqtt_topic_match(filter: &str, topic: &str) -> bool {
    // Quick return for exact matches
    if filter == topic {
        return true;
    }

    // Split filter and topic into parts
    let filter_parts = filter.split('/');
    let topic_parts = topic.split('/');

    // Iterate through both parts simultaneously
    for (filter_part, topic_part) in filter_parts.zip(topic_parts) {
        if filter_part == "#" {
            return true; // Match any remaining parts
        }
        if filter_part != "+" && filter_part != topic_part {
            return false; // Mismatch
        }
    }

    // Ensure both filter and topic are fully matched
    filter.split('/').count() == topic.split('/').count()
}
