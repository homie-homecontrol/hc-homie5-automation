use std::{collections::HashMap, sync::Arc, time::Duration};

use hc_homie5::client::{FlushTimeout, PendingPublishObserver, PendingPublishTracker, QueuedPublishCounter};
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

    /// Like [`stop`](Self::stop), but first gives the client task up to
    /// `grace` to exit on its own.
    ///
    /// Use this after requesting a disconnect: the poll loop exits naturally
    /// once it processes `Outgoing::Disconnect`, sending a clean DISCONNECT
    /// packet. Signalling stop immediately instead can win the race against
    /// the still-queued disconnect request and tear down the TCP connection
    /// uncleanly.
    pub async fn stop_graceful(mut self, grace: Duration) -> Result<(), MqttClientError> {
        match tokio::time::timeout(grace, &mut self.handle).await {
            Ok(res) => {
                res??;
                Ok(())
            }
            Err(_elapsed) => {
                let _ = self.stop_sender.send(true);
                self.handle.await??;
                Ok(())
            }
        }
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
    let (mut pending_publishes, pending_publishes_observer) = PendingPublishTracker::new();
    let queued_counter = pending_publishes.queued_counter();

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
                    // Pending-publish tracking: pkids are recorded on outgoing
                    // publish and released on broker acknowledgement so
                    // `ManagedMqttClient::flush` can await an empty in-flight set.
                    rumqttc::Event::Outgoing(rumqttc::Outgoing::Publish(pkid)) => {
                        pending_publishes.record_publish(pkid);
                    }
                    rumqttc::Event::Incoming(rumqttc::Incoming::PubAck(ack)) => {
                        pending_publishes.record_ack(ack.pkid);
                    }
                    rumqttc::Event::Incoming(rumqttc::Incoming::PubComp(comp)) => {
                        pending_publishes.record_ack(comp.pkid);
                    }
                    rumqttc::Event::Outgoing(rumqttc::Outgoing::Disconnect) => {
                        log::trace!("MQTT: Connection closed from our side.",);
                        // Nothing can be acknowledged after the disconnect —
                        // release any flush waiters instead of letting them
                        // run into their max_wait.
                        pending_publishes.clear_all();
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
                    // Connection lost: rumqttc retransmits in-flight QoS>0
                    // publishes itself after reconnecting (re-emitted as
                    // Outgoing::Publish), so drop the stale pkids — they must
                    // not wedge flush(). Queued requests survive in rumqttc's
                    // request channel and are kept.
                    pending_publishes.clear_in_flight();
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
    Ok((
        MqttClientHandle { handle, stop_sender },
        ManagedMqttClient::new(mqtt_client, pending_publishes_observer, queued_counter),
        receiver,
    ))
}

#[derive(Debug, Clone)]
pub struct ManagedMqttClient {
    client: AsyncClient,
    subscriptions: Arc<RwLock<SubscriptionTracker>>,
    /// Observes queued plus in-flight (unacknowledged) publishes tracked by
    /// the [`run_mqtt_client`] poll loop.
    pending_publishes: PendingPublishObserver,
    /// Counts publishes enqueued but not yet seen by the poll loop.
    queued_publishes: QueuedPublishCounter,
}

/// Refcounted subscription bookkeeping for [`ManagedMqttClient`].
///
/// Multiple consumers (rule triggers, virtual-device MQTT members) may
/// subscribe to the same topic. The tracker counts subscribers per topic so
/// the broker subscribe/unsubscribe is only issued on the first add (0→1) and
/// the last remove (1→0).
#[derive(Debug, Default)]
struct SubscriptionTracker {
    map: HashMap<String, (QoS, usize)>,
}

impl SubscriptionTracker {
    /// Registers a subscriber for `topic`. Returns `true` iff the refcount
    /// transitioned 0→1, i.e. a broker subscribe is needed.
    ///
    /// Known limitation: the first subscriber's QoS wins. Later adds with a
    /// different QoS are counted but do not trigger a broker re-subscribe.
    fn add(&mut self, topic: String, qos: QoS) -> bool {
        match self.map.entry(topic) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().1 += 1;
                false
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((qos, 1));
                true
            }
        }
    }

    /// Removes a subscriber for `topic`. Returns `true` iff the refcount
    /// transitioned 1→0 and the entry was removed, i.e. a broker unsubscribe
    /// is needed. Removing an untracked topic is a no-op returning `false`.
    fn remove(&mut self, topic: &str) -> bool {
        match self.map.get_mut(topic) {
            Some((_, count)) if *count > 1 => {
                *count -= 1;
                false
            }
            Some(_) => {
                self.map.remove(topic);
                true
            }
            None => false,
        }
    }

    /// Iterates over all tracked topics and their QoS (for resubscribe).
    fn iter(&self) -> impl Iterator<Item = (&String, QoS)> {
        self.map.iter().map(|(topic, (qos, _))| (topic, *qos))
    }
}

impl ManagedMqttClient {
    pub fn new(
        client: AsyncClient,
        pending_publishes: PendingPublishObserver,
        queued_publishes: QueuedPublishCounter,
    ) -> Self {
        Self {
            client,
            subscriptions: Arc::new(RwLock::new(SubscriptionTracker::default())),
            pending_publishes,
            queued_publishes,
        }
    }

    /// Waits until all publishes issued before this call have been
    /// acknowledged by the broker.
    ///
    /// Returns immediately when nothing is pending; `max_wait` is only an
    /// upper bound for the dead-broker case. Call this before disconnecting
    /// to guarantee that final messages reached the broker.
    pub async fn flush(&self, max_wait: Duration) -> Result<(), FlushTimeout> {
        self.pending_publishes.flushed(max_wait).await
    }

    /// Sends a MQTT Publish to the `EventLoop`.
    pub async fn publish<S, V>(&self, topic: S, qos: QoS, retain: bool, payload: V) -> Result<(), MqttClientError>
    where
        S: Into<String>,
        V: Into<Vec<u8>>,
    {
        // Count before enqueueing so the poll loop can never observe the
        // request ahead of the counter increment (flush correctness).
        self.queued_publishes.increment();
        if let Err(err) = self.client.publish(topic, qos, retain, payload).await {
            self.queued_publishes.decrement();
            return Err(err.into());
        }
        Ok(())
    }

    /// Registers a subscription for `topic`. The broker subscribe is only
    /// sent for the first subscriber of a topic (refcounted).
    pub async fn subscribe<S: Into<String>>(&self, topic: S, qos: QoS) -> Result<(), MqttClientError> {
        let mut subs = self.subscriptions.write().await;
        let topic: String = topic.into();

        if subs.add(topic.clone(), qos) {
            self.client.subscribe(topic, qos).await?;
        }

        Ok(())
    }

    /// Removes a subscription for `topic`. The broker unsubscribe is only
    /// sent when the last subscriber of the topic is removed (refcounted).
    /// Unsubscribing from an untracked topic is a no-op.
    pub async fn unsubscribe<S: Into<String>>(&self, topic: S) -> Result<(), MqttClientError> {
        let topic = topic.into();
        let mut subs = self.subscriptions.write().await;
        if subs.remove(&topic) {
            self.client.unsubscribe(topic).await?;
        }

        Ok(())
    }

    pub async fn resubscribe(&self) -> Result<(), MqttClientError> {
        let subs = self.subscriptions.read().await;
        self.client
            .subscribe_many(
                subs.iter()
                    .map(|(topic, qos)| SubscribeFilter::new(topic.to_string(), qos)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_returns_true_only_on_first_subscriber() {
        let mut tracker = SubscriptionTracker::default();
        assert!(tracker.add("a/b".into(), QoS::AtLeastOnce)); // 0 -> 1: broker subscribe
        assert!(!tracker.add("a/b".into(), QoS::AtLeastOnce)); // 1 -> 2: no broker call
        assert!(!tracker.add("a/b".into(), QoS::AtMostOnce)); // different QoS still counted only
    }

    #[test]
    fn remove_returns_true_only_on_last_subscriber() {
        let mut tracker = SubscriptionTracker::default();
        tracker.add("a/b".into(), QoS::AtLeastOnce);
        tracker.add("a/b".into(), QoS::AtLeastOnce);
        assert!(!tracker.remove("a/b")); // 2 -> 1: no broker unsubscribe
        assert!(tracker.remove("a/b")); // 1 -> 0: broker unsubscribe
        assert_eq!(tracker.iter().count(), 0);
    }

    #[test]
    fn remove_untracked_topic_is_noop() {
        let mut tracker = SubscriptionTracker::default();
        assert!(!tracker.remove("never/subscribed"));
        tracker.add("a/b".into(), QoS::AtLeastOnce);
        assert!(!tracker.remove("other/topic"));
        assert_eq!(tracker.iter().count(), 1);
    }

    #[test]
    fn first_qos_wins() {
        let mut tracker = SubscriptionTracker::default();
        tracker.add("a/b".into(), QoS::ExactlyOnce);
        tracker.add("a/b".into(), QoS::AtMostOnce);
        let (topic, qos) = tracker.iter().next().unwrap();
        assert_eq!(topic, "a/b");
        assert_eq!(qos, QoS::ExactlyOnce);
    }

    #[test]
    fn iter_yields_all_tracked_topics() {
        let mut tracker = SubscriptionTracker::default();
        tracker.add("a".into(), QoS::AtMostOnce);
        tracker.add("b".into(), QoS::AtLeastOnce);
        tracker.add("b".into(), QoS::AtLeastOnce);
        let mut topics: Vec<_> = tracker.iter().map(|(t, _)| t.clone()).collect();
        topics.sort();
        assert_eq!(topics, vec!["a", "b"]);
    }

    #[test]
    fn resubscribe_after_partial_remove_keeps_topic() {
        let mut tracker = SubscriptionTracker::default();
        tracker.add("shared".into(), QoS::AtLeastOnce); // rule
        tracker.add("shared".into(), QoS::AtLeastOnce); // vdev member
        tracker.remove("shared"); // rule removed
        let remaining: Vec<_> = tracker.iter().collect();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].0, "shared");
    }
}
