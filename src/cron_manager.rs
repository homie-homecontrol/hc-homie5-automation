use chrono::{DateTime, Local, Utc};
use config_watcher::ConfigItemHash;
use cron::Schedule;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

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
        let handle = tokio::spawn(async move {
            let Ok(schedule) = Schedule::from_str(&task_cron_schedule) else {
                log::error!("Invalid cron expression: [{}] - 7 fields required, like [* * * * * * *] (sec, min, hour, day of month, month, day of week, year)", task_cron_schedule);
                return;
            };

            // Iterate over the schedule and send events
            for next in schedule.upcoming(Utc) {
                // Calculate the delay
                let now: DateTime<Utc> = SystemTime::now().into();
                let delay = (next - now)
                    .to_std()
                    .unwrap_or_else(|_| std::time::Duration::from_secs(0));

                log::debug!("{} - next execution at: {}", id_task, next.with_timezone(&Local));
                tokio::time::sleep(delay).await;

                // Send the event via the channel
                if let Err(err) = sender
                    .send(CronEvent {
                        id: id_task.clone(),
                        rule_hash,
                        trigger_index,
                    })
                    .await
                {
                    log::error!("Error sending cron event: {:?}", err);
                }
            }
        });

        // Store the handle in the hashmap
        self.cron_schedules
            .lock()
            .unwrap()
            .insert(id.clone(), ScheduledCron { id, rule_hash, handle });
    }

    /// Cancels a timer by ID
    pub fn cancel_cron_schedule(&self, id: &str) {
        let mut cron_schedules = self.cron_schedules.lock().unwrap();
        if let Some(schedule) = cron_schedules.remove(id) {
            // Cancel the timer by aborting the task
            schedule.handle.abort();
            log::debug!("Schedule {} cancelled.", id);
        }
    }

    pub fn remove_cron_schedule_for_rule(&self, rule_hash: ConfigItemHash) {
        let mut cron_schedules = self.cron_schedules.lock().unwrap();
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
        let mut cron_schedules = self.cron_schedules.lock().unwrap();
        for (_, schedule) in cron_schedules.drain() {
            schedule.handle.abort();
        }
    }
}
