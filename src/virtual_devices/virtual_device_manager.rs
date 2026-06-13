use std::{collections::HashMap, sync::Arc, time::Duration};

use color_eyre::eyre::{self, eyre, Result};
use config_watcher::ConfigItemHash;
use hc_homie5::client::{
    run_homie_client, FlushTimeout, HomieClientEvent, HomieClientHandle, MqttClientConfig, PendingPublishObserver,
};
use hc_homie5::device::HomieDevice;
use hc_homie5::homie_device;
use hc_homie5::util::DebouncedSender;
use homie5::{device_description::DeviceDescriptionBuilder, Homie5ControllerProtocol, HomieValue, PropertyRef};
use tokio::sync::{mpsc, RwLock};

use crate::{
    app_state::AppEvent, cfg_files_tracker::CfgFilesTracker, device_manager::DeviceManager,
    mqtt_client::ManagedMqttClient,
};

use super::{property_indexer::PropertyIndexer, VirtualDevice, VirtualDeviceSpec};

pub struct VirtualDeviceManagerConfig {
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub client_id: String,
    pub homie_domain: HomieDomain,
    pub controller_id: HomieID,
    pub controller_name: String,
    /// Debounce window for controller children updates. While child devices
    /// are added/removed in bursts (e.g. initial config load), the controller
    /// device description is republished only once per quiet period instead
    /// of once per child. `None` publishes immediately on every change.
    pub controller_children_debounce: Option<Duration>,
}

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
    /// Observes queued plus in-flight publishes of this manager's device
    /// client connection (see [`VirtualDeviceManager::flush`]).
    pending_publishes: PendingPublishObserver,
    /// When set, children changes are staged and the controller description
    /// is republished debounced via [`AppEvent::FlushControllerChildren`].
    children_flush: Option<DebouncedSender<AppEvent>>,
    /// True when the controller description has staged children changes that
    /// have not been published yet (see [`VirtualDeviceManager::flush_children`]).
    children_dirty: bool,
}

impl VirtualDeviceManager {
    pub async fn new(
        dm: DeviceManager,
        mqtt_client: ManagedMqttClient,
        app_event_sender: mpsc::Sender<AppEvent>,
        config: VirtualDeviceManagerConfig,
    ) -> Result<(Self, HomieClientHandle, mpsc::Receiver<HomieClientEvent>)> {
        log::debug!("HomieDomain: {:?}", config.homie_domain);

        let index = Arc::new(RwLock::new(PropertyIndexer::new()));

        let (homie_proto, last_will) =
            Homie5DeviceProtocol::new(config.controller_id.clone(), config.homie_domain.clone());

        let homie_client_options = MqttClientConfig::new(&config.hostname)
            .client_id(format!("{}-ctrl", &config.client_id))
            .port(config.port)
            .username(&config.username)
            .password(&config.password)
            .last_will(Some(last_will));

        let (device_client_handle, homie_client, homie_event_receiver) =
            run_homie_client(homie_client_options.to_mqtt_options()?, homie_client_options.mqtt_channel_size)?;

        let device_desc = DeviceDescriptionBuilder::new()
            .name(config.controller_name.clone())
            .build();

        let pending_publishes = device_client_handle.pending_publishes();

        let children_flush = config
            .controller_children_debounce
            .filter(|d| !d.is_zero())
            .map(|debounce| DebouncedSender::new(debounce, app_event_sender.clone()));

        Ok((
            Self {
                status: HomieDeviceStatus::Init,
                ctrl_proto: Homie5ControllerProtocol::new(),
                device_ref: DeviceRef::new(config.homie_domain, config.controller_id),
                homie_proto,
                homie_client,
                devices: Arc::new(RwLock::new(HashMap::new())),
                device_desc,
                dm,
                mqtt_client,
                app_event_sender,
                index,
                files: CfgFilesTracker::new(),
                pending_publishes,
                children_flush,
                children_dirty: false,
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
                .unwrap_or("unknown-document".to_string());
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
        self.stage_or_publish_children().await
    }

    pub async fn remove_child_device(&mut self, child_id: &HomieID) -> Result<()> {
        self.device_desc.remove_child(child_id);
        self.stage_or_publish_children().await
    }

    /// Stage a children change for a debounced publish, or publish
    /// immediately when no debounce is configured.
    async fn stage_or_publish_children(&mut self) -> Result<()> {
        self.device_desc.update_version();
        if let Some(children_flush) = &self.children_flush {
            self.children_dirty = true;
            // each staged change resets the debounce timer; once the burst
            // settles, a single FlushControllerChildren event is emitted and
            // handled by the event loop (which calls [`flush_children`]).
            children_flush.send(AppEvent::FlushControllerChildren).await;
            Ok(())
        } else {
            self.republish_controller_description().await
        }
    }

    /// Publish staged children changes of the controller description. Call
    /// this on [`AppEvent::FlushControllerChildren`] and before shutdown so
    /// the retained description is up to date. Returns `true` if a republish
    /// occurred, `false` when nothing was pending.
    pub async fn flush_children(&mut self) -> Result<bool> {
        if !self.children_dirty {
            return Ok(false);
        }
        self.children_dirty = false;
        self.republish_controller_description().await?;
        Ok(true)
    }

    /// Republish the controller description with an init -> ready state transition.
    async fn republish_controller_description(&mut self) -> Result<()> {
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
        for (_, vdevice) in vdevices.iter_mut() {
            let changed = vdevice.update_compound_members(device_ref, desc, &devices).await?;
            if changed {
                // Only rebuild index for properties with query members, since only
                // query member changes affect the index (direct members are static).
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

    /// Trigger value recalculation for all virtual properties that have compound
    /// members referencing the given device. This is used when a device's state
    /// changes (e.g. Ready -> Lost or Lost -> Ready) so that aggregate values
    /// correctly include/exclude members based on their device readiness.
    pub async fn recalculate_members_for_device(&self, device_ref: &DeviceRef) -> Result<()> {
        let mut vdevices = self.devices.write().await;
        let index = self.index.read().await;
        let devices = self.dm.read().await;

        if let Some(device_index) = index.lookup_device(device_ref) {
            for vprop_refs in device_index.values() {
                for vprop_ref in vprop_refs.iter() {
                    if let Some(vdevice) = vdevices.get_mut(vprop_ref.device_ref()) {
                        vdevice.update_value(vprop_ref, &devices).await?;
                    }
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
        // Only called during full application shutdown; the process exits after
        // publishing disconnected states, so no rediscovery/republish path needs
        // these in-memory devices afterwards.
        devices.clear();
        Ok(())
    }
    pub async fn disconnect_client(&self) -> Result<()> {
        self.homie_client.disconnect().await?;
        Ok(())
    }

    /// Waits until all in-flight QoS>0 publishes of this manager's device
    /// client connection (the LWT-bearing one carrying all virtual device
    /// `$state` updates) have been acknowledged by the broker.
    ///
    /// Returns immediately when nothing is pending; `max_wait` is only an
    /// upper bound for the dead-broker case. Call this before
    /// [`disconnect_client`](Self::disconnect_client) so the broker discards
    /// the LWT on the subsequent clean DISCONNECT.
    pub async fn flush(&self, max_wait: Duration) -> Result<(), FlushTimeout> {
        self.pending_publishes.flushed(max_wait).await
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
