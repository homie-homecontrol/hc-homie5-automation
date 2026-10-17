// modules
mod action;
mod cron;
mod mqtt;
mod properties;
mod queries;
mod solar;
mod timer;
mod virtual_devices;
mod while_condition;

// re-exports
pub use action::*;
pub use cron::*;
pub use mqtt::*;
pub use properties::*;
pub use queries::*;
use simple_kv_store::KeyValueStore;
pub use solar::*;
pub use timer::*;
pub use virtual_devices::*;

use config_watcher::ConfigItemHash;
use hc_homie5::store::DeviceStore;

use crate::{
    device_manager::DeviceManager,
    lua_runtime::LuaModuleManager,
    mqtt_client::ManagedMqttClient,
    rule_manager::RuleManager,
    rules::{Rule, RuleTrigger},
    timer_manager::TimerManager,
    virtual_devices::VirtualDeviceManager,
};

/// Rules with at least one trigger for which `matches` holds against `devices`.
///
/// All triggers and while-conditions are evaluated before this returns, so callers drop the
/// device-store guard before running actions while every rule still sees the same snapshot.
fn matching_rules<'r>(
    rules: &'r RuleManager,
    devices: &DeviceStore,
    matches: impl Fn(&RuleTrigger, &DeviceStore) -> bool,
) -> Vec<(ConfigItemHash, &'r Rule)> {
    rules
        .iter()
        .filter(|(_, rule)| rule.triggers.iter().any(|trigger| matches(trigger, devices)))
        .map(|(hash, rule)| (*hash, rule))
        .collect()
}

pub struct RuleContext<'a> {
    pub rules: &'a RuleManager,
    pub timers: &'a TimerManager,
    pub dm: &'a DeviceManager,
    pub vdm: &'a VirtualDeviceManager,
    pub mqtt_client: &'a ManagedMqttClient,
    pub value_store: &'a KeyValueStore,
    pub lmm: &'a LuaModuleManager,
    /// Global default execution budget for Lua `run` script actions.
    /// A per-action `script_timeout` on the `run` action overrides this.
    pub lua_script_timeout: std::time::Duration,
    /// Memory limit in bytes for each Lua script VM; `0` disables it.
    pub lua_memory_limit: usize,
}
