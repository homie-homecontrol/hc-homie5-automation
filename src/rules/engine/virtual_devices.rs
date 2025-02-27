use crate::rules::RuleTrigger;
use hc_homie5::DeviceStore;
use homie5::{Homie5Message, PropertyRef};

use super::{run_rule_actions, while_condition::match_whilecondition_set, RuleContext};

pub async fn run_on_set_rules(event: &Homie5Message, ctx: &RuleContext<'_>) {
    if let Homie5Message::PropertySet { property, set_value } = event {
        let devices = ctx.dm.read().await;
        for (hash, rule) in ctx.rules.iter().filter(|(_, rule)| {
            rule.triggers
                .iter()
                .any(|trigger| match_prop_set(property, set_value, trigger, &devices))
        }) {
            if let Ok(event) = event.try_into() {
                run_rule_actions(*hash, rule, event, ctx).await;
            }
        }
    };
}

fn match_prop_set(prop: &PropertyRef, on_set_value: &String, trigger: &RuleTrigger, devices: &DeviceStore) -> bool {
    match trigger {
        RuleTrigger::OnSetEventTrigger {
            subjects: on_set_subjects,
            queries: on_set_queries,
            set_value,
            r#while,
        } => {
            // Check if either subjects or queries are non-empty
            let subjects_match = !on_set_subjects.is_empty() && on_set_subjects.iter().any(|subj| subj == prop);
            let queries_match =
                !on_set_queries.is_empty() && on_set_queries.iter().any(|query| query.match_query(prop));

            // If both are empty, return false
            if on_set_subjects.is_empty() && on_set_queries.is_empty() {
                return false;
            }

            // If neither subjects nor queries match, return false
            if !(subjects_match || queries_match) {
                return false;
            }

            if !set_value.evaluate(on_set_value) {
                return false;
            }

            match_whilecondition_set(r#while.as_ref(), devices)
        }
        _ => false,
    }
}
