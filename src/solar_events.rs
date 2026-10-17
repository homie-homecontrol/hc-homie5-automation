use std::collections::HashMap;
use std::hash::Hash;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use color_eyre::eyre::Result;
use config_watcher::ConfigItemHash;
use hc_homie5::logging::chain;
use hc_homie5::value::ValueMatcher;
use serde::{Deserialize, Serialize};
use sun::SunPhase;
use tokio::sync::{mpsc, watch};

use crate::panic_guard::contain;

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SolarPhase {
    Sunrise,
    Sunset,
    SunriseEnd,
    SunsetStart,
    Dawn,
    Dusk,
    NauticalDawn,
    NauticalDusk,
    NightEnd,
    Night,
    GoldenHourEnd,
    GoldenHour,
    Custom(f64, bool),
}

impl From<&SunPhase> for SolarPhase {
    fn from(phase: &SunPhase) -> Self {
        match phase {
            SunPhase::Sunrise => SolarPhase::Sunrise,
            SunPhase::Sunset => SolarPhase::Sunset,
            SunPhase::SunriseEnd => SolarPhase::SunriseEnd,
            SunPhase::SunsetStart => SolarPhase::SunsetStart,
            SunPhase::Dawn => SolarPhase::Dawn,
            SunPhase::Dusk => SolarPhase::Dusk,
            SunPhase::NauticalDawn => SolarPhase::NauticalDawn,
            SunPhase::NauticalDusk => SolarPhase::NauticalDusk,
            SunPhase::NightEnd => SolarPhase::NightEnd,
            SunPhase::Night => SolarPhase::Night,
            SunPhase::GoldenHourEnd => SolarPhase::GoldenHourEnd,
            SunPhase::GoldenHour => SolarPhase::GoldenHour,
            SunPhase::Custom(d, b) => SolarPhase::Custom(*d, *b),
        }
    }
}

impl From<&SolarPhase> for SunPhase {
    fn from(phase: &SolarPhase) -> Self {
        match phase {
            SolarPhase::Sunrise => SunPhase::Sunrise,
            SolarPhase::Sunset => SunPhase::Sunset,
            SolarPhase::SunriseEnd => SunPhase::SunriseEnd,
            SolarPhase::SunsetStart => SunPhase::SunsetStart,
            SolarPhase::Dawn => SunPhase::Dawn,
            SolarPhase::Dusk => SunPhase::Dusk,
            SolarPhase::NauticalDawn => SunPhase::NauticalDawn,
            SolarPhase::NauticalDusk => SunPhase::NauticalDusk,
            SolarPhase::NightEnd => SunPhase::NightEnd,
            SolarPhase::Night => SunPhase::Night,
            SolarPhase::GoldenHourEnd => SunPhase::GoldenHourEnd,
            SolarPhase::GoldenHour => SunPhase::GoldenHour,
            SolarPhase::Custom(d, b) => SunPhase::Custom(*d, *b),
        }
    }
}

impl ValueMatcher for SolarPhase {
    fn as_match_str(&self) -> &str {
        match self {
            SolarPhase::Sunrise => "Sunrise",
            SolarPhase::Sunset => "Sunset",
            SolarPhase::SunriseEnd => "SunriseEnd",
            SolarPhase::SunsetStart => "SunsetStart",
            SolarPhase::Dawn => "Dawn",
            SolarPhase::Dusk => "Dusk",
            SolarPhase::NauticalDawn => "NauticalDawn",
            SolarPhase::NauticalDusk => "NauticalDusk",
            SolarPhase::NightEnd => "NightEnd",
            SolarPhase::Night => "Night",
            SolarPhase::GoldenHourEnd => "GoldenHourEnd",
            SolarPhase::GoldenHour => "GoldenHour",
            SolarPhase::Custom(_d, _b) => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SolarEventTrigger {
    pub rule_hash: ConfigItemHash, // Associated rule identifier
    pub event: SolarEvent,         // Type of solar event trigger
}

#[derive(Debug, Clone)]
pub enum SolarEvent {
    At(SolarPhase),               // Trigger at a specific solar event
    After(SolarPhase, Duration),  // Trigger relative to a solar event
    Before(SolarPhase, Duration), // Trigger relative to a solar event
}

#[derive(Debug)]
pub enum TriggerUpdate {
    Add(SolarEventTrigger),       // Add a single trigger
    RemoveByRule(ConfigItemHash), // Remove all triggers for a specific rule
}

#[derive(Debug)]
pub struct SolarEventHandle {
    stop_sender: watch::Sender<bool>, // Shutdown signal
    handle: tokio::task::JoinHandle<()>,
}

impl SolarEventHandle {
    /// Stops the solar event task.
    pub async fn stop(self) {
        let _ = self.stop_sender.send(true); // Send the shutdown signal
        let _ = self.handle.await; // Await the task's completion
    }
}

#[derive(Debug)]
pub struct SolarEventManager {
    updates: mpsc::Sender<TriggerUpdate>, // Channel for dynamic updates
}

impl SolarEventManager {
    pub async fn add_trigger(&self, trigger: SolarEventTrigger) -> Result<()> {
        self.updates.send(TriggerUpdate::Add(trigger)).await?;
        Ok(())
    }

    pub async fn remove_triggers_by_rule(&self, rule_hash: ConfigItemHash) -> Result<()> {
        self.updates.send(TriggerUpdate::RemoveByRule(rule_hash)).await?;
        Ok(())
    }
}
/// Starts a solar event task that emits events for all phases.
///
/// # Arguments
/// - `latitude`: Latitude for the solar calculation.
/// - `longitude`: Longitude for the solar calculation.
/// - `height`: Observer height in meters above sea level.
/// - `channel_size`: Size of the channel buffer for events.
///
/// # Returns
/// A tuple containing the `SolarEventHandle` to stop the task and a receiver for solar events.
pub fn run_solar_event_task(
    latitude: f64,
    longitude: f64,
    height: f64,
    channel_size: usize,
) -> (SolarEventHandle, SolarEventManager, mpsc::Receiver<SolarEvent>) {
    let (event_sender, event_receiver) = mpsc::channel(channel_size);
    let (updates_sender, mut updates_receiver) = mpsc::channel(channel_size);
    let (stop_sender, mut stop_receiver) = watch::channel(false);

    let handle = tokio::spawn(async move {
        let solar_task = async move {
            let mut scheduler = SolarScheduler::new(latitude, longitude, height, Utc::now());
            let mut location_warned = false;

            loop {
                let delay = scheduler
                    .next_due()
                    .map(|time| (time - Utc::now()).to_std().unwrap_or(Duration::ZERO))
                    .unwrap_or(Duration::from_secs(60)); // Default sleep if no event

                tokio::select! {
                    // Handle trigger updates
                    Some(update) = updates_receiver.recv() => {
                        // fire what became due before the update interrupted the sleep
                        send_due(&mut scheduler, &event_sender).await;
                        match update {
                            TriggerUpdate::Add(trigger) => {
                                scheduler.add(trigger.rule_hash, trigger.event);
                                if !location_warned && scheduler.location_unset() {
                                    tracing::warn!(
                                        setting = "HCACTL_LOCATION",
                                        "solar triggers configured but the location is unset (0,0,0)"
                                    );
                                    location_warned = true;
                                }
                            }
                            TriggerUpdate::RemoveByRule(rule_hash) => {
                                scheduler.remove_by_rule(&rule_hash);
                            }
                        }
                    }

                    // Stop signal received
                    _ = stop_receiver.changed() => {
                        if *stop_receiver.borrow() {
                            tracing::debug!("solar event task stopping");
                            break;
                        }
                    }

                    // Wait for the next event or default delay
                    _ = tokio::time::sleep(delay) => {
                        send_due(&mut scheduler, &event_sender).await;
                    }
                }
            }
        };
        if let Err(panic) = contain(solar_task).await {
            tracing::error!(error = chain(&panic), "solar event task panicked; solar triggers stop until restart");
        }
    });

    (
        SolarEventHandle { stop_sender, handle },
        SolarEventManager {
            updates: updates_sender,
        },
        event_receiver,
    )
}

async fn send_due(scheduler: &mut SolarScheduler<ConfigItemHash>, event_sender: &mpsc::Sender<SolarEvent>) {
    for event in scheduler.take_due(Utc::now()) {
        tracing::trace!(event = ?event, "solar event due");
        if let Err(err) = event_sender.send(event.clone()).await {
            tracing::debug!(event = ?event, error = chain(&err), "cannot deliver solar event");
        }
    }
}

/// How many days ahead [`next_event_time`] looks for a solar event.
const MAX_SOLAR_SEARCH_DAYS: usize = 400;

fn offset_millis(offset: &Duration) -> i64 {
    i64::try_from(offset.as_millis()).unwrap_or(i64::MAX)
}

/// Next time after `now` at which `event` occurs, searching at most [`MAX_SOLAR_SEARCH_DAYS`].
/// A phase the sun never reaches at this location never yields a future time, so the search is
/// bounded instead of walking through the whole calendar.
fn next_event_time(
    event: &SolarEvent,
    now: DateTime<Utc>,
    latitude: f64,
    longitude: f64,
    height: f64,
) -> Option<DateTime<Utc>> {
    let mut day = now.naive_utc().date();
    for _ in 0..MAX_SOLAR_SEARCH_DAYS {
        let today_start_ms = day.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis();

        // Calculate the event time
        let event_time_ms = match event {
            SolarEvent::At(phase) => {
                sun::time_at_phase(today_start_ms, SunPhase::from(phase), latitude, longitude, height)
            }
            SolarEvent::After(phase, offset) => {
                let base_time = sun::time_at_phase(today_start_ms, SunPhase::from(phase), latitude, longitude, height);
                base_time.saturating_add(offset_millis(offset))
            }
            SolarEvent::Before(phase, offset) => {
                let base_time = sun::time_at_phase(today_start_ms, SunPhase::from(phase), latitude, longitude, height);
                base_time.saturating_sub(offset_millis(offset))
            }
        };

        // Convert the calculated time to a DateTime<Utc>; skip invalid times
        let event_time = Utc.timestamp_millis_opt(event_time_ms).single()?;
        if event_time > now {
            return Some(event_time);
        }
        // The event time is in the past, move to the next day
        day = day.succ_opt()?;
    }
    None
}

/// Schedules solar triggers against a watermark: every trigger whose occurrence lies after the
/// last processed instant and at or before `now` fires exactly once, so neither simultaneous
/// triggers nor an interrupted sleep can skip an occurrence.
#[derive(Debug)]
struct SolarScheduler<K> {
    triggers: HashMap<K, Vec<SolarEvent>>,
    processed_up_to: DateTime<Utc>,
    latitude: f64,
    longitude: f64,
    height: f64,
}

impl<K: Eq + Hash> SolarScheduler<K> {
    fn new(latitude: f64, longitude: f64, height: f64, start: DateTime<Utc>) -> Self {
        Self {
            triggers: HashMap::new(),
            processed_up_to: start,
            latitude,
            longitude,
            height,
        }
    }

    fn add(&mut self, rule: K, event: SolarEvent) {
        self.triggers.entry(rule).or_default().push(event);
    }

    fn remove_by_rule(&mut self, rule: &K) {
        self.triggers.remove(rule);
    }

    /// The configured location is the 0,0,0 default.
    fn location_unset(&self) -> bool {
        self.latitude == 0.0 && self.longitude == 0.0 && self.height == 0.0
    }

    fn events(&self) -> impl Iterator<Item = &SolarEvent> {
        self.triggers.values().flatten()
    }

    fn next_time(&self, event: &SolarEvent) -> Option<DateTime<Utc>> {
        next_event_time(event, self.processed_up_to, self.latitude, self.longitude, self.height)
    }

    /// Earliest occurrence that has not been processed yet.
    fn next_due(&self) -> Option<DateTime<Utc>> {
        self.events().filter_map(|event| self.next_time(event)).min()
    }

    /// Returns every trigger due up to `now` and marks that span as processed.
    fn take_due(&mut self, now: DateTime<Utc>) -> Vec<SolarEvent> {
        if now <= self.processed_up_to {
            return Vec::new();
        }
        let due = self
            .events()
            .filter(|event| self.next_time(event).is_some_and(|time| time <= now))
            .cloned()
            .collect();
        self.processed_up_to = now;
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Munich
    const LATITUDE: f64 = 48.166;
    const LONGITUDE: f64 = 11.5683;

    fn sunset_after(t: DateTime<Utc>) -> DateTime<Utc> {
        next_event_time(&SolarEvent::At(SolarPhase::Sunset), t, LATITUDE, LONGITUDE, 519.0).unwrap()
    }

    fn scheduler(start: DateTime<Utc>) -> SolarScheduler<u64> {
        SolarScheduler::new(LATITUDE, LONGITUDE, 519.0, start)
    }

    #[test]
    fn triggers_due_in_the_same_second_all_fire() {
        let sunset = sunset_after(Utc::now());
        let mut sched = scheduler(sunset - chrono::Duration::seconds(10));
        sched.add(1, SolarEvent::At(SolarPhase::Sunset));
        sched.add(2, SolarEvent::After(SolarPhase::Sunset, Duration::from_millis(500)));

        let due = sched.take_due(sunset + chrono::Duration::seconds(1));

        assert_eq!(due.len(), 2, "{due:?}");
        assert!(sched.take_due(sunset + chrono::Duration::seconds(2)).is_empty(), "fired twice");
        assert!(sched.next_due().unwrap() > sunset + chrono::Duration::hours(12));
    }

    #[test]
    fn update_just_before_due_time_does_not_skip_the_trigger() {
        let sunset = sunset_after(Utc::now());
        let mut sched = scheduler(sunset - chrono::Duration::seconds(10));
        sched.add(1, SolarEvent::At(SolarPhase::Sunset));
        assert_eq!(sched.next_due(), Some(sunset));

        // an update arrives 100 ms before sunset and interrupts the sleep
        assert!(sched.take_due(sunset - chrono::Duration::milliseconds(100)).is_empty());
        sched.add(2, SolarEvent::At(SolarPhase::Dusk));

        let due = sched.take_due(sunset + chrono::Duration::milliseconds(5));
        assert!(matches!(&due[..], [SolarEvent::At(SolarPhase::Sunset)]), "{due:?}");
    }

    #[test]
    fn removed_triggers_do_not_fire() {
        let sunset = sunset_after(Utc::now());
        let mut sched = scheduler(sunset - chrono::Duration::seconds(10));
        sched.add(1, SolarEvent::At(SolarPhase::Sunset));
        sched.remove_by_rule(&1);
        assert!(sched.take_due(sunset + chrono::Duration::seconds(1)).is_empty());
        assert_eq!(sched.next_due(), None);
    }

    #[test]
    fn default_location_is_reported_unset() {
        assert!(SolarScheduler::<u64>::new(0.0, 0.0, 0.0, Utc::now()).location_unset());
        assert!(!scheduler(Utc::now()).location_unset());
    }

    #[test]
    fn next_sunset_is_within_a_day() {
        let now = Utc::now();
        let time = next_event_time(&SolarEvent::At(SolarPhase::Sunset), now, LATITUDE, LONGITUDE, 519.0)
            .expect("the sun sets every day in Munich");
        assert!(time > now && time - now <= chrono::Duration::hours(25));
    }

    #[test]
    fn unreachable_phase_yields_no_time() {
        // The sun never stands 89 degrees above the horizon at this latitude.
        let event = SolarEvent::At(SolarPhase::Custom(89.0, true));
        let started = std::time::Instant::now();
        assert_eq!(next_event_time(&event, Utc::now(), LATITUDE, LONGITUDE, 519.0), None);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn huge_offsets_do_not_overflow() {
        let event = SolarEvent::After(SolarPhase::Sunrise, Duration::MAX);
        assert_eq!(next_event_time(&event, Utc::now(), LATITUDE, LONGITUDE, 519.0), None);
        let event = SolarEvent::Before(SolarPhase::Sunrise, Duration::MAX);
        assert_eq!(next_event_time(&event, Utc::now(), LATITUDE, LONGITUDE, 519.0), None);
    }
}
