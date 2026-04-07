use std::collections::HashMap;

use color_eyre::eyre::Result;
use config_watcher::{config_item_watcher::ConfigItemEvent, ConfigItemHash};
use homie5::{
    client::{Publish, QoS},
    extensions::meta::{MetaDeviceOverlay, MetaProviderProtocol},
    HomieDomain, HomieID,
};

use crate::{cfg_files_tracker::CfgFilesTracker, mqtt_client::ManagedMqttClient};

use super::model::MetaConfig;

pub struct MetaManager {
    homie_domain: HomieDomain,
    mqtt_client: ManagedMqttClient,
    files: CfgFilesTracker,
    configs: HashMap<ConfigItemHash, MetaConfig>,
    provider_to_hash: HashMap<HomieID, ConfigItemHash>,
}

impl MetaManager {
    pub fn new(homie_domain: HomieDomain, mqtt_client: ManagedMqttClient) -> Self {
        Self {
            homie_domain,
            mqtt_client,
            files: CfgFilesTracker::new(),
            configs: HashMap::new(),
            provider_to_hash: HashMap::new(),
        }
    }

    pub async fn handle_event(&mut self, event: ConfigItemEvent<MetaConfig>) -> Result<()> {
        match event {
            ConfigItemEvent::NewDocument(filename_hash, filename) => {
                log::info!("Meta config file discovered: {}", filename);
                self.files.add_file(filename_hash, filename).await;
            }
            ConfigItemEvent::RemoveDocument(filename_hash) => {
                let filename = self
                    .files
                    .get_file_name(&filename_hash)
                    .await
                    .unwrap_or_else(|| "unknown-document".to_string());
                log::info!("Meta config file removed: {}", filename);
                self.files.remove_file(&filename_hash).await;
            }
            ConfigItemEvent::New(hash, cfg) => {
                self.add_config(hash, cfg).await?;
            }
            ConfigItemEvent::Removed(hash) => {
                self.remove_config(hash).await?;
            }
        }

        Ok(())
    }

    pub async fn republish_all(&self) -> Result<()> {
        let mut configs: Vec<&MetaConfig> = self.configs.values().collect();
        configs.sort_by_key(|cfg| cfg.provider.id.as_str().to_string());
        log::info!("Republishing {} meta config item(s)", configs.len());
        for cfg in configs {
            self.publish_config(cfg).await?;
        }
        Ok(())
    }

    async fn add_config(&mut self, hash: ConfigItemHash, cfg: MetaConfig) -> Result<()> {
        let filename = self
            .files
            .get_file_name(&hash.filename_hash())
            .await
            .unwrap_or_else(|| "unknown-document".to_string());

        if let Some(existing_hash) = self.provider_to_hash.get(&cfg.provider.id) {
            if existing_hash != &hash {
                let filename = self
                    .files
                    .get_file_name(&hash.filename_hash())
                    .await
                    .unwrap_or_else(|| "unknown-document".to_string());
                let existing_filename = self
                    .files
                    .get_file_name(&existing_hash.filename_hash())
                    .await
                    .unwrap_or_else(|| "unknown-document".to_string());

                log::error!(
                    "Duplicate meta provider id detected [{}]. Ignoring config in [{}], provider is already defined in [{}]",
                    cfg.provider.id,
                    filename,
                    existing_filename,
                );
                return Ok(());
            }
        }

        self.publish_config(&cfg).await?;

        log::info!(
            "Meta config item loaded: provider [{}], devices [{}], file [{}]",
            cfg.provider.id,
            cfg.devices.len(),
            filename,
        );

        self.provider_to_hash.insert(cfg.provider.id.clone(), hash);
        self.configs.insert(hash, cfg);

        Ok(())
    }

    async fn remove_config(&mut self, hash: ConfigItemHash) -> Result<()> {
        let filename = self
            .files
            .get_file_name(&hash.filename_hash())
            .await
            .unwrap_or_else(|| "unknown-document".to_string());

        let Some(cfg) = self.configs.remove(&hash) else {
            log::debug!("Meta config item remove event ignored (not active): hash [{:?}], file [{}]", hash, filename,);
            return Ok(());
        };

        self.provider_to_hash.remove(&cfg.provider.id);
        self.unpublish_config(&cfg).await?;

        log::info!(
            "Meta config item removed: provider [{}], devices [{}], file [{}]",
            cfg.provider.id,
            cfg.devices.len(),
            filename,
        );

        Ok(())
    }

    async fn publish_config(&self, cfg: &MetaConfig) -> Result<()> {
        let provider_proto = MetaProviderProtocol::new(cfg.provider.id.clone(), self.homie_domain.clone());

        self.publish(provider_proto.publish_provider_info(&cfg.provider.info)?)
            .await?;

        for (device_id, meta_device) in &cfg.devices {
            let overlay = MetaDeviceOverlay {
                schema: 2,
                device: Some(meta_device.clone()),
            };
            self.publish(provider_proto.publish_device_overlay(device_id, &overlay)?)
                .await?;
        }

        Ok(())
    }

    async fn unpublish_config(&self, cfg: &MetaConfig) -> Result<()> {
        let provider_proto = MetaProviderProtocol::new(cfg.provider.id.clone(), self.homie_domain.clone());

        for device_id in cfg.devices.keys() {
            self.publish(provider_proto.remove_device_overlay(device_id)).await?;
        }
        self.publish(provider_proto.remove_provider_info()).await?;

        Ok(())
    }

    async fn publish(&self, msg: Publish) -> Result<()> {
        self.mqtt_client
            .publish(msg.topic, map_qos(&msg.qos), msg.retain, msg.payload)
            .await?;
        Ok(())
    }
}

fn map_qos(qos: &QoS) -> rumqttc::QoS {
    match qos {
        QoS::AtMostOnce => rumqttc::QoS::AtMostOnce,
        QoS::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
        QoS::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
    }
}
