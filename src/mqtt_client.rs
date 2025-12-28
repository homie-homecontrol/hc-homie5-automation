use std::{collections::HashMap, sync::Arc, time::Duration};

use rumqttc::{AsyncClient, ClientError, ConnectionError, MqttOptions, QoS, SubscribeFilter};
use thiserror::Error;
use tokio::{
    sync::{
        mpsc::{self, error::SendError, Receiver},
        watch, RwLock,
    },
    task::JoinError,
};

#[derive(Debug, Error)]
pub enum MqttClientError {
    #[error("Mqtt Client error: {0}")]
    MqttClient(#[from] ClientError),
    #[error("Error waiting for homie client task to complete: {0} -- {0:#?}")]
    JoinError(#[from] JoinError),
    #[error("Error sending event to mpsc channel: {0} -- {0:#?}")]
    ChannelSendError(#[from] SendError<MqttClientEvent>),
}

#[derive(Clone, Debug)]
pub struct MqttPublishEvent {
    pub topic: String,
    pub payload: String,
    pub duplicate: bool,
    pub retain: bool,
    pub qos: QoS,
}

#[derive(Debug)]
pub enum MqttClientEvent {
    Connect,
    Disconnect,
    Stop,
    PublishMessage(MqttPublishEvent),
    Error(ConnectionError),
}

pub struct MqttClientHandle {
    stop_sender: watch::Sender<bool>, // Shutdown signal
    handle: tokio::task::JoinHandle<Result<(), MqttClientError>>,
}

impl MqttClientHandle {
    /// Stops the watcher task.
    pub async fn stop(self) -> Result<(), MqttClientError> {
        let _ = self.stop_sender.send(true); // Send the shutdown signal
        self.handle.await??;
        Ok(())
    }
}

pub fn run_mqtt_client(
    mqttoptions: MqttOptions,
    channel_size: usize,
) -> Result<(MqttClientHandle, ManagedMqttClient, Receiver<MqttClientEvent>), Box<MqttClientError>> {
    log::trace!("Connecting to MQTT: {}", mqttoptions.client_id());
    let (sender, receiver) = mpsc::channel(channel_size);

    let (mqtt_client, mut eventloop) = AsyncClient::new(mqttoptions, channel_size);
    let (stop_sender, mut stop_receiver) = watch::channel(false);

    let handle = tokio::task::spawn(async move {
        let mut connected = false;
        loop {
            let poll_res = tokio::select! {
                poll_res = eventloop.poll() => poll_res,
                _exit = stop_receiver.changed() => {
                    if *stop_receiver.borrow() {
                        log::trace!("Received stop signal. Exiting...");
                        break;
                    }
                    continue;
                }
            };

            match poll_res {
                Ok(event) => match event {
                    rumqttc::Event::Incoming(rumqttc::Packet::Publish(p)) => {
                        let payload = match String::from_utf8(p.payload.to_vec()) {
                            Ok(payload) => payload,
                            Err(err) => {
                                log::warn!(
                                    "Cannot parse mqtt payload for topic [{}] to string. Error: {}",
                                    p.topic,
                                    err
                                );
                                continue;
                            }
                        };
                        let pe = MqttPublishEvent {
                            topic: p.topic,
                            retain: p.retain,
                            payload,
                            duplicate: p.dup,
                            qos: p.qos,
                        };
                        sender.send(MqttClientEvent::PublishMessage(pe)).await?;
                    }
                    rumqttc::Event::Incoming(rumqttc::Incoming::ConnAck(_)) => {
                        log::trace!("MQTT: Connected");
                        connected = true;
                        sender.send(MqttClientEvent::Connect).await?;
                    }
                    rumqttc::Event::Outgoing(rumqttc::Outgoing::Disconnect) => {
                        log::trace!("MQTT: Connection closed from our side.",);
                        sender.send(MqttClientEvent::Disconnect).await?;

                        break;
                    }
                    _ => {}
                },

                Err(err) => {
                    if connected {
                        connected = false;
                        sender.send(MqttClientEvent::Disconnect).await?;
                    }
                    log::error!("MQTTClient: Error connecting mqtt. {:#?}", err);
                    sender.send(MqttClientEvent::Error(err)).await?;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            };
        }
        sender.send(MqttClientEvent::Stop).await?;
        log::trace!("Exiting mqtt client eventloop...");
        Ok(())
    });
    Ok((MqttClientHandle { handle, stop_sender }, ManagedMqttClient::new(mqtt_client), receiver))
}

#[derive(Debug, Clone)]
pub struct ManagedMqttClient {
    client: AsyncClient,
    subscriptions: Arc<RwLock<HashMap<String, QoS>>>,
}

impl ManagedMqttClient {
    pub fn new(client: AsyncClient) -> Self {
        Self {
            client,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Sends a MQTT Publish to the `EventLoop`.
    pub async fn publish<S, V>(&self, topic: S, qos: QoS, retain: bool, payload: V) -> Result<(), MqttClientError>
    where
        S: Into<String>,
        V: Into<Vec<u8>>,
    {
        self.client.publish(topic, qos, retain, payload).await?;
        Ok(())
    }

    /// Sends a MQTT Subscribe to the `EventLoop`
    pub async fn subscribe<S: Into<String>>(&self, topic: S, qos: QoS) -> Result<(), MqttClientError> {
        let mut subs = self.subscriptions.write().await;
        let topic: String = topic.into();

        #[allow(clippy::map_entry)]
        if !subs.contains_key(&topic) {
            self.client.subscribe(topic.as_str(), qos).await?;
            subs.insert(topic, qos);
        }

        Ok(())
    }

    /// Sends a MQTT Unsubscribe to the `EventLoop`
    pub async fn unsubscribe<S: Into<String>>(&self, topic: S) -> Result<(), MqttClientError> {
        let topic = topic.into();
        let mut subs = self.subscriptions.write().await;
        subs.remove(&topic);
        self.client.unsubscribe(topic).await?;

        Ok(())
    }

    pub async fn resubscribe(&self) -> Result<(), MqttClientError> {
        let subs = self.subscriptions.read().await;
        self.client
            .subscribe_many(
                subs.iter()
                    .map(|(topic, qos)| SubscribeFilter::new(topic.to_string(), *qos)),
            )
            .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn disconnect(&self) -> Result<(), MqttClientError> {
        self.client.disconnect().await?;
        Ok(())
    }
}
