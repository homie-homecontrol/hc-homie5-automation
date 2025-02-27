#![allow(dead_code)]
use std::collections::{btree_map, HashMap, HashSet};

use color_eyre::eyre::{self, eyre, Result};
use config_watcher::ConfigItemHash;
use hc_homie5::{homie_device, DeviceStore, HomieDevice};
use hc_homie5_smarthome::{
    button_node::ButtonNodeBuilder, colorlight_node::ColorlightNodeBuilder, contact_node::ContactNodeBuilder,
    dimmer_node::DimmerNodeBuilder, light_scene_node::LightSceneNodeBuilder, maintenance_node::MaintenanceNodeBuilder,
    motion_node::MotionNodeBuilder, numeric_sensor_node::NumericSensorNodeBuilder,
    orientation_node::OrientationNodeBuilder, shutter_node::ShutterNodeBuilder, switch_node::SwitchNodeBuilder,
    thermostat_node::ThermostatNodeBuilder, tilt_node::TiltNodeBuilder, vibration_node::VibrationNodeBuilder,
    water_sensor_node::WaterSensorNodeBuilder, weather_node::WeatherNodeBuilder,
};
use homie5::{device_description::DeviceDescriptionBuilder, HomieValue, PropertyPointer, PropertyRef, ToTopic};
use tokio::sync::mpsc;

use crate::{app_state::AppEvent, mqtt_client::ManagedMqttClient};

use super::{virtual_property::VirtualProperty, SmarthomeSpec, VirtualDeviceSpec};

#[homie_device]
#[derive(Debug)]
pub struct VirtualDevice {
    spec_hash: ConfigItemHash,
    pub(crate) properties: HashMap<PropertyPointer, VirtualProperty>,
    pub(crate) alerts: HashMap<HomieID, String>,
    mqtt_client: ManagedMqttClient,
    has_queries: bool,
    mqtt_reads: bool,
}

impl VirtualDevice {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        device_ref: DeviceRef,
        spec_hash: ConfigItemHash,
        spec: VirtualDeviceSpec,
        root_ref: DeviceRef,
        homie_proto: &Homie5DeviceProtocol,
        homie_client: &HomieMQTTClient,
        devices: &DeviceStore,
        mqtt_client: &ManagedMqttClient,
        app_event_sender: mpsc::Sender<AppEvent>,
    ) -> Result<Self, eyre::Error> {
        let homie_proto = homie_proto.clone_for_child(spec.id.clone());
        let mut device_desc = DeviceDescriptionBuilder::new()
            .name(spec.name.clone().unwrap_or_default())
            .root(root_ref.device_id().clone())
            .parent(root_ref.device_id().clone());
        for child in spec.children {
            device_desc = device_desc.add_child(child);
        }
        let mut properties = HashMap::new();
        let mut has_queries = false;
        let mut mqtt_reads = false;

        for node_spec in spec.nodes.into_iter() {
            let (node_id, vnode_desc, vproperty_specs, node_config) = node_spec.split_into();
            let mut prop_ids = HashSet::new();
            let mut node_desc = if let Some(from_smarthome) = node_config.from_smarthome {
                match from_smarthome {
                    SmarthomeSpec::Button(button_node_config) => {
                        ButtonNodeBuilder::new(&button_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::ColorLight(colorlight_node_config) => {
                        ColorlightNodeBuilder::new(&colorlight_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Dimmer(dimmer_node_config) => {
                        DimmerNodeBuilder::new(&dimmer_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::LightScene(light_scene_node_config) => {
                        LightSceneNodeBuilder::new(&light_scene_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Maintenance(maintenance_node_config) => {
                        MaintenanceNodeBuilder::new(maintenance_node_config.clone().unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Motion(motion_node_config) => {
                        MotionNodeBuilder::new(&motion_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Shutter(shutter_node_config) => {
                        ShutterNodeBuilder::new(&shutter_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Switch(switch_node_config) => {
                        SwitchNodeBuilder::new(&switch_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Thermostat(thermostat_node_config) => {
                        ThermostatNodeBuilder::new(&thermostat_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Vibration(vibration_node_config) => {
                        VibrationNodeBuilder::new(&vibration_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Weather(weather_node_config) => {
                        WeatherNodeBuilder::new(&weather_node_config.unwrap_or_default()).build()
                    }
                    SmarthomeSpec::Contact => ContactNodeBuilder::new().build(),
                    SmarthomeSpec::Numeric => NumericSensorNodeBuilder::new().build(),
                    SmarthomeSpec::Orientation => OrientationNodeBuilder::new().build(),
                    SmarthomeSpec::WaterSensor => WaterSensorNodeBuilder::new().build(),
                    SmarthomeSpec::Tilt => TiltNodeBuilder::new().build(),
                }
            } else {
                vnode_desc.into()
            };

            for prop_spec in vproperty_specs.into_iter() {
                let (prop_id, vprop_desc, vprop_config) = prop_spec.split_into();
                let (retained, datatype) = match node_desc.properties.entry(prop_id.clone()) {
                    btree_map::Entry::Vacant(entry) => {
                        let desc = entry.insert(vprop_desc.into());
                        (desc.retained, desc.datatype)
                    }
                    btree_map::Entry::Occupied(mut entry) => {
                        let v = entry.get_mut();
                        vprop_desc.patch(v);
                        (v.retained, v.datatype)
                    }
                };
                let vprop = VirtualProperty::new(
                    PropertyRef::new(
                        homie_proto.homie_domain().clone(),
                        spec.id.clone(),
                        node_id.clone(),
                        prop_id.clone(),
                    ),
                    vprop_config,
                    node_config.property_opts.as_ref(),
                    node_config.pass_through.as_ref(),
                    retained,
                    datatype,
                    devices,
                    mqtt_client,
                    homie_client,
                    app_event_sender.clone(),
                )
                .await?;
                if vprop.has_queries() {
                    has_queries = true;
                }
                if vprop.wait_for_mqtt_read() {
                    mqtt_reads = true;
                }
                properties.insert(PropertyPointer::new(node_id.clone(), prop_id.clone()), vprop);
                prop_ids.insert(prop_id);
            }

            for (prop_id, prop_desc) in node_desc.properties.iter() {
                if prop_ids.contains(prop_id) {
                    continue;
                }

                let vprop = VirtualProperty::new(
                    PropertyRef::new(
                        homie_proto.homie_domain().clone(),
                        spec.id.clone(),
                        node_id.clone(),
                        prop_id.clone(),
                    ),
                    Default::default(),
                    node_config.property_opts.as_ref(),
                    node_config.pass_through.as_ref(),
                    prop_desc.retained,
                    prop_desc.datatype,
                    devices,
                    mqtt_client,
                    homie_client,
                    app_event_sender.clone(),
                )
                .await?;

                if vprop.has_queries() {
                    has_queries = true;
                }
                if vprop.wait_for_mqtt_read() {
                    mqtt_reads = true;
                }

                properties.insert(PropertyPointer::new(node_id.clone(), prop_id.clone()), vprop);
            }

            device_desc = device_desc.add_node(node_id, node_desc);
        }

        let device_desc = device_desc.build();

        Ok(Self {
            spec_hash,
            device_ref,
            status: HomieDeviceStatus::Init,
            device_desc,
            properties,
            alerts: HashMap::new(),
            has_queries,
            mqtt_reads,
            homie_proto,
            homie_client: homie_client.clone(),
            mqtt_client: mqtt_client.clone(),
        })
    }

    pub fn spec_hash(&self) -> ConfigItemHash {
        self.spec_hash
    }

    pub async fn update_compound_members_removed(
        &mut self,
        device_ref: &DeviceRef,
        // devices: &DeviceStore,
    ) -> Result<bool> {
        for (_, vprop) in self.properties.iter_mut() {
            vprop.update_compound_members_removed(device_ref).await?;
        }
        Ok(true)
    }

    pub async fn update_compound_members(
        &mut self,
        device_ref: &DeviceRef,
        desc: &HomieDeviceDescription,
        devices: &DeviceStore,
    ) -> Result<bool> {
        for (_, vprop) in self.properties.iter_mut().filter(|(_, v)| v.has_queries()) {
            vprop.update_compound_members(device_ref, desc, devices).await?;
        }
        Ok(true)
    }

    pub async fn update_member_value_prop(
        &mut self,
        prop: &PropertyRef,
        value: &HomieValue,
        // devices: &DeviceStore,
    ) -> Result<()> {
        for (_, vprop) in self.properties.iter_mut() {
            vprop.update_member_value_prop(prop, value).await?;
        }
        Ok(())
    }

    pub async fn update_member_value_mqtt(
        &mut self,
        topic: &str,
        value: &str,
        // devices: &DeviceStore
    ) -> Result<()> {
        for (_, vprop) in self.properties.iter_mut() {
            vprop.update_member_value_mqtt(topic, value).await?;
        }
        Ok(())
    }

    pub async fn update_value(&mut self, prop: &PropertyRef, devices: &DeviceStore) -> Result<()> {
        if let Some(vprop) = self.properties.get_mut(prop.prop_pointer()) {
            vprop
                .recalculate_value(devices, &self.homie_client, &self.homie_proto)
                .await?;
        }
        Ok(())
    }

    pub async fn set_value(&mut self, prop: &PropertyRef, value: HomieValue) -> Result<()> {
        if !self
            .device_desc
            .with_property(prop, |pdesc| value.validate(pdesc))
            .unwrap_or(false)
        {
            return Err(eyre!("{} - Trying to set a invalid HomieValue: {}", prop.to_topic().build(), value));
        }
        if let Some(vprop) = self.properties.get_mut(prop.prop_pointer()) {
            vprop.set_value(value, &self.homie_client, &self.homie_proto).await?;
        }
        Ok(())
    }

    pub async fn set_str_value(&mut self, prop: &PropertyRef, value: &str) -> Result<()> {
        let Some(v) = self
            .device_desc
            .with_property(prop, |pdesc| HomieValue::parse(value, pdesc))
            .transpose()?
        else {
            return Err(eyre!("Prop {} not part of {} ", prop.to_topic().build(), self.device_ref.to_topic()));
        };
        if let Some(vprop) = self.properties.get_mut(prop.prop_pointer()) {
            vprop.set_value(v, &self.homie_client, &self.homie_proto).await?;
        }
        Ok(())
    }

    pub async fn simulate_set_command(&mut self, property: &PropertyRef, value: HomieValue) -> Result<()> {
        if let Some(prop) = self.properties.get_mut(property.prop_pointer()) {
            // pass on to propery
            if let Some(true) = self
                .device_desc
                .with_property(property, |prop_desc| value.validate(prop_desc))
            {
                prop.handle_set_command(value, &self.homie_client, &self.mqtt_client, &self.homie_proto)
                    .await?;
            }
        }
        Ok(())
    }
    pub async fn set_alert(&mut self, alert_id: HomieID, value: String) -> Result<()> {
        self.homie_client
            .homie_publish(self.homie_proto.publish_alert(&alert_id, &value))
            .await?;
        self.alerts.insert(alert_id.clone(), value);
        Ok(())
    }

    pub async fn clear_alert(&mut self, alert_id: &HomieID) -> Result<()> {
        if self.alerts.remove(alert_id).is_some() {
            self.homie_client
                .homie_publish(self.homie_proto.publish_clear_alert(alert_id))
                .await?;
        }
        Ok(())
    }

    pub async fn handle_mqtt_read(&mut self, prop: &PropertyRef, value: &str) -> Result<()> {
        let Some(v) = self
            .device_desc
            .with_property(prop, |pdesc| HomieValue::parse(value, pdesc))
            .transpose()?
        else {
            return Err(eyre!("Prop {} not part of {} ", prop.to_topic().build(), self.device_ref.to_topic()));
        };
        if let Some(vprop) = self.properties.get_mut(prop.prop_pointer()) {
            vprop
                .handle_read_value_from_mqtt(v, &self.homie_client, &self.homie_proto)
                .await?;
            self.check_mqtt_read_and_publish().await?;
        }
        Ok(())
    }

    pub async fn cancel_mqtt_read(&mut self, prop: &PropertyRef) -> Result<()> {
        if let Some(vprop) = self.properties.get_mut(prop.prop_pointer()) {
            vprop.cancel_read_value_from_mqtt(&self.homie_client).await?;
            self.check_mqtt_read_and_publish().await?;
        }
        Ok(())
    }

    pub async fn cancel_all_mqtt_reads(&mut self) -> Result<()> {
        for (_, vprop) in self.properties.iter_mut() {
            vprop.cancel_read_value_from_mqtt(&self.homie_client).await?;
        }
        Ok(())
    }
    pub async fn check_mqtt_read_and_publish(&mut self) -> Result<()> {
        if self.properties.iter().all(|(_, v)| !v.wait_for_mqtt_read()) {
            self.mqtt_reads = false;
            self.publish_device().await?;
        }
        Ok(())
    }

    pub fn has_queries(&self) -> bool {
        self.has_queries
    }
    pub fn mqtt_reads(&self) -> bool {
        self.mqtt_reads
    }
}

impl HomieDevice for VirtualDevice {
    type ResultError = eyre::Error;

    async fn publish_property_values(&mut self) -> Result<(), Self::ResultError> {
        for (_, virt_prop) in self.properties.iter() {
            virt_prop.publish_value(&self.homie_client, self.homie_proto()).await?;
        }
        Ok(())
    }

    async fn handle_set_command(&mut self, property: &PropertyRef, _set_value: &str) -> Result<(), Self::ResultError> {
        if let Some(prop) = self.properties.get_mut(property.prop_pointer()) {
            // pass on to propery
            if let Some(Ok(value)) = self
                .device_desc
                .with_property(property, |prop_desc| HomieValue::parse(_set_value, prop_desc))
            {
                prop.handle_set_command(value, &self.homie_client, &self.mqtt_client, &self.homie_proto)
                    .await?;
            }
        }
        Ok(())
    }
}
