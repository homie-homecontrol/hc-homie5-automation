use super::RuleContext;
use crate::lua_runtime::{
    setup_custom_loader, LuaEvent, LuaHomie, LuaTimer, LuaUtils, LuaValueStore, LuaVirtualDecvice,
};
use crate::rules::{MapSetFrom, RuleAction, TimerDef};
use crate::solar_events::SolarEvent;
use crate::{
    rules::{Rule, RuleTriggerEvent},
    timer_manager::TimerManager,
};
use color_eyre::eyre::Result;
use config_watcher::ConfigItemHash;
use hc_homie5::HomieMQTTClient;
use hc_homie5::MappingResult;
use homie5::{HomieValue, ToTopic};
use mlua::ExternalResult;
use mlua::Lua;
use std::borrow::Cow;

pub async fn run_rule_actions<'a>(
    rule_hash: ConfigItemHash,
    rule: &Rule,
    trigger_event: RuleTriggerEvent<'a>,
    ctx: &RuleContext<'a>,
) {
    let filename = ctx
        .rules
        .get_filename(rule_hash)
        .map(|f| f.to_owned())
        .unwrap_or_default();

    log::debug!("{} ({}) -- rule triggered", rule.name, filename);
    for (index, action) in rule.actions.iter().enumerate() {
        log::debug!("{}.action[{}] -- action started", rule.name, index);
        match execute_rule_action(rule_hash, &rule.name, action, &trigger_event, ctx, false).await {
            Ok(_) => {
                log::debug!("{}.action[{}] -- action finished", rule.name, index);
            }
            Err(err) => {
                log::error!("{}.action[{}] -- Error executing rule action no: {}", rule.name, index, err);
            }
        }
    }
    log::debug!("{} ({}) -- rule finished", rule.name, filename);
}

pub async fn execute_rule_action<'a>(
    rule_hash: ConfigItemHash,
    rule_name: &str,
    action: &RuleAction,
    trigger_event: &RuleTriggerEvent<'a>,
    ctx: &RuleContext<'a>,
    ignore_timer: bool,
) -> Result<()> {
    match action {
        crate::rules::RuleAction::Set { target, value, timer } => {
            if ignore_timer || timer.is_none() {
                ctx.dm.set_command(target, value).await?;
            } else if let Some(timer) = timer {
                // Handle the timer if it exists
                handle_timer(rule_hash, rule_name, Some(action.clone()), trigger_event, timer, ctx.timers).await?;
            }
        }
        crate::rules::RuleAction::MapSet { target, mapping, timer } => {
            if ignore_timer || timer.is_none() {
                match trigger_event {
                    RuleTriggerEvent::PropertyChanged { to, .. } => {
                        if let MappingResult::Mapped(value) = mapping.map_to(&MapSetFrom::HomieValue(Cow::Borrowed(to)))
                        {
                            ctx.dm.set_command(target, value).await?;
                        }
                    }
                    RuleTriggerEvent::PropertyTriggered { value, .. } => {
                        if let MappingResult::Mapped(value) =
                            mapping.map_to(&MapSetFrom::HomieValue(Cow::Borrowed(value)))
                        {
                            ctx.dm.set_command(target, value).await?;
                        }
                    }
                    RuleTriggerEvent::OnSet { value, .. } => {
                        if let MappingResult::Mapped(value) = mapping.map_to(&MapSetFrom::String(Cow::Borrowed(value)))
                        {
                            ctx.dm.set_command(target, value).await?;
                        }
                    }
                    RuleTriggerEvent::Timer(cow) => {
                        if let MappingResult::Mapped(value) =
                            mapping.map_to(&MapSetFrom::String(Cow::Borrowed(&cow.id)))
                        {
                            ctx.dm.set_command(target, value).await?;
                        }
                    }
                    RuleTriggerEvent::Mqtt(cow) => {
                        if let MappingResult::Mapped(value) =
                            mapping.map_to(&MapSetFrom::String(Cow::Borrowed(&cow.payload)))
                        {
                            ctx.dm.set_command(target, value).await?;
                        }
                    }
                    RuleTriggerEvent::Solar(cow) => match cow {
                        Cow::Borrowed(SolarEvent::At(solar_phase))
                        | Cow::Borrowed(SolarEvent::After(solar_phase, _))
                        | Cow::Borrowed(SolarEvent::Before(solar_phase, _)) => {
                            if let MappingResult::Mapped(value) =
                                mapping.map_to(&MapSetFrom::SolarPhase(Cow::Borrowed(solar_phase)))
                            {
                                ctx.dm.set_command(target, value).await?;
                            }
                        }
                        Cow::Owned(SolarEvent::At(solar_phase))
                        | Cow::Owned(SolarEvent::After(solar_phase, _))
                        | Cow::Owned(SolarEvent::Before(solar_phase, _)) => {
                            if let MappingResult::Mapped(value) =
                                mapping.map_to(&MapSetFrom::SolarPhase(Cow::Borrowed(solar_phase)))
                            {
                                ctx.dm.set_command(target, value).await?;
                            }
                        }
                    },
                    _ => return Ok(()),
                };
            } else if let Some(timer) = timer {
                handle_timer(rule_hash, rule_name, Some(action.clone()), trigger_event, timer, ctx.timers).await?;
            }
        }
        crate::rules::RuleAction::Toggle { target } => {
            let devices = ctx.dm.read().await;
            let value = devices.get_device(target.device_ref()).and_then(|device| {
                device
                    .prop_values
                    .get_value_entry(target.prop_pointer())
                    .and_then(|prop_value_entry| prop_value_entry.value.as_ref())
            });
            if let Some(HomieValue::Bool(value)) = value {
                ctx.dm.set_command(target, &HomieValue::Bool(!value)).await?;
            }
        }
        RuleAction::Mqtt {
            topic,
            value,
            qos,
            retain,
        } => {
            ctx.mqtt_client
                .publish(topic, HomieMQTTClient::map_qos(qos), *retain, value.as_bytes())
                .await?;
        }
        RuleAction::Timer { timer } => {
            handle_timer(rule_hash, rule_name, None, trigger_event, timer, ctx.timers).await?;
        }
        RuleAction::CancelTimer { timer_id } => {
            ctx.timers.cancel_timer(timer_id);
        }
        RuleAction::Run { script, timer } => {
            if ignore_timer || timer.is_none() {
                log::debug!("{} -- starting script", rule_name);
                match run_script(script, rule_hash, action, trigger_event, ctx).await {
                    Ok(_) => {
                        log::debug!("{} -- script run successfull", rule_name);
                    }
                    Err(err) => {
                        log::error!("{} -- Error running script: {}", rule_name, err)
                    }
                }
            } else if let Some(timer) = timer {
                // Handle the timer if it exists
                handle_timer(rule_hash, rule_name, Some(action.clone()), trigger_event, timer, ctx.timers).await?;
            }
        }
    }
    Ok(())
}

async fn handle_timer(
    rule_hash: ConfigItemHash,
    rule_name: &str,
    action: Option<RuleAction>,
    trigger_event: &RuleTriggerEvent<'_>,
    timer_def: &TimerDef,
    timers: &TimerManager,
) -> Result<()> {
    let timer_id = if timer_def.triggerbound {
        trigger_event
            .property_ref()
            .map(|prop| format!("{}-{}", timer_def.id, prop.to_topic().build()))
            .unwrap_or_else(|| timer_def.id.clone())
    } else {
        timer_def.id.clone()
    };

    let mut cancelled = false;

    if let Some(cancelcondition) = &timer_def.cancelcondition {
        // log::debug!(
        //     "{} -- Checking cancel condidtion {:?} against value: {:?}",
        //     rule_name,
        //     cancelcondition,
        //     trigger_event.value()
        // );
        if let Some(value) = trigger_event.value() {
            if cancelcondition.evaluate(value) {
                timers.cancel_timer(&timer_id);
                log::debug!("{} -- Cancelled timer: {}", rule_name, &timer_id);
                cancelled = true;
            }
        }
    }

    if !cancelled {
        let trigger_event = action.is_some().then(|| trigger_event.clone());
        timers.create_timer(rule_hash, timer_id, timer_def.duration, timer_def.repeat, action, trigger_event);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_script(
    script: &str,
    rule_hash: ConfigItemHash,
    _action: &RuleAction,
    trigger_event: &RuleTriggerEvent<'_>,
    ctx: &RuleContext<'_>,
) -> mlua::Result<()> {
    log::trace!("preparing script run");
    let lua = Lua::new();

    let lua_homie = LuaHomie { dm: ctx.dm.clone() };
    let lua_virtual_device = LuaVirtualDecvice {
        vdm: ctx.vdm.as_proxy(),
    };

    let lua_timer = LuaTimer {
        timers: ctx.timers.clone(),
        rule_hash,
    };

    let lua_value_store = LuaValueStore {
        store: ctx.value_store.clone(),
    };

    let lua_event = LuaEvent {
        event: trigger_event.to_owned(),
    };

    let lua_utils = LuaUtils {
        mqtt_client: ctx.mqtt_client.clone(),
    };

    // Set the function in the Lua environment
    let globals = lua.globals();

    globals.set("utils", lua_utils).into_lua_err()?;
    globals.set("homie", lua_homie).into_lua_err()?;
    globals.set("virtual_device", lua_virtual_device).into_lua_err()?;
    globals.set("timers", lua_timer).into_lua_err()?;
    globals.set("value_store", lua_value_store).into_lua_err()?;
    globals.set("event", lua_event).into_lua_err()?;

    setup_custom_loader(&lua, ctx.lmm.file_contents()).await?;

    // run the script
    log::trace!("executing script");
    lua.load(script).exec_async().await.into_lua_err()?;
    log::trace!("finished executing script");
    Ok(())
}
