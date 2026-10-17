use crate::{
    cron_manager::CronManager,
    device_manager::DeviceManager,
    mqtt_client::ManagedMqttClient,
    rules::{
        add_solar_triggers, queries_init_materialized, schedule_cron, self_triggering_mqtt_actions,
        subscribe_mqtt_trigger, unsubscribe_mqtt_trigger, FiringDecision, FiringLimiter, Rule,
    },
    solar_events::SolarEventManager,
    timer_manager::TimerManager,
    virtual_devices::VirtualDevice,
};
use color_eyre::eyre::Result;
use config_watcher::ConfigItemHash;
use homie5::{device_description::HomieDeviceDescription, DeviceRef};
use std::{
    cell::RefCell,
    collections::HashMap,
    ops::{Deref, DerefMut},
    time::Instant,
};

#[derive(Default)]
pub struct RuleManager {
    rules: HashMap<ConfigItemHash, Rule>,
    files: HashMap<u64, String>,
    /// Per-rule firing rate limit; interior mutability because rules fire through `&RuleManager`
    /// on the single-threaded event loop.
    firing_limiter: RefCell<FiringLimiter<ConfigItemHash>>,
}

impl Deref for RuleManager {
    type Target = HashMap<ConfigItemHash, Rule>;

    fn deref(&self) -> &Self::Target {
        &self.rules
    }
}

impl DerefMut for RuleManager {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rules
    }
}

impl RuleManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// `max_firings_per_sec == 0` disables the per-rule firing rate limit.
    pub fn with_max_firings_per_sec(max_firings_per_sec: u32) -> Self {
        Self {
            firing_limiter: RefCell::new(FiringLimiter::new(max_firings_per_sec)),
            ..Self::default()
        }
    }

    /// Records a firing of rule `hash` and decides whether it is within the rate limit.
    pub fn check_firing(&self, hash: ConfigItemHash, now: Instant) -> FiringDecision {
        self.firing_limiter.borrow_mut().check(hash, now)
    }

    pub fn add_rule_file(&mut self, hash: u64, filename: String) {
        self.files.insert(hash, filename);
    }

    pub fn remove_rule_file(&mut self, hash: u64) {
        self.files.remove(&hash);
    }

    pub fn get_filename(&self, hash: ConfigItemHash) -> Option<&String> {
        self.files.get(&hash.filename_hash())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_rule(
        &mut self,
        hash: ConfigItemHash,
        rule: Rule,
        cron: &CronManager,
        mqtt_client: &ManagedMqttClient,
        solar_events: &SolarEventManager,
        dm: &DeviceManager,
        vds: &HashMap<DeviceRef, VirtualDevice>,
    ) -> Result<&mut Rule> {
        let rule = self.rules.entry(hash).or_insert(rule);
        for (action_topic, trigger_filter) in self_triggering_mqtt_actions(rule) {
            tracing::warn!(
                rule = %rule.name,
                action_topic,
                trigger_filter,
                "mqtt action publishes into the rule's own trigger topic; the rule triggers itself"
            );
        }
        schedule_cron(hash, rule, cron);
        subscribe_mqtt_trigger(rule, mqtt_client).await?;
        add_solar_triggers(hash, rule, solar_events).await;
        queries_init_materialized(rule, &*dm.read().await, vds);
        Ok(rule)
    }

    pub async fn remove_rule(
        &mut self,
        hash: ConfigItemHash,
        cron: &CronManager,
        mqtt_client: &ManagedMqttClient,
        solar_events: &SolarEventManager,
        timers: &TimerManager,
    ) -> Result<Option<Rule>> {
        if let Some(rule) = self.rules.remove(&hash) {
            self.firing_limiter.get_mut().remove(&hash);
            timers.remove_timers_for_rule(hash);
            cron.remove_cron_schedule_for_rule(hash);
            solar_events.remove_triggers_by_rule(hash).await?;
            unsubscribe_mqtt_trigger(&rule, mqtt_client).await?;
            Ok(Some(rule))
        } else {
            Ok(None)
        }
    }

    pub fn queries_device_updated(&mut self, device_ref: &DeviceRef, desc: Option<&HomieDeviceDescription>) {
        let Some(desc) = desc else {
            return;
        };
        for (_, rule) in self.iter_mut() {
            for trigger in rule.triggers.iter_mut() {
                match trigger {
                    crate::rules::RuleTrigger::PropertyTriggered { ref mut queries, .. } => {
                        for query in queries.iter_mut() {
                            query.add_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                        }
                    }
                    crate::rules::RuleTrigger::PropertyChanged { ref mut queries, .. } => {
                        for query in queries.iter_mut() {
                            query.add_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                        }
                    }

                    _ => {}
                }
            }
        }
    }
    pub fn queries_device_removed(&mut self, device_ref: &DeviceRef, desc: Option<&HomieDeviceDescription>) {
        let Some(desc) = desc else {
            return;
        };
        for (_, rule) in self.iter_mut() {
            for trigger in rule.triggers.iter_mut() {
                match trigger {
                    crate::rules::RuleTrigger::PropertyTriggered { ref mut queries, .. } => {
                        for query in queries.iter_mut() {
                            query.remove_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                        }
                    }
                    crate::rules::RuleTrigger::PropertyChanged { ref mut queries, .. } => {
                        for query in queries.iter_mut() {
                            query.remove_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                        }
                    }

                    _ => {}
                }
            }
        }
    }
    pub fn queries_virtual_device_updated(&mut self, device_ref: &DeviceRef, desc: &HomieDeviceDescription) {
        for (_, rule) in self.iter_mut() {
            for trigger in rule.triggers.iter_mut() {
                if let crate::rules::RuleTrigger::OnSetEventTrigger {
                    queries: ref mut on_set_queries,
                    ..
                } = trigger
                {
                    for query in on_set_queries.iter_mut() {
                        query.add_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                    }
                }
            }
        }
    }
    pub fn queries_virtual_device_removed(&mut self, device_ref: &DeviceRef, desc: &HomieDeviceDescription) {
        for (_, rule) in self.iter_mut() {
            for trigger in rule.triggers.iter_mut() {
                if let crate::rules::RuleTrigger::OnSetEventTrigger {
                    queries: ref mut on_set_queries,
                    ..
                } = trigger
                {
                    for query in on_set_queries.iter_mut() {
                        query.remove_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                    }
                }
            }
        }
    }
}
