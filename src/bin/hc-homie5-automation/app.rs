use std::{fs, time::Duration};

use color_eyre::eyre::Result;
use hc_homie5_automation::{
    app_state::{AppEvent, AppState, ConnectionState},
    utils::throttle_channel,
};
use simple_kv_store::{InMemoryStore, KeyValueStore, KubernetesStore, SQLiteStore};
use tokio::sync::mpsc::{self};

use crate::eventloop::EventMultiPlexer;

use config_watcher::{backend, config_item_watcher::run_config_item_watcher, YamlTokenizer};
use hc_homie5::{HomieClientHandle, MqttClientConfig};
use hc_homie5_automation::{
    cron_manager::CronManager,
    device_manager::DeviceManager,
    mqtt_client::{run_mqtt_client, MqttClientHandle},
    rule_manager::RuleManager,
    rules::Rule,
    settings::{ConfigBackend, ValueStoreConfig, CHANNEL_CAPACITY, SETTINGS},
    solar_events::{run_solar_event_task, SolarEventHandle},
    timer_manager::TimerManager,
    virtual_devices::{VirtualDeviceManager, VirtualDeviceSpec},
};

pub async fn initialize_app(
) -> Result<(EventMultiPlexer, HomieClientHandle, HomieClientHandle, MqttClientHandle, SolarEventHandle, AppState)> {
    let settings = &SETTINGS;

    let store = KeyValueStore::SQLite(SQLiteStore::new("store.db").await);
    let value = 123; // define a owned string value
    store.set("db_key", &value).await.unwrap(); // store the value
    println!("Value: {}", store.get::<i64>("db_key").await.unwrap()); // retrieve the value -- type annotations are
                                                                      // needed in this case as no type can be deferred
                                                                      // from the println! usage
    let (app_event_sender, app_event_receiver) = mpsc::channel::<AppEvent>(CHANNEL_CAPACITY);

    let homie_client_options = MqttClientConfig::new(&settings.homie.hostname)
        .client_id(&settings.homie.client_id)
        .port(settings.homie.port)
        .username(&settings.homie.username)
        .password(&settings.homie.password);

    // Setup device discovery
    // =====================================================
    let (dm, homie_discovery_client_handle, homie_event_receiver) =
        DeviceManager::new(settings.homie.homie_domain.clone(), &homie_client_options)?;

    // Setup MQTT Client
    // ===============================================

    let mqtt_client_options = MqttClientConfig::new(&settings.homie.hostname)
        .client_id(format!("{}-mqtt", &settings.homie.client_id))
        .port(settings.homie.port)
        .username(&settings.homie.username)
        .password(&settings.homie.password);

    let (mqtt_client_handle, mqtt_client, mqtt_event_receiver) =
        run_mqtt_client(mqtt_client_options.to_mqtt_options(), 65535)?;

    // Setup homie device for controller
    // =====================================================

    let (vdm, homie_ctl_device_client_handle, homie_device_event_receiver) =
        VirtualDeviceManager::new(dm.clone(), mqtt_client.clone(), app_event_sender.clone(), settings).await?;

    // Setup Timers and Cron
    // =====================================================
    let (timers, timers_receiver) = TimerManager::new();

    let (cron, cron_receiver) = CronManager::new();

    let (solar_event_handler, solar_events, solar_events_receiver) = run_solar_event_task(
        settings.app.location.latitude,
        settings.app.location.longitude,
        settings.app.location.elevation,
        10,
    );
    // let (solar_event_handler, solar_events, solar_events_receiver) = run_solar_event_task(48.166, 11.5683, 0.0, 10);

    // Setup configuration watchers
    // =====================================================

    let deserialize_rules = |doc: &str| serde_yml::from_str(doc);

    let (rules_watcher_handle, rules_receiver) = run_config_item_watcher::<Rule, _>(
        || match &settings.app.rules_config {
            ConfigBackend::File { path } => {
                backend::run_config_file_watcher(fs::canonicalize(path).unwrap(), "*.yaml", Duration::from_millis(500))
            }
            ConfigBackend::Kubernetes { name, namespace } => {
                backend::run_configmap_watcher(name.to_string(), namespace.to_string())
            }
            ConfigBackend::Mqtt { topic } => {
                let mco = MqttClientConfig::new(&settings.homie.hostname)
                    .client_id(format!("{}-cfg-r", &settings.homie.client_id))
                    .port(settings.homie.port)
                    .username(&settings.homie.username)
                    .password(&settings.homie.password);
                log::debug!("Using Mqtt backend for virtual devices");
                backend::run_mqtt_watcher(mco.to_mqtt_options(), topic, 1024)
            }
        },
        &YamlTokenizer,
        deserialize_rules,
    )?;

    let deserialize_virtual_devices = |doc: &str| serde_yml::from_str(doc);

    let (vdevices_watcher_handle, vdevices_receiver) = run_config_item_watcher::<VirtualDeviceSpec, _>(
        || match &settings.app.virtual_devices_config {
            ConfigBackend::File { path } => {
                backend::run_config_file_watcher(fs::canonicalize(path).unwrap(), "*.yaml", Duration::from_millis(500))
            }
            ConfigBackend::Kubernetes { name, namespace } => {
                log::debug!("Using Kubernetes backend for virtual devices");
                backend::run_configmap_watcher(name.to_string(), namespace.to_string())
            }
            ConfigBackend::Mqtt { topic } => {
                let mco = MqttClientConfig::new(&settings.homie.hostname)
                    .client_id(format!("{}-cfg-vd", &settings.homie.client_id))
                    .port(settings.homie.port)
                    .username(&settings.homie.username)
                    .password(&settings.homie.password);
                log::debug!("Using Mqtt backend for virtual devices");
                backend::run_mqtt_watcher(mco.to_mqtt_options(), topic, 1024)
            }
        },
        &YamlTokenizer,
        deserialize_virtual_devices,
    )?;

    // Simple Value store
    // =====================================================
    let value_store = match &settings.app.value_store_config {
        ValueStoreConfig::InMemory => KeyValueStore::InMemory(InMemoryStore::new()),
        ValueStoreConfig::Kubernetes {
            name,
            namespace,
            ressource_type,
        } => KeyValueStore::Kubernetes(KubernetesStore::new(namespace, name, *ressource_type).await?),
        ValueStoreConfig::Sqlite { path } => KeyValueStore::SQLite(SQLiteStore::new(path).await),
    };

    // Setup EventMultiPlexer
    // =====================================================
    let event_multiplexer = EventMultiPlexer::new(
        app_event_receiver,
        homie_event_receiver,
        homie_device_event_receiver,
        // throttle new rule detection and initialization to make sure not to overload the mqtt broker
        throttle_channel(rules_receiver, Duration::from_millis(10)),
        // throttle new device detection and creation to make sure not to overload the mqtt broker
        throttle_channel(vdevices_receiver, Duration::from_millis(10)),
        timers_receiver,
        cron_receiver,
        mqtt_event_receiver,
        solar_events_receiver,
    );

    Ok((
        event_multiplexer,
        homie_discovery_client_handle,
        homie_ctl_device_client_handle,
        mqtt_client_handle,
        solar_event_handler,
        AppState {
            dm,
            rules: RuleManager::new(),
            vdm,
            timers,
            solar_events,
            cron,
            mqtt_client,
            app_event_sender,
            should_exit: false,
            mqtt_state: ConnectionState::Init,
            discovery_state: ConnectionState::Init,
            virtual_devices_state: ConnectionState::Init,
            value_store,
            rule_watcher_handle: rules_watcher_handle,
            virtual_devices_watcher_handle: vdevices_watcher_handle,
        },
    ))
}

pub async fn deinitialize_app(
    homie_discovery_client_handle: HomieClientHandle,
    homie_ctrl_device_client_handle: HomieClientHandle,
    mqtt_client_handle: MqttClientHandle,
    solar_events_handle: SolarEventHandle,
) -> Result<()> {
    solar_events_handle.stop().await;

    mqtt_client_handle.stop().await?;

    // once the mqtt connection is closed the discovery task will exit.
    // this is to ensure we wait until this happens before discarding the discovery object
    homie_discovery_client_handle.stop().await?;

    homie_ctrl_device_client_handle.stop().await?;
    log::debug!("Deinitialized app...");

    Ok(())
}
