use homie5::{Homie5Message, ToTopic};
use std::time::Instant;
use tokio::{
    sync::mpsc,
    time::{sleep, Duration},
};

/// Wraps a `tokio::mpsc::Receiver` and returns a new receiver that ensures messages
/// are forwarded with at least `delay` time between them.
///
/// The function spawns a background task and automatically shuts down when the
/// original sender is dropped.
pub fn throttle_channel<T: Send + 'static>(mut input_rx: mpsc::Receiver<T>, delay: Duration) -> mpsc::Receiver<T> {
    let (throttled_tx, throttled_rx) = mpsc::channel(65535);

    tokio::spawn(async move {
        let mut last_sent = Instant::now();

        while let Some(msg) = input_rx.recv().await {
            let elapsed = last_sent.elapsed();
            if elapsed < delay {
                sleep(delay - elapsed).await;
            }

            if throttled_tx.send(msg).await.is_err() {
                break; // Stop if the receiver is closed
            }

            last_sent = Instant::now();
        }
    });

    throttled_rx
}

pub fn log_homie_message(msg: &Homie5Message) -> String {
    match msg {
        Homie5Message::DeviceState { device, state } => {
            format!("[DeviceState]: Device: [{}], State: [{}]", device.to_topic().build(), state)
        }
        Homie5Message::DeviceDescription { device, description } => {
            format!(
                "[DeviceDescription]: Device: [{}], Description: \n{}",
                device.to_topic().build(),
                serde_yml::to_string(description).unwrap_or_default()
            )
        }

        Homie5Message::DeviceLog { device, level, log_msg } => {
            format!("[DeviceLog]: Device: [{}], level: [{}], message: [{}]", device.to_topic().build(), level, log_msg)
        }
        Homie5Message::DeviceAlert {
            device,
            alert_id,
            alert_msg,
        } => {
            format!(
                "[DeviceAlert]: Device: [{}], id: [{}], message: [{}]",
                device.to_topic().build(),
                alert_id,
                alert_msg
            )
        }
        Homie5Message::PropertyValue { property, value } => {
            format!("[PropertyValue]: Property: [{}], value: [{}]", property.to_topic().build(), value)
        }
        Homie5Message::PropertyTarget { property, target } => {
            format!("[PropertyTarget]: Property: [{}], value: [{}]", property.to_topic().build(), target)
        }

        Homie5Message::PropertySet { property, set_value } => {
            format!("[PropertySet]: Property: [{}], set_value: [{}]", property.to_topic().build(), set_value)
        }
        Homie5Message::Broadcast {
            homie_domain,
            subtopic,
            data,
        } => format!("[Broadcast]: homie domain: [{}], subtopic: [{}], data:[{}]", homie_domain, subtopic, data),
        Homie5Message::DeviceRemoval { device } => format!("[DeviceRemoval]: Device: [{}]", device.to_topic().build()),
    }
}
