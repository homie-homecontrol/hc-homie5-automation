use config_watcher::ConfigItemHash;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
};

use crate::panic_guard::contain;
use crate::rules::{RuleAction, RuleTriggerEvent};

/// Longest accepted timer duration or repeat interval (100 years); longer values are clamped so
/// deadline arithmetic cannot overflow.
pub const MAX_TIMER_DURATION: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

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
        // NOTE: must happen before we acquire the lock in `spawn_and_register`
        // because `cancel_timer` locks `timers` itself (std Mutex is not
        // re-entrant).
        self.cancel_timer(&id);

        let (duration, repeat) = normalize_timer_durations(&id, duration, repeat);

        let timers = Arc::clone(&self.timers);
        let sender = self.sender.clone();
        let id_task = id.clone();
        let trigger_event = trigger_event.as_ref().map(|e| Box::new(e.to_owned()));

        // The timer body: wait the initial delay, fire, optionally repeat, and
        // finally remove its own entry. The self-removal locks `timers`, so it
        // is blocked until `spawn_and_register` has inserted the handle.
        let body = async move {
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
            timers.lock().unwrap_or_else(PoisonError::into_inner).remove(&id_task);
        };

        let timer_id = id.clone();
        spawn_and_register(
            &self.timers,
            id,
            move |handle| Timer {
                id: timer_id,
                rule_hash,
                handle,
            },
            body,
        );
    }

    /// Cancels a timer by ID
    pub fn cancel_timer(&self, id: &str) {
        let mut timers = self.timers.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(timer) = timers.remove(id) {
            // Cancel the timer by aborting the task
            timer.handle.abort();
            log::debug!("Timer {} cancelled.", id);
        }
    }

    pub fn remove_timers_for_rule(&self, rule_hash: ConfigItemHash) {
        let mut timers = self.timers.lock().unwrap_or_else(PoisonError::into_inner);
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
        let mut timers = self.timers.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, timer) in timers.drain() {
            timer.handle.abort();
        }
    }
}

/// Clamps `duration` and `repeat` to [`MAX_TIMER_DURATION`] and turns a zero repeat interval into
/// a one-shot timer; `tokio::time::interval` panics on a zero period.
fn normalize_timer_durations(id: &str, duration: Duration, repeat: Option<Duration>) -> (Duration, Option<Duration>) {
    let repeat = match repeat {
        Some(interval) if interval.is_zero() => {
            log::warn!("Timer {} has a zero repeat interval; it fires once", id);
            None
        }
        other => other.map(|interval| interval.min(MAX_TIMER_DURATION)),
    };
    (duration.min(MAX_TIMER_DURATION), repeat)
}

/// Spawns `body` as a task and registers its [`JoinHandle`] in `timers` under a
/// single lock acquisition.
///
/// The lock is acquired **before** spawning and held across the insert, so a
/// task that removes its own map entry on completion (which also locks
/// `timers`) cannot run that removal until the handle has been stored. This
/// closes the create/insert race where a zero/short-duration timer could finish
/// and try to remove its entry before `create_timer` ever inserted it, leaving
/// a stale handle behind. No `.await` occurs while the guard is held, so the
/// std `Mutex` is safe to use here.
fn spawn_and_register<V>(
    timers: &Arc<Mutex<HashMap<String, V>>>,
    id: String,
    make_entry: impl FnOnce(JoinHandle<()>) -> V,
    body: impl std::future::Future<Output = ()> + Send + 'static,
) {
    let mut guard = timers.lock().unwrap_or_else(PoisonError::into_inner);
    let handle = tokio::spawn(async move {
        if let Err(panic) = contain(body).await {
            log::error!("Timer task panicked: {}", panic);
        }
    });
    guard.insert(id, make_entry(handle));
    // guard dropped here, after the insert
}

#[cfg(test)]
mod timer_manager_tests {
    use super::{normalize_timer_durations, spawn_and_register, MAX_TIMER_DURATION};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;

    // Exercises the same `spawn_and_register` path used by `create_timer`. We
    // store bare `JoinHandle`s here (rather than `Timer`) so the test does not
    // need a `ConfigItemHash`, which has no public constructor; the locking
    // discipline under test is identical.

    #[tokio::test]
    async fn zero_duration_one_shot_leaves_no_stale_entry() {
        let timers: Arc<Mutex<HashMap<String, JoinHandle<()>>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = mpsc::channel::<()>(1);

        let body_timers = Arc::clone(&timers);
        let id = "timer-1".to_string();
        let body_id = id.clone();
        let body = async move {
            // Zero delay: the task is eligible to run (and self-remove)
            // immediately after being spawned.
            tokio::time::sleep(Duration::ZERO).await;
            let _ = tx.send(()).await;
            body_timers.lock().unwrap().remove(&body_id);
        };

        spawn_and_register(&timers, id, |handle| handle, body);

        // Receive the timer's "event", then let the task run its self-removal.
        rx.recv().await.expect("timer should fire its event");
        tokio::task::yield_now().await;

        assert!(timers.lock().unwrap().is_empty(), "map must not contain a stale entry after the one-shot timer fired");
    }

    #[tokio::test]
    async fn cancel_before_fire_delivers_no_event() {
        let timers: Arc<Mutex<HashMap<String, JoinHandle<()>>>> = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = mpsc::channel::<()>(1);

        let id = "timer-1".to_string();
        let body = async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let _ = tx.send(()).await;
        };

        spawn_and_register(&timers, id.clone(), |handle| handle, body);

        // Cancel exactly as `cancel_timer` does: remove from the map and abort.
        if let Some(handle) = timers.lock().unwrap().remove(&id) {
            handle.abort();
        }

        // Give the (aborted) task a chance to have run, then assert nothing was
        // delivered and the map is empty.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(rx.try_recv().is_err(), "cancelled timer must not deliver an event");
        assert!(timers.lock().unwrap().is_empty(), "cancelled timer must be removed from the map");
    }

    #[test]
    fn zero_repeat_becomes_one_shot() {
        let (duration, repeat) = normalize_timer_durations("t", Duration::from_secs(5), Some(Duration::ZERO));
        assert_eq!(duration, Duration::from_secs(5));
        assert_eq!(repeat, None);
    }

    #[test]
    fn huge_durations_are_clamped() {
        let (duration, repeat) = normalize_timer_durations("t", Duration::MAX, Some(Duration::MAX));
        assert_eq!(duration, MAX_TIMER_DURATION);
        assert_eq!(repeat, Some(MAX_TIMER_DURATION));
    }

    #[tokio::test]
    async fn clamped_repeat_interval_can_be_created() {
        let (_, repeat) = normalize_timer_durations("t", Duration::ZERO, Some(Duration::MAX));
        let mut interval = tokio::time::interval(repeat.unwrap());
        interval.tick().await;
    }
}
