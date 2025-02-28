use color_eyre::eyre::{self, eyre};
use homie5::{HomieDomain, HomieID};
use once_cell::sync::Lazy;
use rand::{distr::Alphanumeric, Rng};
use simple_kv_store::KubernetesResource;
use std::{env, path::PathBuf, str::FromStr};

use crate::unwrap_or_exit::UnwrapOrExit;

// pub static ENV_PREFIX: Lazy<String> = Lazy::new(|| env!("CARGO_CRATE_NAME").replace('-', "_").to_uppercase());
pub static ENV_PREFIX: Lazy<String> = Lazy::new(|| "HCACTL".to_string());

pub static SETTINGS: Lazy<Settings> = Lazy::new(Settings::default);

pub const CHANNEL_CAPACITY: usize = 65535;

fn env_name(name: &str) -> String {
    format!("{}_{}", *ENV_PREFIX, name)
}

#[derive(Default, Debug)]
pub struct Settings {
    pub homie: HomieSettings,
    pub app: AppSettings,
}

#[derive(Debug)]
pub struct AppSettings {
    pub rules_config: ConfigBackend,
    pub virtual_devices_config: ConfigBackend,
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
            rules_config: generic_setting(
                "RULES_CONFIG",
                ConfigBackend::File {
                    path: PathBuf::from("./rules"),
                },
            ),
            virtual_devices_config: generic_setting(
                "VIRTUAL_DEVICES_CONFIG",
                ConfigBackend::File {
                    path: PathBuf::from("./virtual_devices"),
                },
            ),
            value_store_config: generic_setting("VALUE_STORE_CONFIG", ValueStoreConfig::InMemory),
            location: generic_setting(
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

#[derive(Debug)]
pub struct HomieSettings {
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub client_id: String,
    pub homie_domain: HomieDomain,
    pub controller_id: HomieID,
    pub controller_name: String,
}

impl Default for HomieSettings {
    fn default() -> Self {
        let hostname = string_setting("HOMIE_HOST", "localhost");
        let port = number_setting("HOMIE_PORT", 1883u16);

        let username = string_setting("HOMIE_USERNAME", String::default());
        let password = string_setting("HOMIE_PASSWORD", String::default());
        let client_id = string_setting(
            "HOMIE_CLIENT_ID",
            format!(
                "hcactl-{}",
                rand::rng()
                    .sample_iter(&Alphanumeric)
                    .take(8)
                    .map(char::from)
                    .collect::<String>()
            ),
        );
        let homie_domain = generic_setting("HOMIE_DOMAIN", HomieDomain::Default);
        let controller_id = generic_setting("HOMIE_CTRL_ID", HomieID::new_const("hc-homie5-automation-ctrl"));
        let controller_name = string_setting("HOMIE_CTRL_NAME", "Homecontrol Automation Controller");

        Self {
            hostname,
            port,
            username,
            password,
            client_id,
            homie_domain,
            controller_id,
            controller_name,
        }
    }
}

fn string_setting(name: &str, default: impl Into<String>) -> String {
    env::var(env_name(name)).ok().unwrap_or(default.into())
}

fn number_setting<T>(name: &str, default: T) -> T
where
    T: FromStr,
    T::Err: std::fmt::Display, // Explicit Debug requirement
{
    env::var(env_name(name))
        .ok()
        .map(|value| value.parse::<T>().unwrap_or_exit("Not a valid number!"))
        .unwrap_or(default)
}

fn generic_setting<T>(name: &str, default: T) -> T
where
    T: TryFrom<String>,
    T::Error: std::fmt::Display, // Explicit Debug requirement
{
    env::var(env_name(name))
        .ok()
        .map(|value| value.try_into().unwrap_or_exit("Invalid setting supplied!"))
        .unwrap_or(default)
}
