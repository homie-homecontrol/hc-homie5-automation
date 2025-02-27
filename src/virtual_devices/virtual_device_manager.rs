use std::{collections::HashMap, sync::Arc};

use color_eyre::eyre::{self, eyre, Result};
use config_watcher::ConfigItemHash;
use hc_homie5::{homie_device, run_homie_client, HomieClientEvent, HomieClientHandle, HomieDevice, MqttClientConfig};
use homie5::{device_description::DeviceDescriptionBuilder, Homie5ControllerProtocol, HomieValue, PropertyRef};
use tokio::sync::{mpsc, RwLock};

use crate::{
    app_state::AppEvent, cfg_files_tracker::CfgFilesTracker, device_manager::DeviceManager,
    mqtt_client::ManagedMqttClient, settings::Settings,
};

use super::{property_indexer::PropertyIndexer, VirtualDevice, VirtualDeviceSpec};

#[homie_device]
#[derive(Clone)]
pub struct VirtualDeviceManager {
    devices: Arc<RwLock<HashMap<DeviceRef, VirtualDevice>>>,
    #[allow(dead_code)]
    ctrl_proto: Homie5ControllerProtocol,
    dm: DeviceManager,
    mqtt_client: ManagedMqttClient,
    app_event_sender: mpsc::Sender<AppEvent>,
    index: Arc<RwLock<PropertyIndexer>>,
    files: CfgFilesTracker,
}

impl VirtualDeviceManager {
    pub async fn new(
        dm: DeviceManager,
        mqtt_client: ManagedMqttClient,
        app_event_sender: mpsc::Sender<AppEvent>,
        settings: &Settings,
    ) -> Result<(Self, HomieClientHandle, mpsc::Receiver<HomieClientEvent>)> {
        log::debug!("HomieDomain: {:?}", settings.homie.homie_domain);

        let index = Arc::new(RwLock::new(PropertyIndexer::new()));

        let (homie_proto, last_will) =
            Homie5DeviceProtocol::new(settings.homie.controller_id.clone(), settings.homie.homie_domain.clone());

        let homie_client_options = MqttClientConfig::new(&settings.homie.hostname)
            .client_id(format!("{}-ctrl", &settings.homie.client_id))
            .port(settings.homie.port)
            .username(&settings.homie.username)
            .password(&settings.homie.password)
            .last_will(Some(last_will));

        let (device_client_handle, homie_client, homie_event_receiver) =
            run_homie_client(homie_client_options.to_mqtt_options(), homie_client_options.mqtt_channel_size)?;

        let device_desc = DeviceDescriptionBuilder::new()
            .name(settings.homie.controller_name.clone())
            .build();

        Ok((
            Self {
                status: HomieDeviceStatus::Init,
                ctrl_proto: Homie5ControllerProtocol::new(),
                device_ref: DeviceRef::new(settings.homie.homie_domain.clone(), settings.homie.controller_id.clone()),
                homie_proto,
                homie_client,
                devices: Arc::new(RwLock::new(HashMap::new())),
                device_desc,
                dm,
                mqtt_client,
                app_event_sender,
                index,
                files: CfgFilesTracker::new(),
            },
            device_client_handle,
            homie_event_receiver,
        ))
    }

    pub async fn add_device(&mut self, hash: ConfigItemHash, spec: VirtualDeviceSpec) -> Result<DeviceRef> {
        log::debug!("Adding virtual device: {} -- root state: {}", spec.id, self.status);
        let vdevices = self.devices.read().await;
        let device_ref = DeviceRef::new(self.homie_domain().clone(), spec.id.clone());
        if vdevices.contains_key(&device_ref) {
            let filename = self
                .get_cfg_filename(&hash)
                .await
                .unwrap_or("unkown-document".to_string());
            return Err(eyre!(format!(
                "Duplicate Virtual device defined! ID: {} ({}) - Config Document: {}",
                device_ref.device_id(),
                device_ref.homie_domain().as_str(),
                filename
            )));
        }
        drop(vdevices);
        let devices = self.dm.read().await;

        let mut dev = VirtualDevice::new(
            device_ref,
            hash,
            spec,
            self.device_ref.clone(),
            self.homie_proto(),
            &self.homie_client,
            &devices,
            &self.mqtt_client,
            self.app_event_sender.clone(),
        )
        .await?;
        let mut vdevices = self.devices.write().await;
        {
            let mut index = self.index.write().await;
            index.add_index_for_virtual_device(&dev);
        }
        if !dev.mqtt_reads() {
            // when no mqtt reads are open, publish the complete device and set it to ready.
            dev.publish_device().await?;
        } else {
            // when we need to wait for mqtt readings we only publish a init state
            dev.publish_state().await?;
        }

        let vdev_ref = dev.device_ref().clone();
        vdevices.insert(dev.device_ref().clone(), dev);
        drop(vdevices);
        drop(devices);
        self.add_child_device(vdev_ref.device_id().clone()).await?;
        Ok(vdev_ref)
    }

    pub async fn remove_device(&mut self, hash: ConfigItemHash) -> Result<Option<VirtualDevice>> {
        let mut vdevices = self.devices.write().await;
        let Some(id) = vdevices
            .iter()
            .find(|(_, v)| v.spec_hash() == hash)
            .map(|(k, _)| k.clone())
        else {
            return Ok(None);
        };

        if let Some(mut dev) = vdevices.remove(&id) {
            log::debug!("Removed vdevice, cancel all read tasks: {}", dev.homie_id(),);
            dev.cancel_all_mqtt_reads().await?;
            dev.disconnect_device().await?;
            {
                let mut index = self.index.write().await;
                index.remove_indexes_for_virtual_device(&id);
            }
            drop(vdevices);
            self.remove_child_device(dev.device_ref().device_id()).await?;
            return Ok(Some(dev));
        }

        log::debug!("VDevice removed:{}", hash);
        Ok(None)
    }

    pub async fn add_child_device(&mut self, child_id: HomieID) -> Result<()> {
        self.device_desc.add_child(child_id);
        self.device_desc.update_version();

        self.status = HomieDeviceStatus::Init;
        self.publish_state().await?;
        self.publish_description().await?;
        self.status = HomieDeviceStatus::Ready;
        self.publish_state().await?;

        Ok(())
    }

    pub async fn remove_child_device(&mut self, child_id: &HomieID) -> Result<()> {
        self.device_desc.remove_child(child_id);
        self.device_desc.update_version();

        self.status = HomieDeviceStatus::Init;
        self.publish_state().await?;
        self.publish_description().await?;
        self.status = HomieDeviceStatus::Ready;
        self.publish_state().await?;

        Ok(())
    }

    pub async fn publish_child_devices(&self) -> Result<()> {
        let mut vdevices = self.devices.write().await;
        for (_, device) in vdevices.iter_mut() {
            device.publish_device().await?;
        }
        Ok(())
    }

    pub async fn update_compound_members_removed(&self, device_ref: &DeviceRef) -> Result<bool> {
        let mut vdevices = self.devices.write().await;
        let mut index = self.index.write().await;
        for (_, vdevice) in vdevices.iter_mut() {
            let changed = vdevice.update_compound_members_removed(device_ref).await?;
            if changed {
                for (_, vprop) in vdevice.properties.iter() {
                    index.update_index_for_virtual_prop(vprop);
                }
            }
        }
        Ok(true)
    }

    pub async fn update_compound_members(&self, device_ref: &DeviceRef, desc: &HomieDeviceDescription) -> Result<bool> {
        let mut vdevices = self.devices.write().await;
        let mut index = self.index.write().await;
        let devices = self.dm.read().await;
        for (_, vdevice) in vdevices.iter_mut().filter(|(_, vd)| vd.has_queries()) {
            let changed = vdevice.update_compound_members(device_ref, desc, &devices).await?;
            if changed {
                for (_, vprop) in vdevice.properties.iter().filter(|(_, p)| p.has_queries()) {
                    index.update_index_for_virtual_prop(vprop);
                }
            }
        }
        Ok(true)
    }

    pub async fn update_member_value_prop(&self, prop: &PropertyRef, value: &HomieValue) -> Result<()> {
        let mut vdevices = self.devices.write().await;
        let index = self.index.read().await;

        let indexes = index.lookup_index(prop);
        if let Some(indexes) = indexes {
            for index in indexes.iter() {
                if let Some(vdevice) = vdevices.get_mut(index.device_ref()) {
                    vdevice.update_member_value_prop(prop, value).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn update_member_value_mqtt(&self, topic: &str, value: &str) -> Result<()> {
        let mut vdevices = self.devices.write().await;
        for (_, vdevice) in vdevices.iter_mut() {
            vdevice.update_member_value_mqtt(topic, value).await?;
        }
        Ok(())
    }
    pub async fn update_value(&mut self, prop: &PropertyRef) -> Result<(), eyre::Error> {
        let mut vdevices = self.devices.write().await;
        let devices = self.dm.read().await;
        if let Some(vdev) = vdevices.get_mut(prop.device_ref()) {
            vdev.update_value(prop, &devices).await?;
        }
        Ok(())
    }

    pub async fn handle_mqtt_read(&self, prop: &PropertyRef, value: &str) -> Result<()> {
        let mut vdevices = self.devices.write().await;
        if let Some(vdev) = vdevices.get_mut(prop.device_ref()) {
            vdev.handle_mqtt_read(prop, value).await?;
        }
        Ok(())
    }

    pub async fn cancel_mqtt_read(&self, prop: &PropertyRef) -> Result<()> {
        let mut vdevices = self.devices.write().await;
        if let Some(vdev) = vdevices.get_mut(prop.device_ref()) {
            vdev.cancel_mqtt_read(prop).await?;
        }
        Ok(())
    }
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, HashMap<DeviceRef, VirtualDevice>> {
        self.devices.read().await
    }

    #[allow(dead_code)]
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, HashMap<DeviceRef, VirtualDevice>> {
        self.devices.write().await
    }

    pub async fn disconnect_virtual_devices(&mut self) -> Result<()> {
        let mut devices = self.devices.write().await;

        for (_, device) in devices.iter_mut() {
            device.disconnect_device().await?;
        }
        devices.clear(); // not sure about this
        Ok(())
    }
    pub async fn disconnect_client(&self) -> Result<()> {
        self.homie_client.disconnect().await?;
        Ok(())
    }

    pub fn as_proxy(&self) -> VirtualDeviceManagerProxy {
        VirtualDeviceManagerProxy::new(Arc::clone(&self.devices))
    }

    pub async fn add_cfg_file(&mut self, hash: u64, filename: String) {
        self.files.add_file(hash, filename).await;
    }

    pub async fn remove_cfg_file(&mut self, hash: &u64) {
        self.files.remove_file(hash).await;
    }

    pub async fn get_cfg_filename(&self, hash: &ConfigItemHash) -> Option<String> {
        self.files.get_file_name(&hash.filename_hash()).await
    }
}

impl HomieDevice for VirtualDeviceManager {
    type ResultError = eyre::Error;

    async fn handle_set_command(&mut self, property: &PropertyRef, set_value: &str) -> Result<(), Self::ResultError> {
        if property.device_ref() == &self.device_ref {
            // handle future property set commands for the root device here
        } else {
            // pass on to the child devices
            let mut vdevices = self.devices.write().await;
            if let Some(vdev) = vdevices.get_mut(property.device_ref()) {
                vdev.handle_set_command(property, set_value).await?;
            }
        }
        Ok(())
    }
}

pub struct VirtualDeviceManagerProxy(Arc<RwLock<HashMap<DeviceRef, VirtualDevice>>>);

impl VirtualDeviceManagerProxy {
    pub fn new(d: Arc<RwLock<HashMap<DeviceRef, VirtualDevice>>>) -> Self {
        Self(d)
    }

    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, HashMap<DeviceRef, VirtualDevice>> {
        self.0.read().await
    }

    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, HashMap<DeviceRef, VirtualDevice>> {
        self.0.write().await
    }
    pub async fn set_value(&self, prop: &PropertyRef, value: HomieValue) -> Result<()> {
        let mut vdevices = self.0.write().await;
        if let Some(vdev) = vdevices.get_mut(prop.device_ref()) {
            vdev.set_value(prop, value).await?;
        }
        Ok(())
    }
    pub async fn set_str_value(&self, prop: &PropertyRef, value: &str) -> Result<()> {
        let mut vdevices = self.0.write().await;
        if let Some(vdev) = vdevices.get_mut(prop.device_ref()) {
            vdev.set_str_value(prop, value).await?;
        }
        Ok(())
    }
    pub async fn set_command(&self, property: &PropertyRef, value: HomieValue) -> Result<()> {
        let mut vdevices = self.0.write().await;
        if let Some(vdev) = vdevices.get_mut(property.device_ref()) {
            vdev.simulate_set_command(property, value).await?;
        }
        Ok(())
    }
}
