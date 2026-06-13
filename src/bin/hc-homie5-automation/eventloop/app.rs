use std::time::Duration;

use color_eyre::eyre::Result;

use hc_homie5::device::{HomieDevice, HomieDeviceCore};
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
            state.lua_files_watcher_handle.stop().await?;
            state.meta_watcher_handle.stop().await?;

            // stop discovery and send disconnect signal for all devices
            state.dm.stop_discover().await?;
            state.vdm.disconnect_virtual_devices().await?;
            state.vdm.disconnect_device().await?;

            // Deterministic shutdown: wait until the broker has acknowledged
            // all in-flight publishes (incl. the `$state = disconnected`
            // messages above) on all three connections before disconnecting.
            // After a clean DISCONNECT the broker discards the LWT, so the
            // ack guarantee is what keeps retained `$state` from ending up
            // as `lost`. A flush returns immediately when nothing is
            // pending; FLUSH_MAX_WAIT only bounds the dead-broker case — in
            // that case we log and proceed so shutdown can never hang.
            const FLUSH_MAX_WAIT: Duration = Duration::from_secs(5);
            if let Err(err) = state.vdm.flush(FLUSH_MAX_WAIT).await {
                log::warn!("Shutdown: flushing virtual device connection failed: {err}");
            }
            if let Err(err) = state.dm.flush(FLUSH_MAX_WAIT).await {
                log::warn!("Shutdown: flushing discovery/controller connection failed: {err}");
            }
            if let Err(err) = state.mqtt_client.flush(FLUSH_MAX_WAIT).await {
                log::warn!("Shutdown: flushing mqtt client connection failed: {err}");
            }

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
            // All publishes are flushed and all clients cleanly disconnected
            // above — there is nothing left worth draining from the event
            // channels, so request immediate loop exit instead of waiting for
            // the multiplexer's drain timeout.
            state.should_exit = true;
            return Ok(true);
        }
    }
    Ok(false)
}
