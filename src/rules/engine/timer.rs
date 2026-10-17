use crate::{rules::RuleTrigger, timer_manager::TimerEvent};
use hc_homie5::store::DeviceStore;

use super::{matching_rules, run_rule_actions, while_condition::match_whilecondition_set, RuleContext};

pub async fn run_timer_rules(event: &TimerEvent, ctx: &RuleContext<'_>) {
    let matching = matching_rules(ctx.rules, &*ctx.dm.read().await, |trigger, devices| {
        match_timer(event.id.as_str(), trigger, devices)
    });
    for (hash, rule) in matching {
        run_rule_actions(hash, rule, event.into(), ctx).await;
    }
}

fn match_timer(id: &str, trigger: &RuleTrigger, devices: &DeviceStore) -> bool {
    match trigger {
        RuleTrigger::TimerTrigger { timer_id, r#while } => {
            timer_id == id && match_whilecondition_set(r#while.as_ref(), devices)
        }
        _ => false,
    }
}
