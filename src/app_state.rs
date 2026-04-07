use config_watcher::ConfigItemWatcherHandle;
pub use hc_homie5::connection::{ConnectionEvent, ConnectionState};
use hc_homie5::controller::MetaOverlayHandler;
use homie5::{DeviceRef, PropertyRef};
use simple_kv_store::KeyValueStore;
use tokio::sync::mpsc::Sender;

use crate::{
    cron_manager::CronManager, device_manager::DeviceManager, lua_runtime::LuaModuleManager, meta::MetaManager,
    mqtt_client::ManagedMqttClient, rule_manager::RuleManager, rules::RuleContext, solar_events::SolarEventManager,
    timer_manager::TimerManager, virtual_devices::VirtualDeviceManager,
};

#[derive(Debug)]
pub enum AppEvent {
    // Tasks
    RecalculateVirtualPropertyValue(PropertyRef),
    CancelPropertyValueReadFromMqtt(PropertyRef),
    UpdateVirtualDevicesQueries(DeviceRef),

    Exit,
}
pub struct AppState {
    pub dm: DeviceManager,
    pub rules: RuleManager,
    pub vdm: VirtualDeviceManager,
    pub timers: TimerManager,
    pub solar_events: SolarEventManager,
    pub cron: CronManager,
    pub mqtt_client: ManagedMqttClient,
    pub app_event_sender: Sender<AppEvent>,
    pub should_exit: bool,
    pub mqtt_state: ConnectionState,
    pub discovery_state: ConnectionState,
    pub virtual_devices_state: ConnectionState,
    pub value_store: KeyValueStore,
    pub lua_module_manager: LuaModuleManager,
    pub meta: MetaManager,
    pub meta_handler: MetaOverlayHandler,
    pub rule_watcher_handle: ConfigItemWatcherHandle,
    pub virtual_devices_watcher_handle: ConfigItemWatcherHandle,
    pub meta_watcher_handle: ConfigItemWatcherHandle,
    pub lua_files_watcher_handle: ConfigItemWatcherHandle,
}

impl AppState {
    pub fn as_rule_ctx(&self) -> RuleContext<'_> {
        RuleContext {
            rules: &self.rules,
            timers: &self.timers,
            dm: &self.dm,
            vdm: &self.vdm,
            mqtt_client: &self.mqtt_client,
            value_store: &self.value_store,
            lmm: &self.lua_module_manager,
        }
    }

    pub async fn start_watchers(&self) {
        if let (ConnectionState::Connected, ConnectionState::Connected, ConnectionState::Connected) =
            (self.discovery_state, self.mqtt_state, self.virtual_devices_state)
        {
            match self.rule_watcher_handle.start().await {
                Ok(_) => {
                    log::debug!("Started rule config watcher");
                }
                Err(e) => {
                    log::error!("Error starting rule config watcher. {e:?}");
                }
            }
            match self.virtual_devices_watcher_handle.start().await {
                Ok(_) => {
                    log::debug!("Started virtual devices config watcher");
                }
                Err(e) => {
                    log::error!("Error starting virtual devices config watcher. {:?}", e);
                }
            }
            match self.lua_files_watcher_handle.start().await {
                Ok(_) => {
                    log::debug!("Started lua module config watcher");
                }
                Err(e) => {
                    log::error!("Error starting lua module config watcher. {:?}", e);
                }
            }

            match self.meta_watcher_handle.start().await {
                Ok(_) => {
                    log::debug!("Started meta config watcher");
                }
                Err(e) => {
                    log::error!("Error starting meta config watcher. {:?}", e);
                }
            }
        }
    }
}
