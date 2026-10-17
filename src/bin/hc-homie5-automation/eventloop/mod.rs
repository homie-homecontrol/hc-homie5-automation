use std::time::Duration;

use color_eyre::eyre::Result;
use config_watcher::config_item_watcher::ConfigItemEvent;
use cron::handle_cron_event;
use discovery::handle_discovery_client_event;
use hc_homie5::client::HomieClientEvent;
use hc_homie5::define_event_multiplexer;
use hc_homie5::logging::{chain, Outage};
use hc_homie5_automation::{
    app_state::{AppEvent, AppState},
    cron_manager::CronEvent,
    meta::MetaConfig,
    mqtt_client::MqttClientEvent,
    panic_guard::contain,
    rules::Rule,
    solar_events::SolarEvent,
    timer_manager::TimerEvent,
    virtual_devices::VirtualDeviceSpec,
};
use lua_files::handle_lua_files_changes_event;
use meta::handle_meta_changes_event;
use mqtt_client::handle_mqtt_client_event;
use rules::handle_rules_changes_event;
use solar::handle_solar_event;
use timers::handle_timer_event;
use virtual_devices::{handle_virtual_devices_changes_event, handle_virtual_devices_client_event};

use crate::settings::SETTINGS;

mod app;
mod cron;
mod discovery;
mod lua_files;
mod meta;
mod mqtt_client;
mod rules;
mod solar;
mod timers;
mod virtual_devices;

pub use app::*;

define_event_multiplexer! {
    #[derive(Debug)]
    pub enum Event {
        App(AppEvent) => app,
        DiscoveryClient(HomieClientEvent) => discovery_client,
        VirtualDevicesClient(HomieClientEvent) => virtual_devices,
        RulesChanges(ConfigItemEvent<Rule>) => rules_changes,
        VirtualDevicesChanges(ConfigItemEvent<VirtualDeviceSpec>) => vdevice_changes,
        LuaFilesChanges(ConfigItemEvent<String>) => lua_changes,
        MetaChanges(ConfigItemEvent<MetaConfig>) => meta_changes,
        TimerEvent(TimerEvent) => timer_event,
        CronEvent(CronEvent) => cron_event,
        MqttClientEvent(MqttClientEvent) => mqtt_client_event,
        SolarEvent(SolarEvent) => solar_event,
    }
}

pub async fn run_event_loop(event_multiplexer: &mut EventMultiPlexer, state: &mut AppState) -> Result<()> {
    loop {
        // timeout is usually 60s, except if we want to exit, we set it to one second, so the
        // application exits as soon as all events are done processing
        let timeout = if state.should_exit {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(60)
        };
        let event = event_multiplexer.next(timeout).await;
        let is_exit_event = matches!(event, Event::App(AppEvent::Exit));
        let source = event_source(&event);

        // A failing or panicking handler must not stop the controller: log it and continue with
        // the next event. Only the exit event ends the loop, even if its cleanup fails.
        let exit = match contain(handle_event(event, state)).await {
            Ok(Ok(exit)) => exit,
            Ok(Err(err)) => {
                tracing::error!(event = source, error = chain(err.as_ref()), "event handling failed");
                is_exit_event
            }
            Err(panic) => {
                tracing::error!(event = source, error = chain(&panic), "event handling panicked");
                is_exit_event
            }
        };

        if exit {
            break;
        }
    }
    tracing::debug!("event loop stopped");
    Ok(())
}

/// One of the controller's MQTT connections, for connection logs.
#[derive(Clone, Copy)]
pub(crate) enum MqttConnection {
    Discovery,
    VirtualDevices,
    Mqtt,
}

impl MqttConnection {
    fn name(self) -> &'static str {
        match self {
            MqttConnection::Discovery => "discovery",
            MqttConnection::VirtualDevices => "virtual-devices",
            MqttConnection::Mqtt => "mqtt",
        }
    }

    fn client_id(self) -> String {
        let client_id = &SETTINGS.homie.client_id;
        match self {
            MqttConnection::Discovery => client_id.clone(),
            MqttConnection::VirtualDevices => format!("{client_id}-ctrl"),
            MqttConnection::Mqtt => format!("{client_id}-mqtt"),
        }
    }

    /// Logs an established connection; INFO with the outage duration when it ends an outage.
    pub(crate) fn connected(self, outage: &mut Outage) {
        let broker = broker();
        let client_id = self.client_id();
        match outage.recover() {
            Some(down) => tracing::info!(
                connection = self.name(),
                client_id = %client_id,
                broker = %broker,
                outage_secs = down.as_secs(),
                "MQTT connection restored"
            ),
            None => {
                tracing::info!(connection = self.name(), client_id = %client_id, broker = %broker, "MQTT connected")
            }
        }
    }

    /// Logs a failed connection attempt: WARN when the outage starts, DEBUG for every retry.
    pub(crate) fn failed(self, outage: &mut Outage, err: &(dyn std::error::Error + 'static)) {
        if outage.fail() {
            tracing::warn!(
                connection = self.name(),
                client_id = %self.client_id(),
                broker = %broker(),
                error = chain(err),
                "MQTT broker unreachable"
            );
        } else {
            tracing::debug!(
                connection = self.name(),
                attempt = outage.failures(),
                error = chain(err),
                "MQTT reconnect failed"
            );
        }
    }

    /// Logs a client task that ended outside of a shutdown; the connection is not retried.
    pub(crate) fn stopped(self) {
        tracing::error!(connection = self.name(), "MQTT client stopped");
    }
}

fn broker() -> String {
    format!("{}:{}", SETTINGS.homie.hostname, SETTINGS.homie.port)
}

async fn handle_event(event: Event, state: &mut AppState) -> Result<bool> {
    Ok(match event {
        Event::App(app_event) => handle_app_event(app_event, state).await?,
        Event::DiscoveryClient(homie_client_event) => handle_discovery_client_event(homie_client_event, state).await?,
        Event::RulesChanges(config_file_event) => handle_rules_changes_event(config_file_event, state).await?,
        Event::VirtualDevicesChanges(config_file_event) => {
            handle_virtual_devices_changes_event(config_file_event, state).await?
        }
        Event::VirtualDevicesClient(homie_client_event) => {
            handle_virtual_devices_client_event(homie_client_event, state).await?
        }
        Event::LuaFilesChanges(config_file_event) => handle_lua_files_changes_event(config_file_event, state).await?,
        Event::MetaChanges(config_file_event) => handle_meta_changes_event(config_file_event, state).await?,
        Event::TimerEvent(timer_event) => handle_timer_event(timer_event, state).await?,
        Event::CronEvent(cron_event) => handle_cron_event(cron_event, state).await?,
        Event::MqttClientEvent(mqtt_event) => handle_mqtt_client_event(mqtt_event, state).await?,
        Event::SolarEvent(solar_event) => handle_solar_event(solar_event, state).await?,
        Event::Timeout => state.should_exit,
        Event::None => false,
    })
}

fn event_source(event: &Event) -> &'static str {
    match event {
        Event::App(_) => "app",
        Event::DiscoveryClient(_) => "discovery client",
        Event::VirtualDevicesClient(_) => "virtual devices client",
        Event::RulesChanges(_) => "rules config",
        Event::VirtualDevicesChanges(_) => "virtual devices config",
        Event::LuaFilesChanges(_) => "Lua modules config",
        Event::MetaChanges(_) => "meta config",
        Event::TimerEvent(_) => "timer",
        Event::CronEvent(_) => "cron",
        Event::MqttClientEvent(_) => "MQTT client",
        Event::SolarEvent(_) => "solar",
        Event::Timeout => "timeout",
        Event::None => "empty",
    }
}
