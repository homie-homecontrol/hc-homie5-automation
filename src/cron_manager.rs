use chrono::{DateTime, Local, Utc};
use config_watcher::ConfigItemHash;
use cron::Schedule;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

use hc_homie5::logging::chain;

use crate::panic_guard::contain;

#[derive(Debug)]
pub struct ScheduledCron {
    #[allow(dead_code)]
    pub id: String,
    pub rule_hash: ConfigItemHash,
    pub handle: JoinHandle<()>,
}

#[derive(Debug, Clone)]
pub struct CronEvent {
    #[allow(dead_code)]
    pub id: String,
    pub rule_hash: ConfigItemHash,
    pub trigger_index: usize,
}

#[derive(Debug, Clone)]
pub struct CronManager {
    cron_schedules: Arc<Mutex<HashMap<String, ScheduledCron>>>,
    sender: Sender<CronEvent>,
}

impl CronManager {
    pub fn new() -> (Self, Receiver<CronEvent>) {
        let (sender, receiver) = mpsc::channel(1024);
        (
            Self {
                cron_schedules: Arc::new(Mutex::new(HashMap::new())),
                sender,
            },
            receiver,
        )
    }

    /// Creates a new timer
    pub fn schedule_cron(&self, rule_hash: ConfigItemHash, trigger_index: usize, cron_schedule: &str) {
        let id = format!("{}-{}", rule_hash, trigger_index);
        // cancel existing timer for the id if it exists
        self.cancel_cron_schedule(&id);

        let sender = self.sender.clone();

        let id_task = id.clone();
        let task_cron_schedule = cron_schedule.to_string();

        // Spawn a new task for the timer
        let task_id = id.clone();
        let handle = tokio::spawn(async move {
            let schedule_task = run_cron_schedule(task_cron_schedule, id_task, rule_hash, trigger_index, sender);
            if let Err(panic) = contain(schedule_task).await {
                tracing::error!(cron = %task_id, error = chain(&panic), "cron schedule panicked and stopped");
            }
        });

        // Store the handle in the hashmap
        self.cron_schedules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone(), ScheduledCron { id, rule_hash, handle });
    }

    /// Cancels a timer by ID
    pub fn cancel_cron_schedule(&self, id: &str) {
        let mut cron_schedules = self.cron_schedules.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(schedule) = cron_schedules.remove(id) {
            // Cancel the timer by aborting the task
            schedule.handle.abort();
            tracing::trace!(cron = %id, "cron schedule aborted");
        }
    }

    pub fn remove_cron_schedule_for_rule(&self, rule_hash: ConfigItemHash) {
        let mut cron_schedules = self.cron_schedules.lock().unwrap_or_else(PoisonError::into_inner);
        cron_schedules.retain(|id, schedule| {
            if schedule.rule_hash == rule_hash {
                schedule.handle.abort();
                tracing::debug!(cron = %id, "cron schedule cancelled");
                false
            } else {
                true
            }
        });
    }

    pub fn clear(&self) {
        tracing::debug!("all cron schedules cancelled");
        let mut cron_schedules = self.cron_schedules.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, schedule) in cron_schedules.drain() {
            schedule.handle.abort();
        }
    }
}

/// Sends a [`CronEvent`] at every upcoming time of `cron_schedule` until the schedule ends.
async fn run_cron_schedule(
    cron_schedule: String,
    id: String,
    rule_hash: ConfigItemHash,
    trigger_index: usize,
    sender: Sender<CronEvent>,
) {
    let Ok(schedule) = Schedule::from_str(&cron_schedule) else {
        // Reported with the rule name when the rule is loaded (`rules::engine::cron::schedule_cron`).
        tracing::debug!(cron = %id, schedule = %cron_schedule, "invalid cron schedule; schedule not started");
        return;
    };

    // Iterate over the schedule and send events
    for next in schedule.upcoming(Utc) {
        // Calculate the delay
        let now: DateTime<Utc> = SystemTime::now().into();
        let delay = (next - now)
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(0));

        tracing::debug!(cron = %id, next = %next.with_timezone(&Local), "cron schedule waiting");
        tokio::time::sleep(delay).await;

        // Send the event via the channel
        if let Err(err) = sender
            .send(CronEvent {
                id: id.clone(),
                rule_hash,
                trigger_index,
            })
            .await
        {
            tracing::debug!(cron = %id, error = chain(&err), "cannot deliver cron event");
        }
    }
}
