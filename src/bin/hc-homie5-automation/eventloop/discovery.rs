use color_eyre::eyre::Result;
use hc_homie5::client::HomieClientEvent;
use hc_homie5::model::DiscoveryAction;
use hc_homie5_automation::{
    app_state::{AppState, ConnectionEvent, ConnectionState},
    rules::run_subject_rules,
    utils::log_homie_message,
};
use homie5::ToTopic;

pub async fn handle_discovery_client_event(event: HomieClientEvent, state: &mut AppState) -> Result<bool> {
    match event {
        HomieClientEvent::Connect => {
            log::debug!("Homie discovery mqtt client connected");
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
            log::debug!("Homie discovery mqtt client disconnected");
            state.discovery_state.change_state(ConnectionState::Disconnected);
        }
        HomieClientEvent::HomieMessage(event) => {
            log::trace!("Discovery: {}", log_homie_message(&event));
            let action = state.dm.discovery_handle_event(event).await?;
            if let Some(action) = action {
                match action {
                    DiscoveryAction::Unhandled(_) => {
                        // ignore unhandled messages
                    }
                    // device added / changed
                    DiscoveryAction::DeviceDescriptionChanged(ref device_ref) => {
                        log::debug!("Device discovered/updated: {}", device_ref.to_topic().build());
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

                        // iterate over all rules and update any materialized queries with the
                        // updated/added device
                        state.rules.queries_device_updated(device_ref, desc);
                    }
                    // device removed
                    DiscoveryAction::DeviceRemoved(device) => {
                        log::debug!("Device removed: {}", device.ident.to_topic().build());
                        // iterate over all rules and update any materialized queries with the
                        // removed device

                        state.vdm.update_compound_members_removed(&device.ident).await?;

                        state
                            .rules
                            .queries_device_removed(&device.ident, device.description.as_ref());
                    }
                    DiscoveryAction::StateChanged { device, from, to } => {
                        log::debug!("Device state changed: {}: {} -> {}", device.to_topic().build(), from, to);
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
            log::debug!("Homie discovery client stopped");
        }
        HomieClientEvent::Error(err) => {
            log::error!("Homie discovery HomieError: {:?}", err);
        }
    }
    Ok(false)
}
