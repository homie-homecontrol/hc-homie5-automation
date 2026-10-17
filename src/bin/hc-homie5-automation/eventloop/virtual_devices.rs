use color_eyre::eyre::Result;
use config_watcher::ConfigItemEvent;
use hc_homie5::client::HomieClientEvent;
use hc_homie5::device::{HomieDevice, HomieDeviceCore};
use hc_homie5::logging::{chain, truncate, PAYLOAD_MAX, TRACE_PAYLOAD_MAX};
use hc_homie5_automation::utils::log_homie_message;
use hc_homie5_automation::{
    app_state::AppState, app_state::ConnectionEvent, app_state::ConnectionState, rules::run_on_set_rules,
    virtual_devices::VirtualDeviceSpec,
};

use super::MqttConnection;

pub async fn handle_virtual_devices_client_event(event: HomieClientEvent, state: &mut AppState) -> Result<bool> {
    match event {
        HomieClientEvent::Connect => {
            MqttConnection::VirtualDevices.connected(&mut state.virtual_devices_outage);
            state.vdm.publish_device().await?;

            let con_event = state.virtual_devices_state.change_state(ConnectionState::Connected);
            if let Some(ConnectionEvent::Reconnect) = con_event {
                state.vdm.publish_child_devices().await?;
            }
            state.start_watchers().await;
        }
        HomieClientEvent::Disconnect => {
            tracing::debug!(connection = "virtual-devices", "MQTT disconnected");
            state.virtual_devices_state.change_state(ConnectionState::Disconnected);
        }
        HomieClientEvent::HomieMessage(event) => {
            tracing::trace!(event = truncate(&log_homie_message(&event), TRACE_PAYLOAD_MAX), "virtual device message");
            match &event {
                homie5::Homie5Message::PropertySet { property, set_value } => {
                    tracing::debug!(
                        vdevice = %property.device_id(),
                        node = %property.node_id(),
                        property = %property.prop_id(),
                        value = truncate(set_value, PAYLOAD_MAX),
                        "set command received"
                    );
                    state.vdm.handle_set_command(property, set_value).await?;
                    run_on_set_rules(&event, &state.as_rule_ctx()).await;
                }
                homie5::Homie5Message::PropertyValue { property, value } => {
                    state.vdm.handle_mqtt_read(property, value).await?;
                }
                _ => (),
            }
        }
        HomieClientEvent::MetaMessage(_) => {
            // meta messages are handled by the discovery client
        }
        HomieClientEvent::Stop => {
            MqttConnection::VirtualDevices.stopped();
        }
        HomieClientEvent::Error(err) => {
            MqttConnection::VirtualDevices.failed(&mut state.virtual_devices_outage, &err);
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
        ConfigItemEvent::New(hash, spec) => {
            let vdevice = spec.id.clone();
            let file = state.vdm.get_cfg_filename(&hash).await.unwrap_or_default();
            match state.vdm.add_device(hash, spec).await {
                Ok(vdev_ref) => {
                    tracing::info!(vdevice = %vdevice, file = %file, "virtual device added");
                    let vdevices = state.vdm.read().await;
                    if let Some(vdev) = vdevices.get(&vdev_ref) {
                        state
                            .rules
                            .queries_virtual_device_updated(&vdev_ref, vdev.description());
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        vdevice = %vdevice,
                        file = %file,
                        error = chain(err.as_ref()),
                        "cannot add virtual device"
                    );
                }
            }
        }
        ConfigItemEvent::Removed(hash) => {
            if let Some(vdev) = state.vdm.remove_device(hash).await? {
                let file = state.vdm.get_cfg_filename(&hash).await.unwrap_or_default();
                tracing::info!(vdevice = %vdev.device_ref().device_id(), file = %file, "virtual device removed");
                state
                    .rules
                    .queries_virtual_device_removed(vdev.device_ref(), vdev.description());
            }
        }
        ConfigItemEvent::Invalid { name, error, .. } => {
            tracing::warn!(file = %name, error, "invalid config item skipped");
        }
    }
    Ok(false)
}
