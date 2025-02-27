use config_watcher::ConfigItemHash;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

use crate::rules::{RuleAction, RuleTriggerEvent};

#[derive(Debug)]
pub struct Timer {
    #[allow(dead_code)]
    pub id: String,
    pub rule_hash: ConfigItemHash,
    pub handle: JoinHandle<()>,
}

#[derive(Debug, Clone)]
pub struct TimerEvent {
    pub id: String,
    pub rule_hash: ConfigItemHash,
    pub rule_action: Option<RuleAction>,
    pub trigger_event: Option<Box<RuleTriggerEvent<'static>>>,
}

#[derive(Debug, Clone)]
pub struct TimerManager {
    timers: Arc<Mutex<HashMap<String, Timer>>>,
    sender: Sender<TimerEvent>,
}

impl TimerManager {
    pub fn new() -> (Self, Receiver<TimerEvent>) {
        let (sender, receiver) = mpsc::channel(1024);
        (
            TimerManager {
                timers: Arc::new(Mutex::new(HashMap::new())),
                sender,
            },
            receiver,
        )
    }

    /// Creates a new timer
    pub fn create_timer(
        &self,
        rule_hash: ConfigItemHash,
        id: String,
        duration: Duration,
        repeat: Option<Duration>,
        rule_action: Option<RuleAction>,
        trigger_event: Option<RuleTriggerEvent<'_>>,
    ) {
        // cancel existing timer for the id if it exists
        self.cancel_timer(&id);

        let timers = Arc::clone(&self.timers);

        let sender = self.sender.clone();

        let id_task = id.clone();

        let trigger_event = trigger_event.as_ref().map(|e| Box::new(e.to_owned()));

        // Spawn a new task for the timer
        let handle = tokio::spawn(async move {
            log::debug!("Timer {} created with delay {:?}", id_task, duration);

            // Wait for the initial delay
            tokio::time::sleep(duration).await;

            if let Err(err) = sender
                .send(TimerEvent {
                    id: id_task.clone(),
                    rule_hash,
                    rule_action: rule_action.clone(),
                    trigger_event: trigger_event.clone(),
                })
                .await
            {
                log::warn!("Error sending timer trigger: [{}] - {}", id_task, err);
            }

            // If an interval is configured, start repeating
            if let Some(interval_duration) = repeat {
                log::debug!("Timer {} set to repeat every {:?}", id_task, interval_duration);

                let mut interval = tokio::time::interval(interval_duration);
                interval.tick().await; // Skip the immediate first tick

                loop {
                    interval.tick().await;
                    if let Err(err) = sender
                        .send(TimerEvent {
                            id: id_task.clone(),
                            rule_hash,
                            rule_action: rule_action.clone(),
                            trigger_event: trigger_event.clone(),
                        })
                        .await
                    {
                        log::warn!("Error sending timer trigger: [{}] - {}", id_task, err);
                    }
                }
            }

            // Remove the timer from the manager once it's done
            timers.lock().unwrap().remove(&id_task);
        });

        // Store the handle in the hashmap
        self.timers
            .lock()
            .unwrap()
            .insert(id.clone(), Timer { id, rule_hash, handle });
    }

    /// Cancels a timer by ID
    pub fn cancel_timer(&self, id: &str) {
        let mut timers = self.timers.lock().unwrap();
        if let Some(timer) = timers.remove(id) {
            // Cancel the timer by aborting the task
            timer.handle.abort();
            log::debug!("Timer {} cancelled.", id);
        }
    }

    pub fn remove_timers_for_rule(&self, rule_hash: ConfigItemHash) {
        let mut timers = self.timers.lock().unwrap();
        timers.retain(|id, timer| {
            if timer.rule_hash == rule_hash {
                timer.handle.abort();
                log::debug!("Timer {} cancelled.", id);
                false
            } else {
                true
            }
        });
    }
    pub fn clear(&self) {
        log::debug!("Removing all timers");
        let mut timers = self.timers.lock().unwrap();
        for (_, timer) in timers.drain() {
            timer.handle.abort();
        }
    }
}
