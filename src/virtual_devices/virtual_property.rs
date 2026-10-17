use crate::{
    app_state::AppEvent,
    mqtt_client::ManagedMqttClient,
    virtual_devices::{
        aggregate_members::update_compound_value,
        compound_member::{MqttCompoundMember, PropertyCompoundMember},
        debouncer::MaxWaitDebouncer,
    },
};
use color_eyre::eyre::{self, Result};
use hc_homie5::client::HomieMQTTClient;
use hc_homie5::logging::{truncate, PAYLOAD_MAX};
use hc_homie5::store::DeviceStore;
use hc_homie5::util::{DelayedSender, UniqueByExt};
use hc_homie5::value::MappingResult;

use homie5::{
    client::QoS, device_description::HomieDeviceDescription, DeviceRef, Homie5ControllerProtocol, Homie5DeviceProtocol,
    HomieDataType, HomieValue, PropertyRef, ToTopic,
};
use std::{collections::HashMap, iter, time::Duration};
use tokio::sync::mpsc;

use super::{
    compound_member::{map_value_list, MqttCompoundMembers, PropertyCompoundMembers},
    query_compound_member::QueryCompoundMember,
    CompoundSpec, MemberSpec, VirtualPropertyConfig, VirtualPropertyOptions,
};

const DEFAULT_AGGREGATION_DEBOUNCE: Duration = Duration::from_millis(200);
const DEFAULT_AGGREGATION_MAX_WAIT: Duration = Duration::from_secs(1);

/// How a compound property schedules the recalculation of its value.
#[derive(Debug)]
enum RecalcTrigger {
    /// Not a compound property.
    None,
    Debounced(MaxWaitDebouncer<AppEvent>),
    /// Event (non-retained) properties without a configured debounce: member events recalculate
    /// inline so presses are never merged, other changes go through the event loop.
    Immediate(mpsc::Sender<AppEvent>),
}

impl RecalcTrigger {
    fn for_compound(compound_spec: &CompoundSpec, retained: bool, app_event_sender: mpsc::Sender<AppEvent>) -> Self {
        let debounce = match compound_spec.aggregation_debounce {
            Some(debounce) => debounce,
            None if retained => DEFAULT_AGGREGATION_DEBOUNCE,
            None => return Self::Immediate(app_event_sender),
        };
        Self::Debounced(MaxWaitDebouncer::new(
            debounce,
            compound_spec
                .aggregation_max_wait
                .unwrap_or(DEFAULT_AGGREGATION_MAX_WAIT),
            app_event_sender,
        ))
    }
}

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
    recalc: RecalcTrigger,
    has_queries: bool,
    read_handle: DelayedSender,
    /// A member event arrived since the last recalculation. Non-retained properties are events:
    /// each member event publishes, even when the value repeats.
    event_pending: bool,
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
        let mut mqtt_compound_members = HashMap::new();
        let mut query_compound_members = Vec::new();
        let mut value = None;
        let mut has_queries = false;
        let mut recalc = RecalcTrigger::None;
        if let Some(compound_spec) = &spec.compound_spec {
            recalc = RecalcTrigger::for_compound(compound_spec, retained, app_event_sender.clone());
            let aggregate_function = compound_spec.aggregate_function.unwrap_or_default();
            if !aggregate_function.fits(datatype) {
                tracing::warn!(
                    vdevice = %prop_ref.device_id(),
                    node = %prop_ref.node_id(),
                    property = %prop_ref.prop_id(),
                    aggregate_function = ?aggregate_function,
                    datatype = %datatype,
                    "aggregate function does not fit the property datatype, value stays empty"
                );
            }

            for member_spec in &compound_spec.members {
                match member_spec {
                    MemberSpec::PropertyRef(prop_ref) => {
                        let m = PropertyCompoundMember::new(prop_ref, None, devices, compound_spec.mapping.as_ref());
                        prop_compound_members.insert(prop_ref.clone(), m);
                    }
                    MemberSpec::PropertyRefMember { property, mapping } => {
                        let m = PropertyCompoundMember::new(
                            property,
                            Some(mapping.clone()),
                            devices,
                            compound_spec.mapping.as_ref(),
                        );
                        prop_compound_members.insert(property.clone(), m);
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
                        // one subscription per member entry, so release_mqtt_members balances it
                        if mqtt_compound_members.insert(topic.clone(), m).is_none() {
                            mqtt_client
                                .subscribe(&topic, HomieMQTTClient::map_qos(&input.qos))
                                .await?;
                        }
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
                &mqtt_compound_members,
                datatype,
                devices,
                compound_spec.aggregate_function.unwrap_or_default(),
            );
            tracing::trace!(
                vdevice = %prop_ref.device_id(),
                node = %prop_ref.node_id(),
                property = %prop_ref.prop_id(),
                value = ?value,
                "compound value calculated"
            );
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
        let options = if let Some(options) = spec.property_opts {
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
            mqtt_compound_members,
            query_compound_members,
            has_queries,
            recalc,
            read_handle,
            event_pending: false,
        })
    }

    pub async fn update_compound_members_removed(&mut self, device_ref: &DeviceRef) -> Result<bool> {
        let mut changed = false;

        // Do NOT remove direct property compound members — they are fixed references
        // defined in config. The aggregate function already excludes members from
        // non-Ready devices at evaluation time (via device_state_resolved check).
        // Keeping them ensures they automatically recover when the device reappears.
        if self.prop_compound_members.keys().any(|k| k.device_ref() == device_ref) {
            changed = true;
        }

        // Query members are dynamic — remove the ones belonging to the removed device.
        // They will be re-added when the device is rediscovered via update_compound_members.
        for qcm in self.query_compound_members.iter_mut() {
            if qcm.update_compound_members_removed(device_ref) {
                changed = true;
            }
        }

        if changed {
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
    ) -> Result<bool> {
        let Some(compound_spec) = &self.compound_spec else {
            return Ok(false);
        };

        if device_ref == self.prop_ref.device_ref() && desc.with_property(&self.prop_ref, |_| true).unwrap_or(false) {
            // do not include self in queries or settable updates
            return Ok(false);
        }

        // Always update settable flag for direct property compound members,
        // even if this property has no query members. The settable flag may have
        // been initialized to the default (false) if the member device was not
        // yet discovered when this virtual property was created.
        for (node_id, _, prop_id, prop_desc) in desc.iter() {
            for (prop_ref, pcm) in self.prop_compound_members.iter_mut() {
                if prop_ref.belongs_to_device(device_ref)
                    && prop_ref.node_id() == node_id
                    && prop_ref.prop_id() == prop_id
                {
                    pcm.settable = prop_desc.settable;
                    pcm.retained = prop_desc.retained;
                }
            }
        }

        // Query member re-evaluation only needed if we have query members
        if !self.has_queries {
            return Ok(false);
        }

        let mut changed = false;
        for qcm in self.query_compound_members.iter_mut() {
            if qcm.update_compound_members(device_ref, desc, devices, compound_spec.mapping.as_ref()) {
                changed = true;
            }
        }

        if changed {
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

    /// Publishes the cached value during device publish or republish. Non-retained properties are
    /// events, so their last value is never replayed.
    pub async fn republish_value(
        &self,
        homie_client: &HomieMQTTClient,
        homie_proto: &Homie5DeviceProtocol,
    ) -> Result<()> {
        if !self.retained {
            return Ok(());
        }
        self.publish_value(homie_client, homie_proto).await
    }

    // Called when one of the compound member properties has published a new value.
    // Will store the new value and trigger a (debounced) recalulation of the property value.
    // Returns true when the caller must recalculate the value right away.
    pub async fn update_member_value_prop(
        &mut self,
        prop: &PropertyRef,
        value: &HomieValue,
        // devices: &DeviceStore,
        // homie_client: &HomieMQTTClient,
        // homie_proto: &Homie5DeviceProtocol,
    ) -> Result<bool> {
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
            self.event_pending = true;
            return Ok(self.member_value_recalculation().await);
        }
        Ok(false)
    }

    // Recalculation after a member event. Returns true when it must happen inline.
    async fn member_value_recalculation(&self) -> bool {
        match &self.recalc {
            RecalcTrigger::Immediate(_) => true,
            _ => {
                self.trigger_value_recalculation().await;
                false
            }
        }
    }

    async fn trigger_value_recalculation(&self) {
        let event = AppEvent::RecalculateVirtualPropertyValue(self.prop_ref.clone());
        match &self.recalc {
            RecalcTrigger::None => {}
            RecalcTrigger::Debounced(debouncer) => debouncer.send(event).await,
            RecalcTrigger::Immediate(sender) => {
                if sender.send(event).await.is_err() {
                    tracing::debug!("cannot send recalculation event; receiver closed");
                }
            }
        }
    }

    pub async fn recalculate_value(
        &mut self,
        devices: &DeviceStore,
        homie_client: &HomieMQTTClient,
        homie_proto: &Homie5DeviceProtocol,
    ) -> Result<(), eyre::Error> {
        if let Some(aggr_func) = self
            .compound_spec
            .as_ref()
            .map(|f| f.aggregate_function.unwrap_or_default())
        {
            let value = update_compound_value(
                self.compound_properties(),
                &self.mqtt_compound_members,
                self.datatype,
                devices,
                aggr_func,
            );
            let is_event = !self.retained && std::mem::take(&mut self.event_pending);
            if is_event || value != self.value {
                self.value = value;
                tracing::trace!(
                    vdevice = %self.prop_ref.device_id(),
                    node = %self.prop_ref.node_id(),
                    property = %self.prop_ref.prop_id(),
                    value = ?self.value,
                    "compound value changed"
                );
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
        if !self.retained || Some(&value) != self.value.as_ref() {
            self.value = Some(value);
            tracing::trace!(
                vdevice = %self.prop_ref.device_id(),
                node = %self.prop_ref.node_id(),
                property = %self.prop_ref.prop_id(),
                value = ?self.value,
                "virtual property value set"
            );
            self.publish_value(homie_client, homie_proto).await?;
        }
        Ok(())
    }

    // Returns true when the caller must recalculate the value right away.
    pub async fn update_member_value_mqtt(&mut self, topic: &str, value: &str) -> Result<bool> {
        if let Some(mcm) = self.mqtt_compound_members.get_mut(topic) {
            if let MappingResult::Mapped(value) =
                map_value_list(Some(&String::from(value)), Some(&mcm.input.mapping), None)
            {
                mcm.value = value;
                self.event_pending = true;
                return Ok(self.member_value_recalculation().await);
            }
        }
        Ok(false)
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
            self.value = Some(value.clone());
            self.publish_value(homie_client, homie_proto).await?;
            tracing::info!(
                vdevice = %self.prop_ref.device_id(),
                node = %self.prop_ref.node_id(),
                property = %self.prop_ref.prop_id(),
                value = truncate(&value.to_string(), PAYLOAD_MAX),
                "set value passed through"
            );
        }
        for (prop_ref, member) in self.prop_compound_members.iter().filter(|(_, pcm)| pcm.settable) {
            let mut p = if let Some(mapping) = &member.mapping {
                ctrl_proto.set_command(prop_ref, mapping.map_output(&value).unwrap())
            } else {
                ctrl_proto.set_command(prop_ref, &value)
            };
            // The convention recommends QoS 0 for set commands to non-retained properties.
            if !member.retained {
                p.qos = QoS::AtMostOnce;
            }
            let sent_value = truncate(&String::from_utf8_lossy(&p.payload), PAYLOAD_MAX);
            homie_client.homie_publish(p).await?;
            tracing::info!(
                vdevice = %self.prop_ref.device_id(),
                device = %prop_ref.device_id(),
                node = %prop_ref.node_id(),
                property = %prop_ref.prop_id(),
                value = sent_value,
                source = "vdevice",
                "command sent"
            );
        }
        for (_, member) in self.mqtt_compound_members.iter() {
            let Some(output) = member.output.as_ref() else {
                continue;
            };
            let payload = match output.mapping.map_to(&value) {
                MappingResult::Mapped(v) => v.to_owned(),
                MappingResult::Unmapped(v) => v.to_string(),
            };
            mqtt_client
                .publish(&output.topic, HomieMQTTClient::map_qos(&output.qos), output.retained, payload.as_str())
                .await?;
            tracing::info!(
                vdevice = %self.prop_ref.device_id(),
                topic = %output.topic,
                payload = truncate(&payload, PAYLOAD_MAX),
                retain = output.retained,
                source = "vdevice",
                "message published"
            );
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
            tracing::debug!(
                vdevice = %self.prop_ref.device_id(),
                node = %self.prop_ref.node_id(),
                property = %self.prop_ref.prop_id(),
                value = truncate(&value.to_string(), PAYLOAD_MAX),
                "retained value restored from MQTT"
            );
            self.set_value(value, homie_client, homie_proto).await?;
        }

        Ok(())
    }

    pub async fn cancel_read_value_from_mqtt(&mut self, homie_client: &HomieMQTTClient) -> Result<()> {
        if self.read_handle.abort() {
            tracing::debug!(
                vdevice = %self.prop_ref.device_id(),
                node = %self.prop_ref.node_id(),
                property = %self.prop_ref.prop_id(),
                read_timeout_ms = self.options.read_timeout.map(|t| t.as_millis() as u64),
                "no retained value in MQTT before the read timeout"
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

    /// Releases the MQTT subscriptions of all MQTT compound members.
    pub async fn release_mqtt_members(&self, mqtt_client: &ManagedMqttClient) -> Result<()> {
        for topic in self.mqtt_compound_members.keys() {
            mqtt_client.unsubscribe(topic).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_devices::model::MqttWrapperChannel;
    use hc_homie5::client::PendingPublishTracker;
    use hc_homie5::value::{ValueMapping, ValueMappingList};
    use homie5::HomieDomain;

    const TOPIC: &str = "external/button";

    struct TestClient {
        client: HomieMQTTClient,
        observer: hc_homie5::client::PendingPublishObserver,
        // keeps the request channel open so publishes can be queued
        _eventloop: rumqttc::EventLoop,
    }

    impl TestClient {
        fn new() -> Self {
            let (client, eventloop) =
                rumqttc::AsyncClient::new(rumqttc::MqttOptions::new("test", "localhost", 1883), 64);
            let (tracker, observer) = PendingPublishTracker::new();
            Self {
                client: HomieMQTTClient::new(client, tracker.queued_counter()),
                observer,
                _eventloop: eventloop,
            }
        }

        fn published(&self) -> usize {
            self.observer.pending_count()
        }
    }

    fn proto() -> Homie5DeviceProtocol {
        Homie5DeviceProtocol::new("vdev".try_into().unwrap(), HomieDomain::Default).0
    }

    fn vprop(retained: bool) -> VirtualProperty {
        let mut mqtt_compound_members = HashMap::new();
        mqtt_compound_members.insert(
            TOPIC.to_string(),
            MqttCompoundMember {
                value: None,
                input: MqttWrapperChannel {
                    topic: TOPIC.to_string(),
                    mapping: ValueMappingList(vec![ValueMapping {
                        from: None,
                        to: HomieValue::Enum("press-end".to_string()),
                    }]),
                    retained: false,
                    qos: Default::default(),
                },
                output: None,
            },
        );
        VirtualProperty {
            prop_ref: PropertyRef::new(
                HomieDomain::Default,
                "vdev".try_into().unwrap(),
                "button".try_into().unwrap(),
                "action".try_into().unwrap(),
            ),
            value: None,
            retained,
            datatype: HomieDataType::Enum,
            options: Default::default(),
            pass_through: false,
            compound_spec: Some(CompoundSpec::default()),
            prop_compound_members: HashMap::new(),
            mqtt_compound_members,
            query_compound_members: Vec::new(),
            recalc: RecalcTrigger::None,
            has_queries: false,
            read_handle: Default::default(),
            event_pending: false,
        }
    }

    async fn member_event(prop: &mut VirtualProperty, client: &TestClient) {
        prop.update_member_value_mqtt(TOPIC, "single").await.unwrap();
        prop.recalculate_value(&DeviceStore::new(), &client.client, &proto())
            .await
            .unwrap();
    }

    fn compound(debounce: Option<Duration>) -> CompoundSpec {
        CompoundSpec {
            aggregation_debounce: debounce,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn non_retained_compound_without_debounce_recalculates_immediately() {
        let (tx, _rx) = mpsc::channel(10);
        assert!(matches!(RecalcTrigger::for_compound(&compound(None), false, tx.clone()), RecalcTrigger::Immediate(_)));
        assert!(matches!(RecalcTrigger::for_compound(&compound(None), true, tx.clone()), RecalcTrigger::Debounced(_)));
        assert!(matches!(
            RecalcTrigger::for_compound(&compound(Some(Duration::from_millis(50))), false, tx),
            RecalcTrigger::Debounced(_)
        ));
    }

    #[tokio::test]
    async fn member_event_on_immediate_property_requests_inline_recalculation() {
        let (tx, mut rx) = mpsc::channel(10);
        let mut prop = vprop(false);
        prop.recalc = RecalcTrigger::for_compound(&compound(None), false, tx);

        assert!(prop.update_member_value_mqtt(TOPIC, "single").await.unwrap());
        assert!(rx.try_recv().is_err(), "member events must not be queued");
    }

    #[tokio::test(start_paused = true)]
    async fn member_event_on_retained_property_is_debounced() {
        let (tx, mut rx) = mpsc::channel(10);
        let mut prop = vprop(true);
        prop.recalc = RecalcTrigger::for_compound(&compound(None), true, tx);

        assert!(!prop.update_member_value_mqtt(TOPIC, "single").await.unwrap());
        tokio::time::sleep(Duration::from_millis(199)).await;
        assert!(rx.try_recv().is_err());
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert!(matches!(rx.try_recv(), Ok(AppEvent::RecalculateVirtualPropertyValue(_))));
    }

    async fn mqtt_member_property(mqtt_client: &ManagedMqttClient, homie_client: &HomieMQTTClient) -> VirtualProperty {
        let compound_spec: CompoundSpec = serde_yaml_ng::from_str(&format!(
            "members:\n  - mqtt_input: {{ topic: {TOPIC}, mapping: [], qos: AtMostOnce }}\n"
        ))
        .unwrap();
        let (tx, _rx) = mpsc::channel(10);
        VirtualProperty::new(
            vprop(false).prop_ref,
            VirtualPropertyConfig {
                compound_spec: Some(compound_spec),
                ..Default::default()
            },
            None,
            None,
            false,
            HomieDataType::Enum,
            &DeviceStore::new(),
            mqtt_client,
            homie_client,
            tx,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn mqtt_member_subscribes_with_configured_qos() {
        let (mqtt_client, _eventloop) = ManagedMqttClient::offline();
        let client = TestClient::new();
        let _prop = mqtt_member_property(&mqtt_client, &client.client).await;

        assert_eq!(mqtt_client.subscription(TOPIC).await, Some((rumqttc::QoS::AtMostOnce, 1)));
    }

    #[tokio::test]
    async fn reloading_mqtt_member_does_not_leak_subscriptions() {
        let (mqtt_client, _eventloop) = ManagedMqttClient::offline();
        let client = TestClient::new();
        for _ in 0..10 {
            let prop = mqtt_member_property(&mqtt_client, &client.client).await;
            prop.release_mqtt_members(&mqtt_client).await.unwrap();
        }
        let _prop = mqtt_member_property(&mqtt_client, &client.client).await;

        assert_eq!(mqtt_client.subscription(TOPIC).await.map(|(_, count)| count), Some(1));
    }

    #[tokio::test]
    async fn republish_skips_cached_value_of_non_retained_property() {
        let client = TestClient::new();
        let mut prop = vprop(false);
        prop.value = Some(HomieValue::Enum("press-end".to_string()));

        prop.republish_value(&client.client, &proto()).await.unwrap();

        assert_eq!(client.published(), 0);
    }

    #[tokio::test]
    async fn republish_publishes_value_of_retained_property() {
        let client = TestClient::new();
        let mut prop = vprop(true);
        prop.value = Some(HomieValue::Enum("press-end".to_string()));

        prop.republish_value(&client.client, &proto()).await.unwrap();

        assert_eq!(client.published(), 1);
    }

    #[tokio::test]
    async fn identical_member_events_on_non_retained_property_publish_each_time() {
        let client = TestClient::new();
        let mut prop = vprop(false);

        member_event(&mut prop, &client).await;
        member_event(&mut prop, &client).await;

        assert_eq!(client.published(), 2);
    }

    #[tokio::test]
    async fn identical_member_updates_on_retained_property_publish_once() {
        let client = TestClient::new();
        let mut prop = vprop(true);

        member_event(&mut prop, &client).await;
        member_event(&mut prop, &client).await;

        assert_eq!(client.published(), 1);
    }

    #[tokio::test]
    async fn recalculation_without_member_event_does_not_republish_non_retained_value() {
        let client = TestClient::new();
        let mut prop = vprop(false);

        member_event(&mut prop, &client).await;
        // structural recalculation (e.g. query membership change) without a new member event
        prop.recalculate_value(&DeviceStore::new(), &client.client, &proto())
            .await
            .unwrap();

        assert_eq!(client.published(), 1);
    }

    #[tokio::test]
    async fn identical_set_values_on_non_retained_property_publish_each_time() {
        let client = TestClient::new();
        let mut prop = vprop(false);
        let value = HomieValue::Enum("press-end".to_string());

        prop.set_value(value.clone(), &client.client, &proto()).await.unwrap();
        prop.set_value(value, &client.client, &proto()).await.unwrap();

        assert_eq!(client.published(), 2);
    }

    #[tokio::test]
    async fn identical_set_values_on_retained_property_publish_once() {
        let client = TestClient::new();
        let mut prop = vprop(true);
        let value = HomieValue::Enum("press-end".to_string());

        prop.set_value(value.clone(), &client.client, &proto()).await.unwrap();
        prop.set_value(value, &client.client, &proto()).await.unwrap();

        assert_eq!(client.published(), 1);
    }
}
