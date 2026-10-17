use color_eyre::eyre::Result;
use hc_homie5::logging::{truncate, TRACE_PAYLOAD_MAX};
use hc_homie5_automation::{
    app_state::{AppState, ConnectionEvent, ConnectionState},
    mqtt_client::MqttClientEvent,
    rules::run_mqtt_rules,
};

use super::MqttConnection;

pub async fn handle_mqtt_client_event(event: MqttClientEvent, state: &mut AppState) -> Result<bool> {
    match event {
        MqttClientEvent::Connect => {
            MqttConnection::Mqtt.connected(&mut state.mqtt_outage);
            let con_event = state.mqtt_state.change_state(ConnectionState::Connected);
            if let Some(ConnectionEvent::Reconnect) = con_event {
                state.mqtt_client.resubscribe().await?;
                state.meta.republish_all().await?;
            }
            state.start_watchers().await;
        }
        MqttClientEvent::Disconnect => {
            tracing::debug!(connection = "mqtt", "MQTT disconnected");
            state.mqtt_state.change_state(ConnectionState::Disconnected);
        }
        MqttClientEvent::PublishMessage(publish) => {
            tracing::trace!(
                topic = %publish.topic,
                payload = truncate(&publish.payload, TRACE_PAYLOAD_MAX),
                "MQTT message received"
            );
            // Updates virtual device members, then runs the matching rules.
            run_mqtt_rules(&publish, &state.as_rule_ctx()).await;
        }
        MqttClientEvent::Stop => {
            MqttConnection::Mqtt.stopped();
        }
        MqttClientEvent::Error(err) => {
            MqttConnection::Mqtt.failed(&mut state.mqtt_outage, &err);
        }
    }
    Ok(false)
}
