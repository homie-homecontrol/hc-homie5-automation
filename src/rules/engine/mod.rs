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

use crate::{
    device_manager::DeviceManager, mqtt_client::ManagedMqttClient, rule_manager::RuleManager,
    timer_manager::TimerManager, virtual_devices::VirtualDeviceManager,
};

pub struct RuleContext<'a> {
    pub rules: &'a RuleManager,
    pub timers: &'a TimerManager,
    pub dm: &'a DeviceManager,
    pub vdm: &'a VirtualDeviceManager,
    pub mqtt_client: &'a ManagedMqttClient,
    pub value_store: &'a KeyValueStore,
}
