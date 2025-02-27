use config_watcher::ConfigItemHash;

use super::{run_rule_actions, while_condition::match_whilecondition_set, RuleContext};
use crate::{
    cron_manager::{CronEvent, CronManager},
    rules::{Rule, RuleTrigger},
};

pub async fn run_cron_rules(event: &CronEvent, ctx: &RuleContext<'_>) {
    let Some(rule) = ctx.rules.get(&event.rule_hash) else {
        return;
    };
    let Some(trigger) = rule.triggers.get(event.trigger_index) else {
        return;
    };

    if let RuleTrigger::CronTrigger { schedule: _, r#while } = trigger {
        let devices = ctx.dm.read().await;
        if match_whilecondition_set(r#while.as_ref(), &devices) {
            run_rule_actions(event.rule_hash, rule, event.into(), ctx).await;
        }
    }
}

pub fn schedule_cron(rule_hash: ConfigItemHash, rule: &Rule, cron_manager: &CronManager) {
    for (trigger_index, trigger) in rule.triggers.iter().enumerate() {
        if let RuleTrigger::CronTrigger { schedule, r#while: _ } = trigger {
            cron_manager.schedule_cron(rule_hash, trigger_index, schedule);
        }
    }
}
