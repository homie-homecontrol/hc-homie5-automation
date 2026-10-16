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
                log::error!("Cron schedule {} panicked and stopped: {}", task_id, panic);
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
            log::debug!("Schedule {} cancelled.", id);
        }
    }

    pub fn remove_cron_schedule_for_rule(&self, rule_hash: ConfigItemHash) {
        let mut cron_schedules = self.cron_schedules.lock().unwrap_or_else(PoisonError::into_inner);
        cron_schedules.retain(|id, schedule| {
            if schedule.rule_hash == rule_hash {
                schedule.handle.abort();
                log::debug!("Schedule {} cancelled.", id);
                false
            } else {
                true
            }
        });
    }

    pub fn clear(&self) {
        log::debug!("Removing all cron schedules");
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
        log::error!(
            "Invalid cron expression: [{}] - 7 fields required, like [* * * * * * *] (sec, min, hour, day of month, month, day of week, year)",
            cron_schedule
        );
        return;
    };

    // Iterate over the schedule and send events
    for next in schedule.upcoming(Utc) {
        // Calculate the delay
        let now: DateTime<Utc> = SystemTime::now().into();
        let delay = (next - now)
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(0));

        log::debug!("{} - next execution at: {}", id, next.with_timezone(&Local));
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
            log::error!("Error sending cron event: {:?}", err);
        }
    }
}
