use crate::{
    app_state::AppEvent,
    mqtt_client::ManagedMqttClient,
    virtual_devices::{
        aggregate_members::update_compound_value,
        compound_member::{MqttCompoundMember, PropertyCompoundMember},
    },
};
use color_eyre::eyre::{self, Result};
use hc_homie5::{DebouncedSender, DelayedSender, DeviceStore, HomieMQTTClient, MappingResult, UniqueByExt};

use homie5::{
    device_description::HomieDeviceDescription, DeviceRef, Homie5ControllerProtocol, Homie5DeviceProtocol,
    HomieDataType, HomieValue, PropertyRef, ToTopic,
};
use std::{collections::HashMap, iter, time::Duration};
use tokio::sync::mpsc;

use super::{
    compound_member::{map_value_list, MqttCompoundMembers, PropertyCompoundMembers},
    query_compound_member::QueryCompoundMember,
    CompoundSpec, MemberSpec, VirtualPropertyConfig, VirtualPropertyOptions,
};

#[derive(Debug)]
pub struct VirtualProperty {
    pub prop_ref: PropertyRef,
    pub value: Option<HomieValue>,
    pub retained: bool,
    pub datatype: HomieDataType,
    #[allow(dead_code)]
    pub options: VirtualPropertyOptions,
    pub pass_through: bool,
    compound_spec: Option<CompoundSpec>,
    prop_compound_members: PropertyCompoundMembers,
    mqtt_compound_members: MqttCompoundMembers,
    query_compound_members: Vec<QueryCompoundMember>,
    debouncer: Option<DebouncedSender<AppEvent>>,
    has_queries: bool,
    read_handle: DelayedSender,
}

impl VirtualProperty {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        prop_ref: PropertyRef,
        spec: VirtualPropertyConfig,
        node_prop_opts: Option<&VirtualPropertyOptions>,
        node_pass_through: Option<&bool>,
        retained: bool,
        datatype: HomieDataType,
        devices: &DeviceStore,
        mqtt_client: &ManagedMqttClient,
        homie_client: &HomieMQTTClient,
        app_event_sender: mpsc::Sender<AppEvent>,
    ) -> Result<Self> {
        let mut prop_compound_members: PropertyCompoundMembers = HashMap::new();
        let mut mqtt_compoung_members = HashMap::new();
        let mut query_compound_members = Vec::new();
        let mut value = None;
        let mut has_queries = false;
        let mut debouncer = None;
        if let Some(compound_spec) = &spec.compound_spec {
            debouncer = Some(DebouncedSender::new(
                compound_spec.aggregation_debounce.unwrap_or(Duration::from_millis(200)),
                app_event_sender.clone(),
            ));

            for member_spec in &compound_spec.members {
                match member_spec {
                    MemberSpec::Subject(subject) => {
                        let m = PropertyCompoundMember::new(subject, None, devices, compound_spec.mapping.as_ref());
                        prop_compound_members.insert(subject.clone().into(), m);
                    }
                    MemberSpec::SubjectMember { subject, mapping } => {
                        let m = PropertyCompoundMember::new(
                            subject,
                            Some(mapping.clone()),
                            devices,
                            compound_spec.mapping.as_ref(),
                        );
                        prop_compound_members.insert(subject.clone().into(), m);
                    }
                    MemberSpec::MqttMember {
                        mqtt_input: input,
                        mqtt_output: output,
                    } => {
                        let topic = input.topic.clone();
                        let m = MqttCompoundMember {
                            value: None,
                            input: input.clone(),
                            output: output.clone(),
                        };
                        mqtt_client.subscribe(&topic, rumqttc::QoS::ExactlyOnce).await?;
                        mqtt_compoung_members.insert(topic.clone(), m);
                    }
                    MemberSpec::QueryMember { query, mapping } => {
                        query_compound_members.push(QueryCompoundMember::new(
                            *query.clone(),
                            mapping.clone(),
                            devices,
                            compound_spec.mapping.as_ref(),
                        ));
                        has_queries = true;
                    }
                }
            }

            value = update_compound_value(
                prop_compound_members
                    .values()
                    .chain(query_compound_members.iter().flat_map(|qcm| qcm.values()))
                    .unique_by(|v| &v.prop),
                &mqtt_compoung_members,
                datatype,
                devices,
                compound_spec.aggregate_function,
            );
            log::debug!("{} -- Property value: {:?}", prop_ref.to_topic().build(), value);

            //log::debug!("Prop: {:#?}", prop_compound_members);
        }

        // check if we have pass_through defined, otherwise take it from the node or if this also
        // is not defined use the default
        let pass_through = if let Some(pass_through) = spec.pass_through {
            pass_through
        } else if let Some(pt_node) = node_pass_through {
            *pt_node
        } else {
            false
        };

        // check if we have options defined, otherwise take them from the node or if this also
        // is not defined use the default
        let options = if let Some(options) = spec.propert_opts {
            options
        } else if let Some(node_options) = node_prop_opts {
            node_options.clone()
        } else {
            Default::default()
        };

        let read_handle = if options.read_from_mqtt && retained {
            Self::read_value_from_mqtt(
                &prop_ref,
                homie_client,
                app_event_sender.clone(),
                options.read_timeout.unwrap_or(Duration::from_secs(3)),
            )
            .await?
        } else {
            Default::default()
        };

        Ok(Self {
            prop_ref,
            options,
            pass_through,
            retained,
            datatype,
            value,
            compound_spec: spec.compound_spec,
            prop_compound_members,
            mqtt_compound_members: mqtt_compoung_members,
            query_compound_members,
            has_queries,
            debouncer,
            read_handle,
        })
    }

    pub async fn update_compound_members_removed(
        &mut self,
        device_ref: &DeviceRef,
        // devices: &DeviceStore,
        // homie_client: &HomieMQTTClient,
        // homie_proto: &Homie5DeviceProtocol,
    ) -> Result<bool> {
        let mut changed = false;
        self.prop_compound_members.retain(|k, _| {
            if k.device_ref() == device_ref {
                changed = true;
                return false;
            }
            true
        });

        for qcm in self.query_compound_members.iter_mut() {
            if qcm.update_compound_members_removed(device_ref) {
                changed = true;
            }
        }

        if changed {
            // self.update_value(devices, homie_client, homie_proto).await?;
            self.trigger_value_recalculation().await;
        }
        Ok(changed)
    }

    // called when a device description has changed
    // This will reevaluate the queries and if any changes occured trigger a debounced
    // recalculation of the property value
    pub async fn update_compound_members(
        &mut self,
        device_ref: &DeviceRef,
        desc: &HomieDeviceDescription,
        devices: &DeviceStore,
        // homie_client: &HomieMQTTClient,
        // homie_proto: &Homie5DeviceProtocol,
    ) -> Result<bool> {
        if !self.has_queries {
            return Ok(false);
        }
        let Some(compound_spec) = &self.compound_spec else {
            return Ok(false);
        };

        if device_ref == self.prop_ref.device_ref() && desc.with_property(&self.prop_ref, |_| true).unwrap_or(false) {
            // do not include self in queries
            return Ok(false);
        }

        // update settable flag for property compound members
        for (node_id, _, prop_id, prop_desc) in desc.iter() {
            for (prop_ref, pcm) in self.prop_compound_members.iter_mut() {
                if prop_ref == device_ref && prop_ref.node_id() == node_id && prop_ref.prop_id() == prop_id {
                    pcm.settable = prop_desc.settable;
                }
            }
        }

        let mut changed = false;
        for qcm in self.query_compound_members.iter_mut() {
            if qcm.update_compound_members(device_ref, desc, devices, compound_spec.mapping.as_ref()) {
                changed = true;
            }
        }

        if changed {
            // self.update_value(devices, homie_client, homie_proto).await?;
            self.trigger_value_recalculation().await;
        }

        Ok(changed)
    }

    pub async fn publish_value(
        &self,
        homie_client: &HomieMQTTClient,
        homie_proto: &Homie5DeviceProtocol,
    ) -> Result<()> {
        if let Some(value) = self.value.as_ref() {
            if !matches!(value, HomieValue::Empty) {
                homie_client
                    .homie_publish(homie_proto.publish_value(
                        self.prop_ref.node_id(),
                        self.prop_ref.prop_id(),
                        value,
                        self.retained,
                    ))
                    .await?;
            }
        }
        Ok(())
    }

    // Called when one of the compound member properties has published a new value.
    // Will store the new value and trigger a (debounced) recalulation of the property value
    pub async fn update_member_value_prop(
        &mut self,
        prop: &PropertyRef,
        value: &HomieValue,
        // devices: &DeviceStore,
        // homie_client: &HomieMQTTClient,
        // homie_proto: &Homie5DeviceProtocol,
    ) -> Result<()> {
        let mut changed = false;
        if let Some(pcm) = self.prop_compound_members.get_mut(prop) {
            pcm.update_value(self.compound_spec.as_ref().and_then(|cs| cs.mapping.as_ref()), value);
            changed = true;
        }

        for qcm in self.query_compound_members.iter_mut() {
            if qcm.update_member_value_prop(prop, value, self.compound_spec.as_ref().and_then(|cs| cs.mapping.as_ref()))
            {
                changed = true;
            }
        }
        if changed {
            // self.update_value(devices, homie_client, homie_proto).await?;
            self.trigger_value_recalculation().await;
        }
        Ok(())
    }

    async fn trigger_value_recalculation(&self) {
        if let Some(debouncer) = &self.debouncer {
            debouncer
                .send(AppEvent::RecalculateVirtualPropertyValue(self.prop_ref.clone()))
                .await;
        }
    }

    pub async fn recalculate_value(
        &mut self,
        devices: &DeviceStore,
        homie_client: &HomieMQTTClient,
        homie_proto: &Homie5DeviceProtocol,
    ) -> Result<(), eyre::Error> {
        if let Some(aggr_func) = self.compound_spec.as_ref().map(|f| f.aggregate_function) {
            let value = update_compound_value(
                self.compound_properties(),
                &self.mqtt_compound_members,
                self.datatype,
                devices,
                aggr_func,
            );
            if value != self.value {
                self.value = value;
                log::debug!("{} -- Property value: {:?}", self.prop_ref.to_topic().build(), self.value);
                self.publish_value(homie_client, homie_proto).await?;
            }
        } else {
            self.publish_value(homie_client, homie_proto).await?;
        }
        Ok(())
    }

    pub async fn set_value(
        &mut self,
        value: HomieValue,
        homie_client: &HomieMQTTClient,
        homie_proto: &Homie5DeviceProtocol,
    ) -> Result<(), eyre::Error> {
        if Some(&value) != self.value.as_ref() {
            self.value = Some(value);
            log::debug!("{} -- Property value: {:?}", self.prop_ref.to_topic().build(), self.value);
            self.publish_value(homie_client, homie_proto).await?;
        }
        Ok(())
    }

    pub async fn update_member_value_mqtt(
        &mut self,
        topic: &str,
        value: &str,
        // devices: &DeviceStore,
        // homie_client: &HomieMQTTClient,
        // homie_proto: &Homie5DeviceProtocol,
    ) -> Result<()> {
        if let Some(mcm) = self.mqtt_compound_members.get_mut(topic) {
            if let MappingResult::Mapped(value) =
                map_value_list(Some(&String::from(value)), Some(&mcm.input.mapping), None)
            {
                mcm.value = value;
                // self.update_value(devices, homie_client, homie_proto).await?;
                self.trigger_value_recalculation().await;
            }
        }
        Ok(())
    }

    // returns a list (distinct by propref) of all compound members
    pub fn compound_properties(&self) -> impl Iterator<Item = &PropertyCompoundMember> {
        self.prop_compound_members
            .values()
            .chain(self.query_compound_members.iter().flat_map(|qcm| qcm.values()))
            .unique_by(|v| &v.prop)
    }

    pub async fn handle_set_command(
        &mut self,
        value: HomieValue,
        homie_client: &HomieMQTTClient,
        mqtt_client: &ManagedMqttClient,
        homie_proto: &Homie5DeviceProtocol,
    ) -> Result<()> {
        let ctrl_proto = Homie5ControllerProtocol::new();
        if self.pass_through && self.value.as_ref() != Some(&value) {
            log::debug!("pass_through value: {}", value);
            self.value = Some(value.clone());
            self.publish_value(homie_client, homie_proto).await?;
        }
        for (prop_ref, member) in self.prop_compound_members.iter().filter(|(_, pcm)| pcm.settable) {
            let p = if let Some(mapping) = &member.mapping {
                ctrl_proto.set_command(prop_ref, mapping.map_ouput(&value).unwrap())
            } else {
                ctrl_proto.set_command(prop_ref, &value)
            };
            homie_client.homie_publish(p).await?;
        }
        for (_, member) in self.mqtt_compound_members.iter() {
            let Some(output) = member.output.as_ref() else {
                continue;
            };
            match output.mapping.map_to(&value) {
                MappingResult::Mapped(v) => {
                    mqtt_client
                        .publish(&output.topic, HomieMQTTClient::map_qos(&output.qos), output.retained, v.as_str())
                        .await?;
                }
                MappingResult::Unmapped(v) => {
                    mqtt_client
                        .publish(
                            &output.topic,
                            HomieMQTTClient::map_qos(&output.qos),
                            output.retained,
                            v.to_string().as_str(),
                        )
                        .await?;
                }
            }
        }
        Ok(())
    }

    pub async fn read_value_from_mqtt(
        prop_ref: &PropertyRef,
        homie_client: &HomieMQTTClient,
        app_event_sender: mpsc::Sender<AppEvent>,
        timeout: Duration,
    ) -> Result<DelayedSender> {
        homie_client
            .homie_subscribe(iter::once(homie5::client::Subscription {
                topic: prop_ref.to_topic().build(),
                qos: homie5::client::QoS::ExactlyOnce,
            }))
            .await?;
        let ds = DelayedSender::from_schedule(
            app_event_sender,
            AppEvent::CancelPropertyValueReadFromMqtt(prop_ref.clone()),
            timeout,
        )
        .await;
        // let handle = tokio::task::spawn(async move {
        //     log::debug!(
        //         "{} - schedule cancel task for value read from mqtt: {:?}",
        //         prop_ref_task.to_topic().build(),
        //         timeout
        //     );
        //     tokio::time::sleep(timeout).await;
        //     app_event_sender
        //         .send(AppEvent::CancelPropertyValueReadFromMqtt(prop_ref_task))
        //         .await?;
        //     Ok::<(), eyre::Error>(())
        // });
        Ok(ds)
    }

    pub async fn handle_read_value_from_mqtt(
        &mut self,
        value: HomieValue,
        homie_client: &HomieMQTTClient,
        homie_proto: &Homie5DeviceProtocol,
    ) -> Result<()> {
        if self.read_handle.abort() {
            // unsubscribe from property topic
            homie_client
                .homie_unsubscribe(iter::once(homie5::client::Unsubscribe {
                    topic: self.prop_ref.to_topic().build(),
                }))
                .await?;
            log::debug!("{} - Value read from mqtt: {}", self.prop_ref.to_topic().build(), value);
            self.set_value(value, homie_client, homie_proto).await?;
        }

        Ok(())
    }

    pub async fn cancel_read_value_from_mqtt(&mut self, homie_client: &HomieMQTTClient) -> Result<()> {
        if self.read_handle.abort() {
            log::debug!(
                "{} - Cancel reading from mqtt, timeout: {:?}",
                self.prop_ref.to_topic().build(),
                self.options.read_timeout
            );
            // unsubscribe from property topic
            homie_client
                .homie_unsubscribe(iter::once(homie5::client::Unsubscribe {
                    topic: self.prop_ref.to_topic().build(),
                }))
                .await?;
        }
        Ok(())
    }

    pub fn has_queries(&self) -> bool {
        self.has_queries
    }
    pub fn wait_for_mqtt_read(&self) -> bool {
        !self.read_handle.is_finished()
    }
}
