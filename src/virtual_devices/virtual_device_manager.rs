use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

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

use super::{group_materializer::materialize, property_indexer::PropertyIndexer, VirtualDevice, VirtualDeviceSpec};

/// Default debounce for structure rebuilds of a group device when the spec
/// does not set `group.rebuild_debounce`.
const DEFAULT_REBUILD_DEBOUNCE: Duration = Duration::from_secs(1);

/// Runtime state of a materialized group device (a [`VirtualDeviceSpec`] with
/// `group: Some(..)`). Stored by the manager so member discovery changes can
/// trigger a debounced re-materialization (see
/// [`VirtualDeviceManager::rebuild_group_device`]).
struct GroupRuntime {
    /// The original (un-materialized) spec as parsed from the config.
    original_spec: VirtualDeviceSpec,
    /// Structure hash of the last materialization result (rebuild gate).
    structure_hash: u64,
    /// Resolved member device refs (static for the lifetime of the spec).
    member_refs: HashSet<DeviceRef>,
    /// Per-group debouncer emitting [`AppEvent::RebuildGroupDevice`].
    rebuild_flush: DebouncedSender<AppEvent>,
}

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
    /// Runtime state of materialized group devices, keyed by the spec's
    /// config item hash.
    groups: Arc<RwLock<HashMap<ConfigItemHash, GroupRuntime>>>,
    /// Reverse index source device -> group devices it is a member of
    /// (O(1) rebuild trigger lookup on discovery changes).
    member_index: Arc<RwLock<HashMap<DeviceRef, HashSet<ConfigItemHash>>>>,
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
                groups: Arc::new(RwLock::new(HashMap::new())),
                member_index: Arc::new(RwLock::new(HashMap::new())),
            },
            device_client_handle,
            homie_event_receiver,
        ))
    }

    pub async fn add_device(&mut self, hash: ConfigItemHash, spec: VirtualDeviceSpec) -> Result<DeviceRef> {
        // Group devices are materialized first: the structure is derived from
        // the discovered member descriptions and the expanded spec then runs
        // through the unchanged creation path below. The group runtime is
        // registered only after the device was successfully added.
        let mut group_ctx: Option<(VirtualDeviceSpec, u64)> = None;
        let spec = if spec.group.is_some() {
            let devices = self.dm.read().await;
            let materialized = materialize(&spec, &devices, self.homie_domain());
            drop(devices);
            group_ctx = Some((spec, materialized.structure_hash));
            materialized.spec
        } else {
            spec
        };

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
        if let Some((original_spec, structure_hash)) = group_ctx {
            self.register_group(hash, original_spec, structure_hash).await;
        }
        Ok(vdev_ref)
    }

    pub async fn remove_device(&mut self, hash: ConfigItemHash) -> Result<Option<VirtualDevice>> {
        // Unconditionally drop any group runtime for this spec: the device
        // itself may be missing from the map (e.g. a failed rebuild), but the
        // group bookkeeping must not outlive the config item.
        self.unregister_group(hash).await;
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

    /// Register the runtime state of a successfully added group device:
    /// stores the original spec + structure hash and indexes its members for
    /// O(1) rebuild trigger lookup.
    async fn register_group(&self, hash: ConfigItemHash, original_spec: VirtualDeviceSpec, structure_hash: u64) {
        let Some(group) = original_spec.group.as_ref() else {
            return;
        };
        let member_refs: HashSet<DeviceRef> = group
            .devices
            .iter()
            .map(|id| DeviceRef::new(self.homie_domain().clone(), id.clone()))
            .collect();
        let rebuild_debounce = group.rebuild_debounce.unwrap_or(DEFAULT_REBUILD_DEBOUNCE);
        let rebuild_flush = DebouncedSender::new(rebuild_debounce, self.app_event_sender.clone());
        {
            let mut member_index = self.member_index.write().await;
            index_group_members(&mut member_index, hash, &member_refs);
        }
        self.groups.write().await.insert(
            hash,
            GroupRuntime {
                original_spec,
                structure_hash,
                member_refs,
                rebuild_flush,
            },
        );
    }

    /// Drop the group runtime and member index entries for a spec. No-op for
    /// non-group specs (nothing registered under the hash).
    async fn unregister_group(&self, hash: ConfigItemHash) {
        let Some(runtime) = self.groups.write().await.remove(&hash) else {
            return;
        };
        let mut member_index = self.member_index.write().await;
        unindex_group_members(&mut member_index, hash, &runtime.member_refs);
    }

    /// Notify the manager that a source device's description changed. Fans
    /// out a debounced [`AppEvent::RebuildGroupDevice`] to every group the
    /// device is a member of. No-op for devices that are not group members.
    pub async fn source_device_changed(&self, device_ref: &DeviceRef) {
        self.trigger_group_rebuilds(device_ref).await;
    }

    /// Notify the manager that a source device was removed. Same debounced
    /// rebuild fan-out as [`Self::source_device_changed`] -- the
    /// re-materialization reads the current `DeviceStore` state either way.
    pub async fn source_device_removed(&self, device_ref: &DeviceRef) {
        self.trigger_group_rebuilds(device_ref).await;
    }

    async fn trigger_group_rebuilds(&self, device_ref: &DeviceRef) {
        let hashes = {
            let member_index = self.member_index.read().await;
            affected_groups(&member_index, device_ref)
        };
        if hashes.is_empty() {
            return;
        }
        let groups = self.groups.read().await;
        for hash in hashes {
            if let Some(runtime) = groups.get(&hash) {
                log::debug!(
                    "[group:{}] member '{}' changed -- scheduling debounced rebuild",
                    runtime.original_spec.id,
                    device_ref.device_id()
                );
                runtime.rebuild_flush.send(AppEvent::RebuildGroupDevice(hash)).await;
            }
        }
    }

    /// Rebuild a materialized group device after member discovery changes
    /// (called on [`AppEvent::RebuildGroupDevice`]).
    ///
    /// Re-runs materialization on the stored original spec; when the
    /// structure hash is unchanged the rebuild is a no-op. Otherwise the
    /// existing [`VirtualDevice`] is torn down (cancel mqtt reads, publish
    /// disconnected state, drop property indexes) and a new one is created
    /// and published (init -> ready cycle with a fresh description version).
    /// The controller child registration is kept across the rebuild so the
    /// controller description does not churn.
    ///
    /// Returns the device ref of the rebuilt device so the caller can
    /// re-notify rule queries with the new description, or `None` when
    /// nothing changed (or the group is no longer registered).
    pub async fn rebuild_group_device(&self, hash: ConfigItemHash) -> Result<Option<DeviceRef>> {
        let (original_spec, old_hash) = {
            let groups = self.groups.read().await;
            let Some(runtime) = groups.get(&hash) else {
                // group was removed while the rebuild event was in flight
                return Ok(None);
            };
            (runtime.original_spec.clone(), runtime.structure_hash)
        };

        let devices = self.dm.read().await;
        let materialized = materialize(&original_spec, &devices, self.homie_domain());
        let device_ref = DeviceRef::new(self.homie_domain().clone(), materialized.spec.id.clone());

        let exists = self.devices.read().await.contains_key(&device_ref);
        if materialized.structure_hash == old_hash && exists {
            log::debug!(
                "[group:{}] discovery change did not alter the materialized structure -- skipping rebuild",
                original_spec.id
            );
            return Ok(None);
        }

        // Teardown of the existing device. Mirrors remove_device, but the
        // controller child registration is intentionally kept.
        {
            let mut vdevices = self.devices.write().await;
            if let Some(mut dev) = vdevices.remove(&device_ref) {
                log::debug!("[group:{}] tearing down for rebuild", original_spec.id);
                dev.cancel_all_mqtt_reads().await?;
                dev.disconnect_device().await?;
                let mut index = self.index.write().await;
                index.remove_indexes_for_virtual_device(&device_ref);
            }
        }

        let mut dev = VirtualDevice::new(
            device_ref.clone(),
            hash,
            materialized.spec,
            self.device_ref.clone(),
            self.homie_proto(),
            &self.homie_client,
            &devices,
            &self.mqtt_client,
            self.app_event_sender.clone(),
        )
        .await?;
        {
            let mut index = self.index.write().await;
            index.add_index_for_virtual_device(&dev);
        }
        if !dev.mqtt_reads() {
            dev.publish_device().await?;
        } else {
            dev.publish_state().await?;
        }
        self.devices.write().await.insert(device_ref.clone(), dev);

        // Update the rebuild gate only after the new device is in place; on
        // creation failure the stale hash keeps the next trigger rebuilding.
        if let Some(runtime) = self.groups.write().await.get_mut(&hash) {
            runtime.structure_hash = materialized.structure_hash;
        }
        log::info!("[group:{}] rebuilt and republished after member discovery change", original_spec.id);
        Ok(Some(device_ref))
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

/// Add `hash` to the member index entry of every member device.
///
/// Generic over the group key so the pure index logic is unit-testable
/// ([`ConfigItemHash`] has no public constructor).
fn index_group_members<H: Copy + Eq + std::hash::Hash>(
    member_index: &mut HashMap<DeviceRef, HashSet<H>>,
    hash: H,
    member_refs: &HashSet<DeviceRef>,
) {
    for member in member_refs {
        member_index.entry(member.clone()).or_default().insert(hash);
    }
}

/// Remove `hash` from the member index entries of every member device,
/// dropping entries that become empty.
fn unindex_group_members<H: Copy + Eq + std::hash::Hash>(
    member_index: &mut HashMap<DeviceRef, HashSet<H>>,
    hash: H,
    member_refs: &HashSet<DeviceRef>,
) {
    for member in member_refs {
        if let Some(hashes) = member_index.get_mut(member) {
            hashes.remove(&hash);
            if hashes.is_empty() {
                member_index.remove(member);
            }
        }
    }
}

/// All groups the given source device is a member of (rebuild fan-out set).
fn affected_groups<H: Copy + Eq + std::hash::Hash>(
    member_index: &HashMap<DeviceRef, HashSet<H>>,
    device_ref: &DeviceRef,
) -> Vec<H> {
    member_index
        .get(device_ref)
        .map(|hashes| hashes.iter().copied().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use homie5::HomieDomain;

    fn dev(id: &str) -> DeviceRef {
        DeviceRef::new(HomieDomain::Default, id.to_string().try_into().unwrap())
    }

    fn members(ids: &[&str]) -> HashSet<DeviceRef> {
        ids.iter().map(|id| dev(id)).collect()
    }

    #[test]
    fn fan_out_returns_all_groups_sharing_a_member() {
        let mut index: HashMap<DeviceRef, HashSet<u64>> = HashMap::new();
        index_group_members(&mut index, 1u64, &members(&["dev-a", "dev-b"]));
        index_group_members(&mut index, 2u64, &members(&["dev-b", "dev-c"]));

        let mut hit = affected_groups(&index, &dev("dev-b"));
        hit.sort_unstable();
        assert_eq!(hit, vec![1, 2]);

        assert_eq!(affected_groups(&index, &dev("dev-a")), vec![1]);
        assert_eq!(affected_groups(&index, &dev("dev-c")), vec![2]);
    }

    #[test]
    fn fan_out_is_empty_for_non_member_devices() {
        let mut index: HashMap<DeviceRef, HashSet<u64>> = HashMap::new();
        index_group_members(&mut index, 1u64, &members(&["dev-a"]));

        assert!(affected_groups(&index, &dev("dev-x")).is_empty());
        assert!(affected_groups(&HashMap::<DeviceRef, HashSet<u64>>::new(), &dev("dev-a")).is_empty());
    }

    #[test]
    fn unindex_removes_only_own_entries_and_cleans_empty_sets() {
        let mut index: HashMap<DeviceRef, HashSet<u64>> = HashMap::new();
        let g1 = members(&["dev-a", "dev-b"]);
        let g2 = members(&["dev-b", "dev-c"]);
        index_group_members(&mut index, 1u64, &g1);
        index_group_members(&mut index, 2u64, &g2);

        unindex_group_members(&mut index, 1u64, &g1);

        // dev-a entry is gone entirely, dev-b only lost group 1
        assert!(affected_groups(&index, &dev("dev-a")).is_empty());
        assert!(!index.contains_key(&dev("dev-a")));
        assert_eq!(affected_groups(&index, &dev("dev-b")), vec![2]);
        assert_eq!(affected_groups(&index, &dev("dev-c")), vec![2]);

        unindex_group_members(&mut index, 2u64, &g2);
        assert!(index.is_empty());
    }

    #[test]
    fn unindex_is_idempotent_for_unknown_groups() {
        let mut index: HashMap<DeviceRef, HashSet<u64>> = HashMap::new();
        let g1 = members(&["dev-a"]);
        index_group_members(&mut index, 1u64, &g1);

        // removing a never-registered group must not disturb existing entries
        unindex_group_members(&mut index, 99u64, &g1);
        assert_eq!(affected_groups(&index, &dev("dev-a")), vec![1]);
    }
}
