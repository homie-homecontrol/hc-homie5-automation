use super::{matching_rules, run_rule_actions, while_condition::match_whilecondition_set, RuleContext};
use crate::{
    mqtt_client::{ManagedMqttClient, MqttPublishEvent},
    rules::{Rule, RuleTrigger},
};
use color_eyre::eyre::Result;
use hc_homie5::client::HomieMQTTClient;
use hc_homie5::logging::{chain, truncate, PAYLOAD_MAX};
use hc_homie5::store::DeviceStore;

pub async fn run_mqtt_rules(event: &MqttPublishEvent, ctx: &RuleContext<'_>) {
    match ctx.vdm.update_member_value_mqtt(&event.topic, &event.payload).await {
        Ok(_) => {}
        Err(err) => {
            tracing::warn!(
                topic = %event.topic,
                payload = truncate(&event.payload, PAYLOAD_MAX),
                error = chain(err.as_ref()),
                "cannot update virtual devices from MQTT message"
            );
        }
    }
    let matching = matching_rules(ctx.rules, &*ctx.dm.read().await, |trigger, devices| {
        match_mqtt_trigger(event, trigger, devices)
    });
    for (hash, rule) in matching {
        run_rule_actions(hash, rule, event.into(), ctx).await;
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
/// Matches an MQTT topic against a topic filter following the MQTT spec
/// wildcard rules:
///
/// - `+` matches exactly one topic level.
/// - `#` is the multi-level wildcard. It must be the last level of the filter
///   and matches the parent level as well as any number of child levels
///   (e.g. `a/b/#` matches `a/b`, `a/b/c` and `a/b/c/d`).
/// - Without wildcards the filter and topic must match level-for-level
///   (including having the same number of levels).
///
/// Unlike a naive `zip`, a filter that is longer than the topic (and does not
/// end in `#`) correctly returns `false` instead of matching on the truncated
/// prefix.
pub fn mqtt_topic_match(filter: &str, topic: &str) -> bool {
    // Fast path for exact matches.
    if filter == topic {
        return true;
    }

    let filter_parts: Vec<&str> = filter.split('/').collect();
    let topic_parts: Vec<&str> = topic.split('/').collect();

    let mut i = 0;
    loop {
        // Multi-level wildcard: `#` must be the last filter level and matches
        // the parent level plus any (including zero) remaining topic levels.
        // `a/b/#` (i == 2) matches a topic with >= 2 levels.
        if i < filter_parts.len() && filter_parts[i] == "#" {
            return topic_parts.len() >= i;
        }

        // Topic exhausted: only a match if the filter is exhausted too.
        if i >= topic_parts.len() {
            return i >= filter_parts.len();
        }

        // Topic still has levels but the filter is exhausted: no match.
        if i >= filter_parts.len() {
            return false;
        }

        let f = filter_parts[i];
        let t = topic_parts[i];
        // `+` matches exactly one level; otherwise levels must be equal.
        if f != "+" && f != t {
            return false;
        }

        i += 1;
    }
}

#[cfg(test)]
mod topic_match_tests {
    use super::mqtt_topic_match;

    #[test]
    fn table_driven_topic_match() {
        // (filter, topic, expected)
        let cases = [
            // exact matches
            ("a/b", "a/b", true),
            ("a/b/c", "a/b/c", true),
            // multi-level wildcard matches parent and children
            ("a/b/#", "a/b", true),
            ("a/b/#", "a/b/c", true),
            ("a/b/#", "a/b/c/d", true),
            ("#", "a", true),
            ("#", "a/b/c", true),
            // multi-level wildcard does not match a divergent prefix
            ("a/b/#", "a/x", false),
            ("a/b/#", "a", false),
            // single-level wildcard
            ("a/+/c", "a/b/c", true),
            ("a/+/c", "a/c", false),
            ("a/+/c", "a/b/d/c", false),
            ("a/+", "a/b", true),
            ("a/+", "a/b/c", false),
            // level-count mismatches (the zip-truncation bug)
            ("a/b", "a", false),
            ("a/b", "a/b/c", false),
            ("a/b/c", "a/b", false),
            ("a", "a/b", false),
            // non-matching levels
            ("a/b", "x/y", false),
        ];

        for (filter, topic, expected) in cases {
            assert_eq!(mqtt_topic_match(filter, topic), expected, "filter={:?} topic={:?}", filter, topic);
        }
    }
}
