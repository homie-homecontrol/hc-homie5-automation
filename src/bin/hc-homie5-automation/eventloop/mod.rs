use color_eyre::eyre::Result;
use config_watcher::config_item_watcher::ConfigItemEvent;
use cron::handle_cron_event;
use discovery::handle_discovery_client_event;
use hc_homie5::{define_event_multiplexer, HomieClientEvent};
use hc_homie5_automation::{
    app_state::{AppEvent, AppState},
    cron_manager::CronEvent,
    mqtt_client::MqttClientEvent,
    rules::Rule,
    solar_events::SolarEvent,
    timer_manager::TimerEvent,
    virtual_devices::VirtualDeviceSpec,
};
use lua_files::handle_lua_files_changes_event;
use mqtt_client::handle_mqtt_client_event;
use rules::handle_rules_changes_event;
use solar::handle_solar_event;
use timers::handle_timer_event;
use virtual_devices::{handle_virtual_devices_changes_event, handle_virtual_devices_client_event};

mod app;
mod cron;
mod discovery;
mod lua_files;
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
        let timeout = if state.should_exit { 1 } else { 60 };
        let exit = match event_multiplexer.next(timeout).await {
            Event::App(app_event) => handle_app_event(app_event, state).await?,
            Event::DiscoveryClient(homie_client_event) => {
                handle_discovery_client_event(homie_client_event, state).await?
            }
            Event::RulesChanges(config_file_event) => handle_rules_changes_event(config_file_event, state).await?,
            Event::VirtualDevicesChanges(config_file_event) => {
                handle_virtual_devices_changes_event(config_file_event, state).await?
            }
            Event::VirtualDevicesClient(homie_client_event) => {
                handle_virtual_devices_client_event(homie_client_event, state).await?
            }
            Event::LuaFilesChanges(config_file_event) => {
                handle_lua_files_changes_event(config_file_event, state).await?
            }
            Event::TimerEvent(timer_event) => handle_timer_event(timer_event, state).await?,
            Event::CronEvent(cron_event) => handle_cron_event(cron_event, state).await?,
            Event::MqttClientEvent(mqtt_event) => handle_mqtt_client_event(mqtt_event, state).await?,
            Event::SolarEvent(solar_event) => handle_solar_event(solar_event, state).await?,
            Event::Timeout => state.should_exit,
            Event::None => false,
        };

        if exit {
            break;
        }
    }
    log::debug!("Exiting application event loop");
    Ok(())
}
