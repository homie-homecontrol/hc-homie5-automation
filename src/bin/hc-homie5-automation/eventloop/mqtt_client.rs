use color_eyre::eyre::Result;
use hc_homie5_automation::{
    app_state::{AppState, ConnectionEvent, ConnectionState},
    mqtt_client::MqttClientEvent,
    rules::run_mqtt_rules,
};

pub async fn handle_mqtt_client_event(event: MqttClientEvent, state: &mut AppState) -> Result<bool> {
    match event {
        MqttClientEvent::Connect => {
            log::debug!("MQTT client connected");
            let con_event = state.mqtt_state.change_state(ConnectionState::Connected);
            if let Some(ConnectionEvent::Reconnect) = con_event {
                state.mqtt_client.resubscribe().await?;
            }
            state.start_watchers().await;
        }
        MqttClientEvent::Disconnect => {
            log::debug!("MQTT client disconnected");
            state.mqtt_state.change_state(ConnectionState::Disconnected);
        }
        MqttClientEvent::PublishMessage(publish) => {
            log::debug!("MQTT value received: {} = {}", publish.topic, publish.payload);
            match state
                .vdm
                .update_member_value_mqtt(&publish.topic, &publish.payload)
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    log::warn!(
                        "Error updating virtual devices with value for {} - {}: {}",
                        publish.topic,
                        publish.payload,
                        err
                    );
                }
            }
            run_mqtt_rules(&publish, &state.as_rule_ctx()).await;
        }
        MqttClientEvent::Stop => {
            log::debug!("MQTT client stopped");
        }
        MqttClientEvent::Error(err) => {
            log::error!("MQTT Client Error: {:?}", err);
        }
    }
    Ok(false)
}
