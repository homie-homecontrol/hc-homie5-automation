use std::{fs, iter, time::Duration};

use color_eyre::eyre::Result;
use hc_homie5_automation::{
    app_state::{AppEvent, AppState, ConnectionState},
    lua_runtime::LuaModuleManager,
    meta::{MetaConfig, MetaManager},
    utils::throttle_channel,
};
use homie5::set_fallback_homie_domain;
use simple_kv_store::{InMemoryStore, KeyValueStore, KubernetesStore, SQLiteStore};
use tokio::sync::mpsc::{self};

use crate::eventloop::EventMultiPlexer;

use crate::settings::{ConfigBackend, ValueStoreConfig, CHANNEL_CAPACITY, SETTINGS};
use config_watcher::{backend, config_item_watcher::run_config_item_watcher, Tokenizer, WatcherError, YamlTokenizer};
use hc_homie5::client::HomieClientHandle;
use hc_homie5_automation::{
    cron_manager::CronManager,
    device_manager::DeviceManager,
    mqtt_client::{run_mqtt_client, MqttClientHandle},
    rule_manager::RuleManager,
    rules::Rule,
    solar_events::{run_solar_event_task, SolarEventHandle},
    timer_manager::TimerManager,
    virtual_devices::{VirtualDeviceManager, VirtualDeviceSpec},
};
pub struct LuaFileTokenizer;

impl Tokenizer for LuaFileTokenizer {
    fn tokenize<'a>(&self, content: &'a str) -> Box<dyn Iterator<Item = &'a str> + 'a> {
        Box::new(iter::once(content))
    }
}

/// Starts the config-watcher backend for one config kind based on its
/// configured [`ConfigBackend`].
///
/// * `kind` — label used in the missing-folder panic message
///   (`"Configured {kind} folder [..] does not exist!"`)
/// * `log_kind` — label used in the backend-selection debug logs
/// * `file_glob` — file pattern watched by the `File` backend (e.g. `*.yaml`)
/// * `client_id_suffix` — appended to the MQTT client id for the `Mqtt`
///   backend's dedicated connection (e.g. `-cfg-r`)
fn run_config_backend(
    settings: &crate::settings::Settings,
    backend_config: &ConfigBackend,
    kind: &str,
    log_kind: &str,
    file_glob: &str,
    client_id_suffix: &str,
) -> std::result::Result<(backend::WatcherHandle, mpsc::Receiver<backend::DocumentEvent>), WatcherError> {
    match backend_config {
        ConfigBackend::File { path } => {
            let absolute_path = fs::canonicalize(path).unwrap_or_else(|_| {
                panic!("Configured {} folder [{}] does not exist!", kind, path.as_os_str().to_string_lossy())
            });
            backend::run_config_file_watcher(absolute_path, file_glob)
        }
        ConfigBackend::Kubernetes { name, namespace } => {
            log::debug!("Using Kubernetes backend for {}", log_kind);
            backend::run_configmap_watcher(name.to_string(), namespace.to_string())
        }
        ConfigBackend::Mqtt { topic } => {
            let mco = settings
                .homie
                .to_mqtt_client_config()
                .client_id(format!("{}{}", &settings.homie.client_id, client_id_suffix));
            log::debug!("Using Mqtt backend for {}", log_kind);
            backend::run_mqtt_watcher(mco.to_mqtt_options().expect("MQTT TLS configuration error"), topic, 1024)
        }
    }
}

pub async fn initialize_app(
) -> Result<(EventMultiPlexer, HomieClientHandle, HomieClientHandle, MqttClientHandle, SolarEventHandle, AppState)> {
    let settings = &SETTINGS;

    set_fallback_homie_domain(settings.homie.homie_domain.clone());

    let (app_event_sender, app_event_receiver) = mpsc::channel::<AppEvent>(CHANNEL_CAPACITY);

    let homie_client_options = settings.homie.to_mqtt_client_config();

    // Setup device discovery
    // =====================================================
    let (dm, homie_discovery_client_handle, homie_event_receiver) =
        DeviceManager::new(settings.homie.homie_domain.clone(), &homie_client_options)?;

    // Setup MQTT Client
    // ===============================================

    let mqtt_client_options = settings
        .homie
        .to_mqtt_client_config()
        .client_id(format!("{}-mqtt", &settings.homie.client_id));

    let (mqtt_client_handle, mqtt_client, mqtt_event_receiver) =
        run_mqtt_client(mqtt_client_options.to_mqtt_options()?, CHANNEL_CAPACITY)?;

    // Setup homie device for controller
    // =====================================================

    let (vdm, homie_ctl_device_client_handle, homie_device_event_receiver) = VirtualDeviceManager::new(
        dm.clone(),
        mqtt_client.clone(),
        app_event_sender.clone(),
        crate::settings::vdm_config_from_homie(&settings.homie, &settings.app),
    )
    .await?;

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

    // Setup configuration watchers
    // =====================================================

    let deserialize_rules = |doc: &str| serde_yaml_ng::from_str(doc);

    let (rules_watcher_handle, rules_receiver) = run_config_item_watcher::<Rule, _>(
        || run_config_backend(settings, &settings.app.rules_config, "Rules", "rules", "*.yaml", "-cfg-r"),
        &YamlTokenizer,
        deserialize_rules,
    )?;

    let deserialize_virtual_devices = |doc: &str| serde_yaml_ng::from_str(doc);

    let (vdevices_watcher_handle, vdevices_receiver) = run_config_item_watcher::<VirtualDeviceSpec, _>(
        || {
            run_config_backend(
                settings,
                &settings.app.virtual_devices_config,
                "Virtual Devices",
                "virtual devices",
                "*.yaml",
                "-cfg-vd",
            )
        },
        &YamlTokenizer,
        deserialize_virtual_devices,
    )?;

    let deserialize_lua_file = |doc: &str| -> std::result::Result<String, WatcherError> { Ok(doc.to_string()) };

    let (lua_files_watcher_handle, lua_files_receiver) = run_config_item_watcher::<String, _>(
        || {
            run_config_backend(
                settings,
                &settings.app.lua_files_config,
                "Lua modules",
                "lua modules",
                "*.lua",
                "-cfg-lua",
            )
        },
        &LuaFileTokenizer,
        deserialize_lua_file,
    )?;

    let deserialize_meta = |doc: &str| -> std::result::Result<MetaConfig, String> {
        serde_yaml_ng::from_str::<MetaConfig>(doc).map_err(|err| {
            let preview = doc.lines().take(3).collect::<Vec<_>>().join(" | ");
            let msg = format!(
                "Meta overlay parse error: {}. Hint: annotation values must be strings or string arrays. Document preview: {}",
                err, preview
            );
            log::error!("{}", msg);
            msg
        })
    };

    let (meta_watcher_handle, meta_receiver) = run_config_item_watcher::<MetaConfig, _>(
        || run_config_backend(settings, &settings.app.meta_config, "meta", "meta overlays", "*.yaml", "-cfg-meta"),
        &YamlTokenizer,
        deserialize_meta,
    )?;
    // Simple Value store
    // =====================================================
    let value_store = match &settings.app.value_store_config {
        ValueStoreConfig::InMemory => KeyValueStore::InMemory(InMemoryStore::new()),
        ValueStoreConfig::Kubernetes {
            name,
            namespace,
            resource_type,
        } => KeyValueStore::Kubernetes(KubernetesStore::new(namespace, name, *resource_type).await?),
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
        lua_files_receiver,
        throttle_channel(meta_receiver, Duration::from_millis(10)),
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
            mqtt_client: mqtt_client.clone(),
            app_event_sender,
            should_exit: false,
            mqtt_state: ConnectionState::Init,
            discovery_state: ConnectionState::Init,
            virtual_devices_state: ConnectionState::Init,
            lua_module_manager: LuaModuleManager::new(),
            meta: MetaManager::new(settings.homie.homie_domain.clone(), mqtt_client.clone()),
            meta_handler: hc_homie5::controller::MetaOverlayHandler::new(settings.homie.homie_domain.clone()),
            value_store,
            rule_watcher_handle: rules_watcher_handle,
            virtual_devices_watcher_handle: vdevices_watcher_handle,
            meta_watcher_handle,
            lua_files_watcher_handle,
            lua_script_timeout: settings.app.lua_script_timeout,
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

    // After the Exit path requested clean disconnects, each client poll loop
    // exits on its own once it processes `Outgoing::Disconnect` (sending a
    // clean DISCONNECT packet so the broker discards the LWT). The graceful
    // stop waits briefly for that natural exit instead of racing it with the
    // stop signal, which could tear down the TCP connection uncleanly and
    // fire the LWT (`$state = lost`).
    const STOP_GRACE: Duration = Duration::from_secs(2);

    mqtt_client_handle.stop_graceful(STOP_GRACE).await?;

    // once the mqtt connection is closed the discovery task will exit.
    // this is to ensure we wait until this happens before discarding the discovery object
    homie_discovery_client_handle.stop_graceful(STOP_GRACE).await?;

    homie_ctrl_device_client_handle.stop_graceful(STOP_GRACE).await?;
    log::debug!("Deinitialized app...");

    Ok(())
}
