use config_watcher::ConfigItemWatcherHandle;
pub use hc_homie5::connection::{ConnectionEvent, ConnectionState};
use hc_homie5::controller::MetaOverlayHandler;
use hc_homie5::logging::{chain, Outage};
use hc_homie5::util::DebouncedSender;
use homie5::{DeviceRef, PropertyRef};
use simple_kv_store::KeyValueStore;
use tokio::sync::mpsc::Sender;

use crate::{
    config_item::ItemHash, cron_manager::CronManager, device_manager::DeviceManager, lua_runtime::LuaModuleManager,
    meta::MetaManager, mqtt_client::ManagedMqttClient, rule_manager::RuleManager, rules::RuleContext,
    solar_events::SolarEventManager, templates::TemplateStage, timer_manager::TimerManager,
    virtual_devices::VirtualDeviceManager,
};

#[derive(Debug, Clone)]
pub enum AppEvent {
    // Tasks
    RecalculateVirtualPropertyValue(PropertyRef),
    CancelPropertyValueReadFromMqtt(PropertyRef),
    UpdateVirtualDevicesQueries(DeviceRef),
    /// Publish a pending (debounced) children update of the controller
    /// device description (see [`VirtualDeviceManager::flush_children`]).
    FlushControllerChildren,
    /// Rebuild a materialized group device after member discovery changes
    /// (debounced per group, see [`VirtualDeviceManager::rebuild_group_device`]).
    RebuildGroupDevice(ItemHash),
    /// Re-expand template-using rule and virtual-device documents after a template, source
    /// document, device or `$meta` change (debounced, see [`AppState::schedule_template_expansion`]).
    ExpandTemplates,

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
    /// Set when the controller shuts down because of a failure; the process then exits non-zero.
    pub exit_error: Option<String>,
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
    /// Global default execution budget for Lua `run` script actions
    /// (a per-action `script_timeout` on the `run` action overrides it).
    pub lua_script_timeout: std::time::Duration,
    /// Memory limit in bytes for each Lua script VM; `0` disables it.
    pub lua_memory_limit: usize,
    /// Broker outages of the discovery, virtual devices, and generic MQTT connections.
    pub discovery_outage: Outage,
    pub virtual_devices_outage: Outage,
    pub mqtt_outage: Outage,
    /// Set once the config watchers were started for the first time.
    pub watchers_started: bool,
    /// Templates and template-using documents of the rules config.
    pub rule_templates: TemplateStage,
    /// Templates and template-using documents of the virtual-devices config.
    pub vdevice_templates: TemplateStage,
    /// Debouncer emitting [`AppEvent::ExpandTemplates`].
    pub template_expansion: DebouncedSender<AppEvent>,
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
            lua_script_timeout: self.lua_script_timeout,
            lua_memory_limit: self.lua_memory_limit,
        }
    }

    /// Requests a re-expansion of all template-using documents; bursts (a file with many
    /// documents, a template edited as remove + add) collapse into one expansion.
    pub async fn schedule_template_expansion(&self) {
        self.template_expansion.send(AppEvent::ExpandTemplates).await;
    }

    /// Requests a re-expansion after a device appeared, changed its description or `$meta`, or
    /// disappeared, when a `for_each` depends on the devices.
    pub async fn devices_changed_for_templates(&self) {
        if self.rule_templates.uses_devices() || self.vdevice_templates.uses_devices() {
            self.schedule_template_expansion().await;
        }
    }

    pub async fn start_watchers(&mut self) {
        if let (ConnectionState::Connected, ConnectionState::Connected, ConnectionState::Connected) =
            (self.discovery_state, self.mqtt_state, self.virtual_devices_state)
        {
            let watchers = [
                ("rules", &self.rule_watcher_handle),
                ("virtual devices", &self.virtual_devices_watcher_handle),
                ("lua modules", &self.lua_files_watcher_handle),
                ("meta", &self.meta_watcher_handle),
            ];
            for (config, watcher) in watchers {
                if let Err(err) = watcher.start().await {
                    tracing::error!(config, error = chain(&err), "cannot start config watcher");
                }
            }
            if !self.watchers_started {
                self.watchers_started = true;
                tracing::info!("config watchers started");
            }
        }
    }
}
