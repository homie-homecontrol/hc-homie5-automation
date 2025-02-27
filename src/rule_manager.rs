use crate::{
    cron_manager::CronManager,
    device_manager::DeviceManager,
    mqtt_client::ManagedMqttClient,
    rules::{
        add_solar_triggers, queries_init_materialized, schedule_cron, subscribe_mqtt_trigger, unsubscribe_mqtt_trigger,
        Rule,
    },
    solar_events::SolarEventManager,
    timer_manager::TimerManager,
    virtual_devices::VirtualDevice,
};
use color_eyre::eyre::Result;
use config_watcher::ConfigItemHash;
use homie5::{device_description::HomieDeviceDescription, DeviceRef};
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

#[derive(Default)]
pub struct RuleManager {
    rules: HashMap<ConfigItemHash, Rule>,
    files: HashMap<u64, String>,
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
        Self {
            rules: HashMap::new(),
            files: HashMap::new(),
        }
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
        let rule = self.entry(hash).or_insert(rule);
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
        if let Some(rule) = self.remove(&hash) {
            timers.remove_timers_for_rule(hash);
            cron.remove_cron_schedule_for_rule(hash);
            solar_events.remove_triggers_by_rule(hash).await?;
            unsubscribe_mqtt_trigger(&rule, mqtt_client).await?;
            // queries_remove_init_materialized(&mut rule, &*dm.read().await);
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
                    crate::rules::RuleTrigger::SubjectTriggered { ref mut queries, .. } => {
                        for query in queries.iter_mut() {
                            query.add_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                        }
                    }
                    crate::rules::RuleTrigger::SubjectChanged { ref mut queries, .. } => {
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
                    crate::rules::RuleTrigger::SubjectTriggered { ref mut queries, .. } => {
                        for query in queries.iter_mut() {
                            query.remove_materialized(device_ref.homie_domain(), device_ref.device_id(), desc);
                        }
                    }
                    crate::rules::RuleTrigger::SubjectChanged { ref mut queries, .. } => {
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
