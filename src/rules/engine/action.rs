use super::{FiringDecision, RuleContext, FIRING_WINDOW};
use crate::lua_runtime::{
    install_print, new_sandboxed_lua, setup_custom_loader, LuaEvent, LuaHomie, LuaTimer, LuaUtils, LuaValueStore,
    LuaVirtualDevice,
};
use crate::panic_guard::contain;
use crate::rules::{MapSetFrom, RuleAction, TimerDef};
use crate::solar_events::SolarEvent;
use crate::{
    rules::{Rule, RuleTriggerEvent},
    timer_manager::TimerManager,
};
use color_eyre::eyre::{eyre, Result};
use config_watcher::ConfigItemHash;
use hc_homie5::client::HomieMQTTClient;
use hc_homie5::logging::{chain, truncate, PAYLOAD_MAX};
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
    if let FiringDecision::Limited { first_in_window } = ctx.rules.check_firing(rule_hash, Instant::now()) {
        if first_in_window {
            tracing::warn!(
                rule = %rule.name,
                trigger = trigger_event.trigger_type(),
                window_ms = FIRING_WINDOW.as_millis() as u64,
                "rule fires too often; skipping further firings in this window"
            );
        }
        return;
    }
    let filename = ctx
        .rules
        .get_filename(rule_hash)
        .map(|f| f.to_owned())
        .unwrap_or_default();

    let prop = trigger_event.property_ref();
    tracing::info!(
        rule = %rule.name,
        file = %filename,
        trigger = trigger_event.trigger_type(),
        device = prop.map(|p| tracing::field::display(p.device_id())),
        node = prop.map(|p| tracing::field::display(p.node_id())),
        property = prop.map(|p| tracing::field::display(p.prop_id())),
        value = trigger_value(&trigger_event),
        timer = trigger_event.timer_id(),
        topic = trigger_event.mqtt_topic(),
        "rule fired"
    );
    for (index, action) in rule.actions.iter().enumerate() {
        tracing::debug!(rule = %rule.name, action = index, "rule action started");
        let started = Instant::now();
        match contain(execute_rule_action(rule_hash, &rule.name, action, &trigger_event, ctx, false)).await {
            Ok(Ok(_)) => {
                tracing::debug!(rule = %rule.name, action = index, "rule action finished");
            }
            Ok(Err(err)) => {
                tracing::error!(rule = %rule.name, action = index, error = chain(err.as_ref()), "rule action failed");
            }
            Err(panic) => {
                tracing::error!(rule = %rule.name, action = index, error = chain(&panic), "rule action panicked");
            }
        }
        let elapsed = started.elapsed();
        if elapsed > SLOW_ACTION_WARN_THRESHOLD {
            // Actions run inline and delay all further event processing.
            tracing::warn!(
                rule = %rule.name,
                action = index,
                duration_ms = elapsed.as_millis() as u64,
                threshold_ms = SLOW_ACTION_WARN_THRESHOLD.as_millis() as u64,
                "slow rule action"
            );
        }
    }
    tracing::debug!(rule = %rule.name, "rule finished");
}

/// The value that fired a trigger, for the `rule fired` line.
fn trigger_value(event: &RuleTriggerEvent<'_>) -> Option<String> {
    match event {
        RuleTriggerEvent::PropertyChanged { to, .. } => Some(truncate(&to.to_string(), PAYLOAD_MAX)),
        RuleTriggerEvent::PropertyTriggered { value, .. } => Some(truncate(&value.to_string(), PAYLOAD_MAX)),
        RuleTriggerEvent::OnSet { value, .. } => Some(truncate(value, PAYLOAD_MAX)),
        RuleTriggerEvent::Mqtt(event) => Some(truncate(&event.payload, PAYLOAD_MAX)),
        RuleTriggerEvent::Solar(event) => Some(format!("{:?}", event)),
        RuleTriggerEvent::Timer(_) | RuleTriggerEvent::Cron(_) => None,
    }
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
                set_targets(ctx, rule_name, target, value).await?;
            } else if let Some(timer) = timer {
                // Handle the timer if it exists
                handle_timer(rule_hash, rule_name, Some(action.clone()), trigger_event, timer, ctx.timers).await?;
            }
        }
        RuleAction::MapSet { target, mapping, timer } => {
            if ignore_timer || timer.is_none() {
                let from = match trigger_event {
                    RuleTriggerEvent::PropertyChanged { to, .. } => MapSetFrom::HomieValue(Cow::Borrowed(to)),
                    RuleTriggerEvent::PropertyTriggered { value, .. } => MapSetFrom::HomieValue(Cow::Borrowed(value)),
                    RuleTriggerEvent::OnSet { value, .. } => MapSetFrom::String(Cow::Borrowed(value)),
                    RuleTriggerEvent::Timer(cow) => MapSetFrom::String(Cow::Borrowed(&cow.id)),
                    RuleTriggerEvent::Mqtt(cow) => MapSetFrom::String(Cow::Borrowed(&cow.payload)),
                    RuleTriggerEvent::Solar(cow) => match cow.as_ref() {
                        SolarEvent::At(solar_phase)
                        | SolarEvent::After(solar_phase, _)
                        | SolarEvent::Before(solar_phase, _) => MapSetFrom::SolarPhase(Cow::Borrowed(solar_phase)),
                    },
                    _ => return Ok(()),
                };
                match mapping.map_to(&from) {
                    MappingResult::Mapped(value) => set_targets(ctx, rule_name, target, value).await?,
                    MappingResult::Unmapped(_) => {
                        tracing::debug!(rule = %rule_name, "map_set has no mapping for the trigger value; nothing sent");
                    }
                }
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
                            _ => {
                                tracing::debug!(
                                    rule = %rule_name,
                                    device = %target.device_id(),
                                    node = %target.node_id(),
                                    property = %target.prop_id(),
                                    "toggle target has no boolean value; skipped"
                                );
                                None
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            };

            set_target_commands(ctx, rule_name, commands).await?;
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
            tracing::info!(
                rule = %rule_name,
                topic = %topic,
                payload = truncate(value, PAYLOAD_MAX),
                retain = *retain,
                source = "rule",
                "message published"
            );
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
                tracing::debug!(rule = %rule_name, "script started");
                match run_script(script, rule_hash, rule_name, *script_timeout, trigger_event, ctx).await {
                    Ok(_) => {
                        tracing::debug!(rule = %rule_name, "script finished");
                    }
                    Err(err) => {
                        tracing::error!(rule = %rule_name, error = chain(&err), "script failed")
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

async fn set_targets(
    ctx: &RuleContext<'_>,
    rule_name: &str,
    targets: &crate::rules::ActionTargets,
    value: &HomieValue,
) -> Result<()> {
    let mut attempts = 0;
    let mut failures = 0;

    for target in targets.iter() {
        attempts += 1;
        if !send_command(ctx, rule_name, target, value).await {
            failures += 1;
        }
    }

    target_failures_result(failures, attempts)
}

async fn set_target_commands(
    ctx: &RuleContext<'_>,
    rule_name: &str,
    commands: Vec<(&PropertyRef, HomieValue)>,
) -> Result<()> {
    let mut attempts = 0;
    let mut failures = 0;

    for (target, value) in commands {
        attempts += 1;
        if !send_command(ctx, rule_name, target, &value).await {
            failures += 1;
        }
    }

    target_failures_result(failures, attempts)
}

/// Sends one `/set` command of a rule action and logs it; returns false if it failed.
async fn send_command(ctx: &RuleContext<'_>, rule_name: &str, target: &PropertyRef, value: &HomieValue) -> bool {
    match ctx.dm.set_command(target, value).await {
        Ok(()) => {
            tracing::info!(
                rule = %rule_name,
                device = %target.device_id(),
                node = %target.node_id(),
                property = %target.prop_id(),
                value = truncate(&value.to_string(), PAYLOAD_MAX),
                source = "rule",
                "command sent"
            );
            true
        }
        Err(err) => {
            tracing::error!(
                rule = %rule_name,
                device = %target.device_id(),
                node = %target.node_id(),
                property = %target.prop_id(),
                value = truncate(&value.to_string(), PAYLOAD_MAX),
                error = chain(&err),
                "command failed"
            );
            false
        }
    }
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
                tracing::debug!(rule = %rule_name, timer = %timer_id, "timer cancelled by cancel condition");
                cancelled = true;
            }
        }
    }

    if !cancelled {
        tracing::debug!(
            rule = %rule_name,
            timer = %timer_id,
            delay_ms = timer_def.duration.as_millis() as u64,
            repeat_ms = timer_def.repeat.map(|repeat| repeat.as_millis() as u64),
            delayed_action = action.is_some(),
            "timer scheduled"
        );
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
    let budget = effective_script_budget(script_timeout, ctx.lua_script_timeout);
    let lua = new_sandboxed_lua(ctx.lua_memory_limit)?;
    install_print(&lua, rule_name)?;

    let lua_homie = LuaHomie {
        dm: ctx.dm.clone(),
        rule: rule_name.to_owned(),
    };
    let lua_virtual_device = LuaVirtualDevice {
        vdm: ctx.vdm.as_proxy(),
        rule: rule_name.to_owned(),
    };

    let lua_timer = LuaTimer {
        timers: ctx.timers.clone(),
        rule_hash,
        rule: rule_name.to_owned(),
    };

    let lua_value_store = LuaValueStore {
        store: ctx.value_store.clone(),
    };

    let lua_event = LuaEvent {
        event: trigger_event.to_owned(),
    };

    let lua_utils = LuaUtils {
        mqtt_client: ctx.mqtt_client.clone(),
        rule: rule_name.to_owned(),
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
    tracing::trace!(rule = %rule_name, budget_ms = budget.as_millis() as u64, "executing script");
    exec_script_with_budget(&lua, script, budget, rule_name).await?;
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
        assert!(started.elapsed() < Duration::from_secs(10), "cancellation must happen near the budget, not hang");
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

#[cfg(test)]
mod script_containment_tests {
    use super::*;
    use crate::lua_runtime::{LuaHomieValue, DEFAULT_LUA_MEMORY_LIMIT};

    /// A sandboxed VM with `explode()`, a Rust callback that panics like a failing HTTP client
    /// build did in hc-homie5-automation 0.4.0.
    fn lua_with_panicking_callback() -> Lua {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).expect("sandboxed lua");
        let explode = lua
            .create_async_function(|_, ()| async move {
                tokio::task::yield_now().await;
                if true {
                    panic!("callback exploded");
                }
                Ok(())
            })
            .expect("create explode fn");
        lua.globals().set("explode", explode).expect("set explode fn");
        lua
    }

    #[tokio::test]
    async fn sandboxed_script_cannot_exit_the_process() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        let err = exec_script_with_budget(&lua, "os.exit(1)", Duration::from_secs(5), "exit-rule")
            .await
            .expect_err("os.exit must not be callable");
        assert!(err.to_string().contains("exit"), "{err}");
        let err = exec_script_with_budget(&lua, "io.popen('kill 1')", Duration::from_secs(5), "popen-rule")
            .await
            .expect_err("io must not be available");
        assert!(err.to_string().contains("io"), "{err}");
    }

    #[tokio::test]
    async fn panicking_callback_is_contained_without_pcall() {
        let lua = lua_with_panicking_callback();
        let result = contain(exec_script_with_budget(&lua, "explode()", Duration::from_secs(5), "boom-rule")).await;
        let failed = match result {
            Err(panic) => panic.message.contains("callback exploded"),
            Ok(Err(err)) => err.to_string().contains("callback exploded"),
            Ok(Ok(())) => false,
        };
        assert!(failed, "the panic must surface as a contained failure");
    }

    #[tokio::test]
    async fn panicking_callback_is_catchable_with_pcall() {
        let lua = lua_with_panicking_callback();
        let result = contain(exec_script_with_budget(
            &lua,
            "local ok, err = pcall(explode); assert(not ok); assert(tostring(err):find('callback exploded'))",
            Duration::from_secs(5),
            "pcall-rule",
        ))
        .await;
        assert!(matches!(result, Ok(Ok(()))), "pcall must catch the panic: {result:?}");
    }

    #[tokio::test]
    async fn runaway_allocation_fails_the_script_only() {
        let lua = new_sandboxed_lua(8 * 1024 * 1024).unwrap();
        let err = exec_script_with_budget(
            &lua,
            "local t = {}; while true do t[#t + 1] = string.rep('x', 1024) end",
            Duration::from_secs(5),
            "alloc-rule",
        )
        .await
        .expect_err("allocation must hit the memory limit");
        assert!(err.to_string().contains("not enough memory"), "{err}");
    }

    #[test]
    fn deeply_nested_table_is_rejected_as_homie_value() {
        let lua = new_sandboxed_lua(DEFAULT_LUA_MEMORY_LIMIT).unwrap();
        let table: mlua::Value = lua
            .load("local t = {}; local c = t; for i = 1, 100000 do c.x = {}; c = c.x end; return t")
            .eval()
            .unwrap();
        let err = <LuaHomieValue as mlua::FromLua>::from_lua(table, &lua).expect_err("nesting limit");
        assert!(err.to_string().contains("nesting"), "{err}");
        let flat: mlua::Value = lua.load("return { a = 1, b = { c = 'x' } }").eval().unwrap();
        assert!(<LuaHomieValue as mlua::FromLua>::from_lua(flat, &lua).is_ok());
    }
}
