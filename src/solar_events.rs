use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use color_eyre::eyre::Result;
use config_watcher::ConfigItemHash;
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
            let mut triggers: HashMap<ConfigItemHash, Vec<SolarEventTrigger>> = HashMap::new(); // Rule-hash -> Triggers

            loop {
                let next_event = calculate_next_event(&triggers, latitude, longitude, height);

                let delay = next_event
                    .as_ref()
                    .map(|(_, delay)| *delay)
                    .unwrap_or(Duration::from_secs(60)); // Default sleep if no event

                tokio::select! {
                    // Handle trigger updates
                    Some(update) = updates_receiver.recv() => {
                        match update {
                            TriggerUpdate::Add(trigger) => {
                                triggers.entry(trigger.rule_hash)
                                    .or_default()
                                    .push(trigger);
                            }
                            TriggerUpdate::RemoveByRule(rule_hash) => {
                                triggers.remove(&rule_hash);
                            }
                        }
                    }

                    // Stop signal received
                    _ = stop_receiver.changed() => {
                        if *stop_receiver.borrow() {
                            log::debug!("Stopping solar event task...");
                            break;
                        }
                    }

                    // Wait for the next event or default delay
                    _ = tokio::time::sleep(delay) => {
                            if let Some((trigger, _)) = next_event {
                                log::debug!("Triggering event for rule: {}", trigger.rule_hash);
                                if let Err(err) = event_sender.send(trigger.event.clone()).await {
                                    log::warn!("Error sending solar event trigger {:?}: {:#?}", trigger, err);
                                }
                            }
                    }
                }
            }
        };
        if let Err(panic) = contain(solar_task).await {
            log::error!("Solar event task panicked; solar triggers stop until restart: {}", panic);
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

/// How many days ahead [`calculate_next_event`] looks for a solar event.
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

fn calculate_next_event(
    triggers: &'_ HashMap<ConfigItemHash, Vec<SolarEventTrigger>>,
    latitude: f64,
    longitude: f64,
    height: f64,
) -> Option<(&'_ SolarEventTrigger, Duration)> {
    let now = Utc::now();

    // Iterate through all triggers and calculate the next valid event
    let mut next_events = triggers
        .values()
        .flat_map(|rule_triggers| rule_triggers.iter())
        .filter_map(|trigger| {
            next_event_time(&trigger.event, now, latitude, longitude, height).map(|time| (trigger, time))
        })
        .collect::<Vec<_>>();

    // Find the next event (closest future event)
    next_events.sort_by_key(|(_, time)| *time);

    // Map to the first event with a valid delay
    next_events
        .into_iter()
        .filter_map(|(trigger, time)| {
            let delay = (time - now).to_std().ok()?;
            Some((trigger, delay))
        })
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Munich
    const LATITUDE: f64 = 48.166;
    const LONGITUDE: f64 = 11.5683;

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
