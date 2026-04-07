use std::collections::HashMap;

use homie5::{
    extensions::meta::{MetaDeviceLevel, MetaProviderInfo},
    HomieID,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct MetaConfig {
    pub provider: MetaProviderConfig,
    #[serde(default)]
    pub devices: HashMap<HomieID, MetaDeviceLevel>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetaProviderConfig {
    pub id: HomieID,
    #[serde(flatten)]
    pub info: MetaProviderInfo,
}
