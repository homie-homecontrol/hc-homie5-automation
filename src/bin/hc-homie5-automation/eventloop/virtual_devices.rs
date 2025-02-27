use color_eyre::eyre::Result;
use config_watcher::ConfigItemEvent;
use hc_homie5::HomieDeviceCore;
use hc_homie5::{HomieClientEvent, HomieDevice};
use hc_homie5_automation::utils::log_homie_message;
use hc_homie5_automation::{
    app_state::AppState, app_state::ConnectionEvent, app_state::ConnectionState, rules::run_on_set_rules,
    virtual_devices::VirtualDeviceSpec,
};

pub async fn handle_virtual_devices_client_event(event: HomieClientEvent, state: &mut AppState) -> Result<bool> {
    match event {
        HomieClientEvent::Connect => {
            log::debug!("Virtual Devices: mqtt connected. Publishing");
            state.vdm.publish_device().await?;

            let con_event = state.virtual_devices_state.change_state(ConnectionState::Connected);
            if let Some(ConnectionEvent::Reconnect) = con_event {
                state.vdm.publish_child_devices().await?;
            }
            state.start_watchers().await;
        }
        HomieClientEvent::Disconnect => {
            log::debug!("Virtual Devices: mqtt disconnected.");
            state.virtual_devices_state.change_state(ConnectionState::Disconnected);
        }
        HomieClientEvent::HomieMessage(event) => {
            log::trace!("Virtual Device: {}", log_homie_message(&event));
            match &event {
                homie5::Homie5Message::PropertySet { property, set_value } => {
                    state.vdm.handle_set_command(property, set_value).await?;
                    run_on_set_rules(&event, &state.as_rule_ctx()).await;
                }
                homie5::Homie5Message::PropertyValue { property, value } => {
                    state.vdm.handle_mqtt_read(property, value).await?;
                }
                _ => (),
            }
        }
        HomieClientEvent::Stop => {
            log::debug!("Virtual Device client stopped");
        }
        HomieClientEvent::Error(err) => {
            log::error!("Virtual Device HomieError: {:?}", err);
        }
    }
    Ok(false)
}
pub async fn handle_virtual_devices_changes_event(
    event: ConfigItemEvent<VirtualDeviceSpec>,
    state: &mut AppState,
) -> Result<bool> {
    match event {
        ConfigItemEvent::NewDocument(id, filename) => {
            state.vdm.add_cfg_file(id, filename).await;
        }
        ConfigItemEvent::RemoveDocument(id) => {
            state.vdm.remove_cfg_file(&id).await;
        }
        ConfigItemEvent::New(hash, spec) => match state.vdm.add_device(hash, spec).await {
            Ok(vdev_ref) => {
                let vdevices = state.vdm.read().await;
                if let Some(vdev) = vdevices.get(&vdev_ref) {
                    state
                        .rules
                        .queries_virtual_device_updated(&vdev_ref, vdev.description());
                }
            }
            Err(e) => {
                log::error!("Error adding device: {}", e);
            }
        },
        ConfigItemEvent::Removed(hash) => {
            if let Some(vdev) = state.vdm.remove_device(hash).await? {
                state
                    .rules
                    .queries_virtual_device_removed(vdev.device_ref(), vdev.description());
            }
        }
    }
    Ok(false)
}
