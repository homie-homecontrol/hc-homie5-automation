use color_eyre::eyre::{self, eyre};
use homie5::{HomieDomain, HomieID};
use once_cell::sync::Lazy;
use simple_kv_store::KubernetesResource;
use std::{path::PathBuf, str::FromStr};

use hc_homie5::settings::{self, HomieSettings};
use hc_homie5_automation::virtual_devices::VirtualDeviceManagerConfig;

// pub static ENV_PREFIX: Lazy<String> = Lazy::new(|| env!("CARGO_CRATE_NAME").replace('-', "_").to_uppercase());
pub static ENV_PREFIX: Lazy<String> = Lazy::new(|| "HCACTL".to_string());

pub static SETTINGS: Lazy<Settings> = Lazy::new(Settings::default);

pub const CHANNEL_CAPACITY: usize = 65535;

#[derive(Debug)]
pub struct Settings {
    pub homie: HomieSettings,
    pub app: AppSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            homie: HomieSettings::from_env(&ENV_PREFIX, "hcactl-", HomieDomain::Default),
            app: AppSettings::default(),
        }
    }
}

#[derive(Debug)]
pub struct AppSettings {
    pub rules_config: ConfigBackend,
    pub virtual_devices_config: ConfigBackend,
    pub meta_config: ConfigBackend,
    pub lua_files_config: ConfigBackend,
    pub value_store_config: ValueStoreConfig,
    pub location: LocationConfig,
}

/// - `latitude`: Latitude for the solar calculation.
/// - `longitude`: Longitude for the solar calculation.
/// - `height`: Observer height in meters above sea level.
#[derive(Debug)]
pub enum ConfigBackend {
    File { path: PathBuf },
    Kubernetes { name: String, namespace: String },
    Mqtt { topic: String },
}

impl TryFrom<String> for ConfigBackend {
    type Error = eyre::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let cb = value.parse()?;
        Ok(cb)
    }
}

impl FromStr for ConfigBackend {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() != 2 {
            println!("Error parsing config backend: {}", s);
            return Err(eyre!("Invalid format. Use 'file:/path', 'mqtt:topic' or 'kubernetes:name[,namespace]'"));
        }

        match parts[0].to_lowercase().as_str() {
            "file" => Ok(ConfigBackend::File {
                path: PathBuf::from(parts[1]),
            }),
            "mqtt" => Ok(ConfigBackend::Mqtt {
                topic: parts[1].to_string(),
            }),
            "kubernetes" => {
                let kube_parts: Vec<&str> = parts[1].splitn(2, ',').collect();
                let name = kube_parts[0].to_string();
                let namespace = if kube_parts.len() == 2 {
                    kube_parts[1].to_string()
                } else {
                    "default".to_string() // Use "default" if namespace is missing
                };
                Ok(ConfigBackend::Kubernetes { name, namespace })
            }
            _ => {
                println!("Error parsing config backend: {}", s);
                Err(eyre!("Unknown backend type. Use 'file' or 'kubernetes'"))
            }
        }
    }
}

#[derive(Debug)]
pub enum ValueStoreConfig {
    InMemory,
    Kubernetes {
        name: String,
        namespace: String,
        ressource_type: KubernetesResource,
    },
    Sqlite {
        path: String,
    },
}

impl TryFrom<String> for ValueStoreConfig {
    type Error = eyre::Report;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();

        match parts[0].to_lowercase().as_str() {
            "inmemory" => Ok(ValueStoreConfig::InMemory),

            "sqlite" if parts.len() == 2 => Ok(ValueStoreConfig::Sqlite {
                path: parts[1].to_string(),
            }),
            "kubernetes" if parts.len() >= 2 => {
                let kube_parts: Vec<&str> = parts[1].splitn(3, ',').collect();
                let ressource_type = match kube_parts[0] {
                    "secret" => KubernetesResource::Secret,
                    "configmap" => KubernetesResource::ConfigMap,
                    _ => KubernetesResource::ConfigMap,
                };
                let name = kube_parts[1].to_string();
                let namespace = if kube_parts.len() == 3 {
                    kube_parts[2].to_string()
                } else {
                    "default".to_string() // Use "default" if namespace is missing
                };
                Ok(ValueStoreConfig::Kubernetes {
                    name,
                    namespace,
                    ressource_type,
                })
            }
            _ => {
                println!("Error parsing value store config: {}", s);
                Err(eyre!("Invalid format. Use 'inmemory', 'sqlite:/path/to/filename.db' or 'kubernetes:secret|configmap,name[,namespace]'"))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocationConfig {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: f64,
}

impl TryFrom<String> for LocationConfig {
    type Error = eyre::Report;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = s.splitn(3, ',').collect();
        if parts.len() != 3 {
            println!("Error parsing value store config: {}", s);
            return Err(eyre!("Invalid format. Use '<latitude>,<longitude>,<elevation>'"));
        }
        let latitude: f64 = parts[0].parse()?;
        let longitude: f64 = parts[1].parse()?;
        let elevation: f64 = parts[2].parse()?;
        Ok(LocationConfig {
            latitude,
            longitude,
            elevation,
        })
    }
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            rules_config: settings::generic_setting(
                &ENV_PREFIX,
                "RULES_CONFIG",
                ConfigBackend::File {
                    path: PathBuf::from("./rules"),
                },
            ),
            virtual_devices_config: settings::generic_setting(
                &ENV_PREFIX,
                "VIRTUAL_DEVICES_CONFIG",
                ConfigBackend::File {
                    path: PathBuf::from("./virtual_devices"),
                },
            ),
            meta_config: settings::generic_setting(
                &ENV_PREFIX,
                "META_CONFIG",
                ConfigBackend::File {
                    path: PathBuf::from("./meta"),
                },
            ),
            lua_files_config: settings::generic_setting(
                &ENV_PREFIX,
                "LUA_MODULE_CONFIG",
                ConfigBackend::File {
                    path: PathBuf::from("./lua"),
                },
            ),
            value_store_config: settings::generic_setting(
                &ENV_PREFIX,
                "VALUE_STORE_CONFIG",
                ValueStoreConfig::InMemory,
            ),
            location: settings::generic_setting(
                &ENV_PREFIX,
                "LOCATION",
                LocationConfig {
                    longitude: 0f64,
                    latitude: 0f64,
                    elevation: 0f64,
                },
            ),
        }
    }
}

pub fn vdm_config_from_homie(settings: &HomieSettings) -> VirtualDeviceManagerConfig {
    VirtualDeviceManagerConfig {
        hostname: settings.hostname.clone(),
        port: settings.port,
        username: settings.username.clone(),
        password: settings.password.clone(),
        client_id: settings.client_id.clone(),
        homie_domain: settings.homie_domain.clone(),
        controller_id: settings
            .controller_id
            .clone()
            .unwrap_or_else(|| HomieID::new_const("hc-homie5-automation-ctrl")),
        controller_name: settings
            .controller_name
            .clone()
            .unwrap_or_else(|| "Homecontrol Automation Controller".to_string()),
    }
}
