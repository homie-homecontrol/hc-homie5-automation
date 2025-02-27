use super::{run_rule_actions, while_condition::match_whilecondition_set, RuleContext};
use crate::{
    rules::{Rule, RuleTrigger},
    solar_events::{SolarEvent, SolarEventManager, SolarEventTrigger},
};
use config_watcher::ConfigItemHash;
use hc_homie5::DeviceStore;

pub async fn run_solar_rules(event: &SolarEvent, ctx: &RuleContext<'_>) {
    let devices = &ctx.dm.read().await;
    for (hash, rule) in ctx.rules.iter().filter(|(_, rule)| {
        rule.triggers
            .iter()
            .any(|trigger| match_solar_event(event, trigger, devices))
    }) {
        run_rule_actions(*hash, rule, event.into(), ctx).await;
    }
}

fn match_solar_event(event: &SolarEvent, trigger: &RuleTrigger, devices: &DeviceStore) -> bool {
    match trigger {
        RuleTrigger::SolarEventTrigger { sun_phase, r#while } => {
            let matched = match event {
                SolarEvent::At(event_sun_phase) => sun_phase == event_sun_phase,
                SolarEvent::After(_event_sun_phase, _event_duration) => {
                    return false;
                }
                SolarEvent::Before(_event_sun_phase, _event_duration) => {
                    return false;
                }
            };

            if !matched {
                return false;
            }

            match_whilecondition_set(r#while.as_ref(), devices)
        }
        RuleTrigger::SolarEventTriggerAfter {
            sun_phase,
            min_after,
            r#while,
        } => {
            let matched = match event {
                SolarEvent::At(_event_sun_phase) => {
                    return false;
                }
                SolarEvent::After(event_sun_phase, event_duration) => {
                    min_after == event_duration && event_sun_phase == sun_phase
                }
                SolarEvent::Before(_event_sun_phase, _event_duration) => {
                    return false;
                }
            };

            if !matched {
                return false;
            }

            match_whilecondition_set(r#while.as_ref(), devices)
        }
        RuleTrigger::SolarEventTriggerBefore {
            sun_phase,
            min_before,
            r#while,
        } => {
            let matched = match event {
                SolarEvent::At(_event_sun_phase) => {
                    return false;
                }
                SolarEvent::After(_event_sun_phase, _event_duration) => {
                    return false;
                }
                SolarEvent::Before(event_sun_phase, event_duration) => {
                    min_before == event_duration && event_sun_phase == sun_phase
                }
            };

            if !matched {
                return false;
            }

            match_whilecondition_set(r#while.as_ref(), devices)
        }
        _ => false,
    }
}

pub async fn add_solar_triggers(rule_hash: ConfigItemHash, rule: &Rule, solar_manager: &SolarEventManager) {
    for trigger in rule.triggers.iter() {
        match trigger {
            RuleTrigger::SolarEventTrigger { sun_phase, r#while: _ } => {
                if let Err(err) = solar_manager
                    .add_trigger(SolarEventTrigger {
                        rule_hash,
                        event: SolarEvent::At(*sun_phase),
                    })
                    .await
                {
                    log::warn!("Error adding solar event trigger: {}", err)
                }
            }
            RuleTrigger::SolarEventTriggerAfter {
                sun_phase,
                min_after,
                r#while: _,
            } => {
                if let Err(err) = solar_manager
                    .add_trigger(SolarEventTrigger {
                        rule_hash,
                        event: SolarEvent::After(*sun_phase, *min_after),
                    })
                    .await
                {
                    log::warn!("Error adding solar event trigger: {}", err)
                }
            }
            RuleTrigger::SolarEventTriggerBefore {
                sun_phase,
                min_before,
                r#while: _,
            } => {
                if let Err(err) = solar_manager
                    .add_trigger(SolarEventTrigger {
                        rule_hash,
                        event: SolarEvent::Before(*sun_phase, *min_before),
                    })
                    .await
                {
                    log::warn!("Error adding solar event trigger: {}", err)
                }
            }
            _ => (),
        }
    }
}
