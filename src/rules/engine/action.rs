use super::RuleContext;
use crate::lua_runtime::{
    setup_custom_loader, LuaEvent, LuaHomie, LuaTimer, LuaUtils, LuaValueStore, LuaVirtualDevice,
};
use crate::rules::{MapSetFrom, RuleAction, TimerDef};
use crate::solar_events::SolarEvent;
use crate::{
    rules::{Rule, RuleTriggerEvent},
    timer_manager::TimerManager,
};
use color_eyre::eyre::{eyre, Result};
use config_watcher::ConfigItemHash;
use hc_homie5::client::HomieMQTTClient;
use hc_homie5::value::MappingResult;
use homie5::{HomieValue, PropertyRef, ToTopic};
use mlua::ExternalResult;
use mlua::{HookTriggers, Lua, VmState};
use std::borrow::Cow;
use std::time::{Duration, Instant};

/// Rule actions taking longer than this are logged with a warning. Actions run
/// sequentially inline in the event loop (D-7), so a slow action delays all
/// subsequent events — this makes such stalls visible.
const SLOW_ACTION_WARN_THRESHOLD: Duration = Duration::from_millis(250);

/// How many Lua VM instructions to execute between deadline checks in the
/// instruction-count hook. Large enough to keep overhead negligible, small
/// enough to cancel a CPU-bound busy loop within (sub-)milliseconds of the
/// deadline.
const LUA_HOOK_INSTRUCTION_INTERVAL: u32 = 16384;

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
        let started = Instant::now();
        match execute_rule_action(rule_hash, &rule.name, action, &trigger_event, ctx, false).await {
            Ok(_) => {
                log::debug!("{}.action[{}] -- action finished", rule.name, index);
            }
            Err(err) => {
                log::error!("{}.action[{}] -- Error executing rule action no: {}", rule.name, index, err);
            }
        }
        let elapsed = started.elapsed();
        if elapsed > SLOW_ACTION_WARN_THRESHOLD {
            log::warn!(
                "{}.action[{}] -- slow rule action took {:?} (threshold {:?}); actions run inline and delay event processing",
                rule.name,
                index,
                elapsed,
                SLOW_ACTION_WARN_THRESHOLD
            );
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
        RuleAction::Set { target, value, timer } => {
            if ignore_timer || timer.is_none() {
                set_targets(ctx, target, value).await?;
            } else if let Some(timer) = timer {
                // Handle the timer if it exists
                handle_timer(rule_hash, rule_name, Some(action.clone()), trigger_event, timer, ctx.timers).await?;
            }
        }
        RuleAction::MapSet { target, mapping, timer } => {
            if ignore_timer || timer.is_none() {
                match trigger_event {
                    RuleTriggerEvent::PropertyChanged { to, .. } => {
                        if let MappingResult::Mapped(value) = mapping.map_to(&MapSetFrom::HomieValue(Cow::Borrowed(to)))
                        {
                            set_targets(ctx, target, value).await?;
                        }
                    }
                    RuleTriggerEvent::PropertyTriggered { value, .. } => {
                        if let MappingResult::Mapped(value) =
                            mapping.map_to(&MapSetFrom::HomieValue(Cow::Borrowed(value)))
                        {
                            set_targets(ctx, target, value).await?;
                        }
                    }
                    RuleTriggerEvent::OnSet { value, .. } => {
                        if let MappingResult::Mapped(value) = mapping.map_to(&MapSetFrom::String(Cow::Borrowed(value)))
                        {
                            set_targets(ctx, target, value).await?;
                        }
                    }
                    RuleTriggerEvent::Timer(cow) => {
                        if let MappingResult::Mapped(value) =
                            mapping.map_to(&MapSetFrom::String(Cow::Borrowed(&cow.id)))
                        {
                            set_targets(ctx, target, value).await?;
                        }
                    }
                    RuleTriggerEvent::Mqtt(cow) => {
                        if let MappingResult::Mapped(value) =
                            mapping.map_to(&MapSetFrom::String(Cow::Borrowed(&cow.payload)))
                        {
                            set_targets(ctx, target, value).await?;
                        }
                    }
                    RuleTriggerEvent::Solar(cow) => match cow {
                        Cow::Borrowed(SolarEvent::At(solar_phase))
                        | Cow::Borrowed(SolarEvent::After(solar_phase, _))
                        | Cow::Borrowed(SolarEvent::Before(solar_phase, _)) => {
                            if let MappingResult::Mapped(value) =
                                mapping.map_to(&MapSetFrom::SolarPhase(Cow::Borrowed(solar_phase)))
                            {
                                set_targets(ctx, target, value).await?;
                            }
                        }
                        Cow::Owned(SolarEvent::At(solar_phase))
                        | Cow::Owned(SolarEvent::After(solar_phase, _))
                        | Cow::Owned(SolarEvent::Before(solar_phase, _)) => {
                            if let MappingResult::Mapped(value) =
                                mapping.map_to(&MapSetFrom::SolarPhase(Cow::Borrowed(solar_phase)))
                            {
                                set_targets(ctx, target, value).await?;
                            }
                        }
                    },
                    _ => return Ok(()),
                };
            } else if let Some(timer) = timer {
                handle_timer(rule_hash, rule_name, Some(action.clone()), trigger_event, timer, ctx.timers).await?;
            }
        }
        RuleAction::Toggle { target } => {
            let commands = {
                let devices = ctx.dm.read().await;
                target
                    .iter()
                    .filter_map(|target| {
                        let value = devices.get_device(target.device_ref()).and_then(|device| {
                            device
                                .prop_values
                                .get_value_entry(target.prop_pointer())
                                .and_then(|prop_value_entry| prop_value_entry.value.as_ref())
                        });

                        match value {
                            Some(HomieValue::Bool(value)) => Some((target, HomieValue::Bool(!*value))),
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>()
            };

            set_target_commands(ctx, commands).await?;
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
        RuleAction::Run {
            script,
            timer,
            script_timeout,
        } => {
            if ignore_timer || timer.is_none() {
                log::debug!("{} -- starting script", rule_name);
                match run_script(script, rule_hash, rule_name, *script_timeout, trigger_event, ctx).await {
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

async fn set_targets(ctx: &RuleContext<'_>, targets: &crate::rules::ActionTargets, value: &HomieValue) -> Result<()> {
    let mut attempts = 0;
    let mut failures = 0;

    for target in targets.iter() {
        attempts += 1;
        if let Err(err) = ctx.dm.set_command(target, value).await {
            failures += 1;
            log::error!("failed to set target {}: {}", target, err);
        }
    }

    target_failures_result(failures, attempts)
}

async fn set_target_commands(ctx: &RuleContext<'_>, commands: Vec<(&PropertyRef, HomieValue)>) -> Result<()> {
    let mut attempts = 0;
    let mut failures = 0;

    for (target, value) in commands {
        attempts += 1;
        if let Err(err) = ctx.dm.set_command(target, &value).await {
            failures += 1;
            log::error!("failed to set target {}: {}", target, err);
        }
    }

    target_failures_result(failures, attempts)
}

fn target_failures_result(failures: usize, attempts: usize) -> Result<()> {
    if failures == 0 {
        Ok(())
    } else {
        Err(eyre!("failed to set {} of {} target(s)", failures, attempts))
    }
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

/// Resolves the effective script execution budget: a per-action
/// `script_timeout` on the `run` action overrides the global
/// `lua_script_timeout` default.
fn effective_script_budget(per_action: Option<Duration>, global_default: Duration) -> Duration {
    per_action.unwrap_or(global_default)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_script(
    script: &str,
    rule_hash: ConfigItemHash,
    rule_name: &str,
    script_timeout: Option<Duration>,
    trigger_event: &RuleTriggerEvent<'_>,
    ctx: &RuleContext<'_>,
) -> mlua::Result<()> {
    log::trace!("preparing script run");
    let budget = effective_script_budget(script_timeout, ctx.lua_script_timeout);
    let lua = Lua::new();

    let lua_homie = LuaHomie { dm: ctx.dm.clone() };
    let lua_virtual_device = LuaVirtualDevice {
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

    // run the script with a bounded execution budget
    log::trace!("executing script (budget {:?})", budget);
    exec_script_with_budget(&lua, script, budget, rule_name).await?;
    log::trace!("finished executing script");
    Ok(())
}

/// Executes `script` on `lua` with a hard execution budget enforced two ways:
///
/// 1. `tokio::time::timeout` cancels scripts that are blocked on an await
///    point (e.g. `utils.sleep`, HTTP calls) once the budget elapses.
/// 2. An instruction-count hook checks a wall-clock deadline every
///    [`LUA_HOOK_INSTRUCTION_INTERVAL`] VM instructions and errors the script
///    — this covers CPU-bound busy loops (`while true do end`) that never
///    reach an await point and therefore can't be cancelled by the timeout.
///
/// Dropping the timed-out future is safe: each rule script runs on a fresh
/// `Lua` VM that is discarded right after execution, so no shared interpreter
/// state can be left behind in a partially-mutated form.
async fn exec_script_with_budget(lua: &Lua, script: &str, budget: Duration, rule_name: &str) -> mlua::Result<()> {
    let deadline = Instant::now() + budget;
    let hook_rule_name = rule_name.to_owned();
    // NOTE: must be a *global* hook: `exec_async` drives the chunk on a new
    // mlua-created coroutine thread, which a plain `set_hook` (current thread
    // only) would never cover — a CPU-bound busy loop would then hang forever.
    lua.set_global_hook(
        HookTriggers::new().every_nth_instruction(LUA_HOOK_INSTRUCTION_INTERVAL),
        move |_lua, _debug| {
            if Instant::now() >= deadline {
                Err(mlua::Error::RuntimeError(format!(
                    "script for rule '{}' exceeded its execution budget of {:?} (CPU-bound loop cancelled)",
                    hook_rule_name, budget
                )))
            } else {
                Ok(VmState::Continue)
            }
        },
    )?;

    match tokio::time::timeout(budget, lua.load(script).exec_async()).await {
        Ok(result) => result.into_lua_err(),
        Err(_elapsed) => Err(mlua::Error::RuntimeError(format!(
            "script for rule '{}' exceeded its execution budget of {:?} and was cancelled",
            rule_name, budget
        ))),
    }
}

#[cfg(test)]
mod script_timeout_tests {
    use super::*;
    use crate::rules::Rule;

    /// Registers a tokio-backed async sleep in the Lua globals. Stand-in for
    /// `utils.sleep` (whose `LuaUtils` carrier needs a live MQTT client).
    fn lua_with_sleep() -> Lua {
        let lua = Lua::new();
        let sleep = lua
            .create_async_function(|_, ms: u64| async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                Ok(())
            })
            .expect("create sleep fn");
        lua.globals().set("sleep_ms", sleep).expect("set sleep fn");
        lua
    }

    #[tokio::test]
    async fn script_timeout_fast_script_unaffected() {
        let lua = Lua::new();
        exec_script_with_budget(&lua, "local x = 1 + 1", Duration::from_secs(5), "fast-rule")
            .await
            .expect("fast script must run to completion");
    }

    #[tokio::test]
    async fn script_timeout_cancels_async_sleep_at_budget() {
        // Mirrors `utils.sleep(60000)`: blocked on an await point, cancelled
        // by the tokio timeout once the budget elapses.
        let lua = lua_with_sleep();
        let err = exec_script_with_budget(&lua, "sleep_ms(60000)", Duration::from_millis(100), "sleepy-rule")
            .await
            .expect_err("script must be cancelled at budget");
        let msg = err.to_string();
        assert!(msg.contains("sleepy-rule"), "error must name the rule: {msg}");
        assert!(msg.contains("cancelled"), "error must state cancellation: {msg}");
    }

    #[tokio::test]
    async fn script_timeout_cancels_busy_loop_via_instruction_hook() {
        // `while true do end` never reaches an await point, so only the
        // instruction-count hook can stop it.
        let lua = Lua::new();
        let started = Instant::now();
        let err = exec_script_with_budget(&lua, "while true do end", Duration::from_millis(100), "busy-rule")
            .await
            .expect_err("busy loop must be cancelled via hook");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "cancellation must happen near the budget, not hang"
        );
        let msg = err.to_string();
        assert!(msg.contains("busy-rule"), "error must name the rule: {msg}");
        assert!(msg.contains("budget"), "error must mention the budget: {msg}");
    }

    #[tokio::test]
    async fn script_timeout_per_action_override_respected() {
        // Budget resolution: per-action override wins over the global default.
        assert_eq!(effective_script_budget(None, Duration::from_secs(5)), Duration::from_secs(5));
        assert_eq!(
            effective_script_budget(Some(Duration::from_millis(100)), Duration::from_secs(5)),
            Duration::from_millis(100)
        );

        // A sleep the 5s global default tolerates is cancelled under a 100ms
        // per-action override...
        let lua = lua_with_sleep();
        exec_script_with_budget(&lua, "sleep_ms(1000)", Duration::from_millis(100), "override-rule")
            .await
            .expect_err("100ms override must cancel a 1s sleep");

        // ...and allowed again when the budget exceeds the sleep.
        let lua = lua_with_sleep();
        exec_script_with_budget(&lua, "sleep_ms(1000)", Duration::from_secs(5), "override-rule")
            .await
            .expect("5s budget must allow a 1s sleep");
    }

    #[test]
    fn script_timeout_parses_from_run_action_yaml() {
        let action: RuleAction = serde_yaml_ng::from_str(
            r#"
type: run
script: print("hi")
script_timeout: 100ms
"#,
        )
        .expect("run action with script_timeout should deserialize");
        let RuleAction::Run { script_timeout, .. } = action else {
            panic!("expected run action");
        };
        assert_eq!(script_timeout, Some(Duration::from_millis(100)));

        let action: RuleAction = serde_yaml_ng::from_str(
            r#"
type: run
script: print("hi")
"#,
        )
        .expect("run action without script_timeout should deserialize");
        let RuleAction::Run { script_timeout, .. } = action else {
            panic!("expected run action");
        };
        assert_eq!(script_timeout, None);

        // ...and still works embedded in a full rule.
        let rule: Rule = serde_yaml_ng::from_str(
            r#"
name: test
triggers:
  - timer_id: t1
actions:
  - type: run
    script: print("hi")
    script_timeout: 2s
"#,
        )
        .expect("rule should deserialize");
        let RuleAction::Run { script_timeout, .. } = &rule.actions[0] else {
            panic!("expected run action");
        };
        assert_eq!(*script_timeout, Some(Duration::from_secs(2)));
    }
}
