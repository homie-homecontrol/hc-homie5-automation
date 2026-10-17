use color_eyre::eyre::Result;
use hc_homie5::client::HomieClientEvent;
use hc_homie5::logging::{truncate, TRACE_PAYLOAD_MAX};
use hc_homie5::model::DiscoveryAction;
use hc_homie5_automation::{
    app_state::{AppState, ConnectionEvent, ConnectionState},
    rules::run_subject_rules,
    utils::log_homie_message,
};

use super::MqttConnection;

pub async fn handle_discovery_client_event(event: HomieClientEvent, state: &mut AppState) -> Result<bool> {
    match event {
        HomieClientEvent::Connect => {
            MqttConnection::Discovery.connected(&mut state.discovery_outage);
            if let Some(ConnectionEvent::Reconnect) = state.discovery_state.change_state(ConnectionState::Connected) {
                let mut devices = state.dm.write().await;
                devices.clear();
                drop(devices);
                state.meta_handler.clear();
            }
            state.dm.discover().await?;
            state.start_watchers().await;
        }
        HomieClientEvent::Disconnect => {
            tracing::debug!(connection = "discovery", "MQTT disconnected");
            state.discovery_state.change_state(ConnectionState::Disconnected);
        }
        HomieClientEvent::HomieMessage(event) => {
            tracing::trace!(event = truncate(&log_homie_message(&event), TRACE_PAYLOAD_MAX), "discovery message");
            let action = state.dm.discovery_handle_event(event).await?;
            if let Some(action) = action {
                match action {
                    DiscoveryAction::Unhandled(_) => {
                        // ignore unhandled messages
                    }
                    DiscoveryAction::NewDevice { ref device, ref status } => {
                        tracing::debug!(device = %device.device_id(), state = %status, "device discovered");
                        run_subject_rules(&action, &state.as_rule_ctx()).await;
                    }
                    // device added / changed
                    DiscoveryAction::DeviceDescriptionChanged(ref device_ref) => {
                        tracing::debug!(device = %device_ref.device_id(), "device description changed");
                        // Write lock scope: apply pending meta overlays, then drop
                        {
                            let mut devices = state.dm.write().await;
                            state.meta_handler.apply_pending_for_device(device_ref, &mut devices);
                        }
                        // Read lock scope: update compound members and queries
                        let devices = state.dm.read().await;
                        let desc = devices.get_device(device_ref).and_then(|d| d.description.as_ref());

                        if let Some(desc) = desc {
                            state.vdm.update_compound_members(device_ref, desc).await?;
                        }

                        // schedule debounced rebuilds for group devices this
                        // device is a member of (no-op for non-members)
                        state.vdm.source_device_changed(device_ref).await;

                        // iterate over all rules and update any materialized queries with the
                        // updated/added device
                        state.rules.queries_device_updated(device_ref, desc);
                    }
                    // device removed
                    DiscoveryAction::DeviceRemoved(device) => {
                        tracing::debug!(device = %device.ident.device_id(), "device removed");
                        // iterate over all rules and update any materialized queries with the
                        // removed device

                        state.vdm.update_compound_members_removed(&device.ident).await?;

                        // schedule debounced rebuilds for group devices this
                        // device was a member of (no-op for non-members)
                        state.vdm.source_device_removed(&device.ident).await;

                        state
                            .rules
                            .queries_device_removed(&device.ident, device.description.as_ref());
                    }
                    DiscoveryAction::StateChanged { device, from, to } => {
                        tracing::debug!(device = %device.device_id(), from = %from, to = %to, "device state changed");
                        // Recalculate aggregated values for virtual properties referencing
                        // this device, since the aggregate function filters members by
                        // Ready state — a state transition changes which members participate.
                        state.vdm.recalculate_members_for_device(&device).await?;
                    }
                    _ => {
                        run_subject_rules(&action, &state.as_rule_ctx()).await;
                    }
                }
            }
        }
        HomieClientEvent::MetaMessage(event) => {
            let mut devices = state.dm.write().await;
            state.meta_handler.handle_meta_message(event, &mut devices);
        }
        HomieClientEvent::Stop => {
            MqttConnection::Discovery.handle_stop(state).await;
        }
        HomieClientEvent::Error(err) => {
            MqttConnection::Discovery.failed(&mut state.discovery_outage, &err);
        }
    }
    Ok(false)
}
