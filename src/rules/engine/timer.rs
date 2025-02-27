use crate::{rules::RuleTrigger, timer_manager::TimerEvent};
use hc_homie5::DeviceStore;

use super::{run_rule_actions, while_condition::match_whilecondition_set, RuleContext};

pub async fn run_timer_rules(event: &TimerEvent, ctx: &RuleContext<'_>) {
    log::debug!("Timer Event: {:#?}", event);
    let devices = &ctx.dm.read().await;
    for (hash, rule) in ctx.rules.iter().filter(|(_, rule)| {
        rule.triggers
            .iter()
            .any(|trigger| match_timer(event.id.as_str(), trigger, devices))
    }) {
        run_rule_actions(*hash, rule, event.into(), ctx).await;
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
