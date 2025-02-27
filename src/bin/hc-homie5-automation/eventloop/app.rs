use std::time::Duration;

use color_eyre::eyre::Result;

use hc_homie5::HomieDevice;
use hc_homie5::HomieDeviceCore;
use hc_homie5_automation::app_state::{AppEvent, AppState};

pub async fn handle_app_event(event: AppEvent, state: &mut AppState) -> Result<bool> {
    match event {
        AppEvent::RecalculateVirtualPropertyValue(prop) => {
            state.vdm.update_value(&prop).await?;
        }
        AppEvent::CancelPropertyValueReadFromMqtt(prop) => {
            state.vdm.cancel_mqtt_read(&prop).await?;
        }
        AppEvent::UpdateVirtualDevicesQueries(device_ref) => {
            let vdevices = state.vdm.read().await;
            if let Some(vdev) = vdevices.get(&device_ref) {
                state
                    .rules
                    .queries_virtual_device_updated(&device_ref, vdev.description());
            }
        }
        AppEvent::Exit => {
            // Stop configuration watchers
            state.rule_watcher_handle.stop().await?;
            state.virtual_devices_watcher_handle.stop().await?;

            // stop discovery and send disconnect signal for all devices
            state.dm.stop_discover().await?;
            state.vdm.disconnect_virtual_devices().await?;
            state.vdm.disconnect_device().await?;

            // wait a second to give mqtt the change to publish the disconnect states properly
            // TODO: Find a solution that does not rely on some arbitrary number of seconds of wait
            // time
            tokio::time::sleep(Duration::from_secs(1)).await;
            // disconnect all the clients
            state.dm.disconnect_client().await?;
            state.vdm.disconnect_client().await?;
            state.mqtt_client.disconnect().await?;

            // clear discoved devices
            let mut devices = state.dm.write().await;
            devices.clear();

            // clear active times and crons
            state.timers.clear();
            state.cron.clear();

            // exit
            state.should_exit = true;
        }
    }
    Ok(false)
}
