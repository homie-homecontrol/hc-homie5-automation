use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use config_watcher::ConfigItemHash;
use cron::Schedule;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex, PoisonError},
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

    // Cron fields are wall-clock times in the controller's local timezone (`TZ`).
    for next in upcoming_instants(&schedule, Local::now()) {
        let delay = (next - Utc::now()).to_std().unwrap_or_default();

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

/// Returns the instants after `after` at which `schedule` fires, evaluating its fields as wall-clock
/// times in the timezone of `after`.
///
/// A wall-clock time that occurs twice when daylight saving time ends fires once, at its first
/// occurrence. A wall-clock time skipped when daylight saving time starts does not fire.
fn upcoming_instants<Z>(schedule: &Schedule, after: DateTime<Z>) -> impl Iterator<Item = DateTime<Utc>>
where
    Z: TimeZone + 'static,
{
    let mut previous: Option<NaiveDateTime> = None;
    schedule
        .after_owned(after)
        .filter(move |next| {
            let local = next.naive_local();
            let repeated = previous == Some(local);
            previous = Some(local);
            !repeated
        })
        .map(|next| next.with_timezone(&Utc))
}

#[cfg(test)]
mod cron_manager_tests {
    use super::upcoming_instants;
    use chrono::{DateTime, TimeZone, Utc};
    use chrono_tz::Europe::Berlin;
    use cron::Schedule;
    use std::str::FromStr;

    fn berlin(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<chrono_tz::Tz> {
        Berlin
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
    }

    fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
    }

    fn next_instants(expression: &str, after: DateTime<chrono_tz::Tz>, count: usize) -> Vec<DateTime<Utc>> {
        let schedule = Schedule::from_str(expression).unwrap();
        upcoming_instants(&schedule, after).take(count).collect()
    }

    #[test]
    fn weekend_schedule_fires_at_local_time_in_summer() {
        // Friday 2026-10-16 is still CEST (UTC+2).
        let fired = next_instants("0 30 7 * * Sat,Sun *", berlin(2026, 10, 16, 12, 0), 2);
        assert_eq!(fired, vec![utc(2026, 10, 17, 5, 30), utc(2026, 10, 18, 5, 30)]);
    }

    #[test]
    fn weekday_schedule_fires_at_local_time_in_winter() {
        // Sunday 2026-12-06; December is CET (UTC+1).
        let fired = next_instants("0 15 6 * * Mon-Fri *", berlin(2026, 12, 6, 12, 0), 2);
        assert_eq!(fired, vec![utc(2026, 12, 7, 5, 15), utc(2026, 12, 8, 5, 15)]);
    }

    #[test]
    fn schedule_keeps_local_time_across_daylight_saving_end() {
        // CEST ends on Sunday 2026-10-25 at 03:00 local time.
        let fired = next_instants("0 30 7 * * * *", berlin(2026, 10, 24, 12, 0), 2);
        assert_eq!(fired, vec![utc(2026, 10, 25, 6, 30), utc(2026, 10, 26, 6, 30)]);
    }

    #[test]
    fn repeated_local_time_fires_once_when_daylight_saving_ends() {
        // 02:00 local time occurs at 00:00 and 01:00 UTC on 2026-10-25.
        let fired = next_instants("0 0 2 * * * *", berlin(2026, 10, 24, 12, 0), 2);
        assert_eq!(fired, vec![utc(2026, 10, 25, 0, 0), utc(2026, 10, 26, 1, 0)]);
    }

    #[test]
    fn skipped_local_time_does_not_fire_when_daylight_saving_starts() {
        // 02:00 to 03:00 local time does not exist on 2026-03-29.
        let fired = next_instants("0 30 2 * * * *", berlin(2026, 3, 28, 12, 0), 1);
        assert_eq!(fired, vec![utc(2026, 3, 30, 0, 30)]);
    }
}
