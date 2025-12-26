use std::sync::Mutex;

use color_eyre::eyre::Result;
use hc_homie5::HomieMQTTClient;
use homie5::{Homie5ControllerProtocol, HomieDomain, HomieValue, PropertyRef};
use once_cell::sync::Lazy;

static DEFAULT_HOMIE_DOMAIN: Lazy<Mutex<HomieDomain>> = Lazy::new(|| Mutex::new(HomieDomain::Default));

pub fn set_default_homie_domain(domain: HomieDomain) {
    *DEFAULT_HOMIE_DOMAIN.lock().unwrap() = domain;
}

pub fn get_default_homie_domain() -> HomieDomain {
    DEFAULT_HOMIE_DOMAIN.lock().unwrap().clone()
}

#[derive(Clone)]
pub struct HomieControllerClient {
    protocol: Homie5ControllerProtocol,
    homie_client: HomieMQTTClient,
}

impl HomieControllerClient {
    pub fn new(protocol: Homie5ControllerProtocol, homie_client: HomieMQTTClient) -> Self {
        Self { protocol, homie_client }
    }

    pub async fn set_command(&self, prop: &PropertyRef, value: &HomieValue) -> Result<()> {
        self.homie_client
            .homie_publish(self.protocol.set_command(prop, value))
            .await?;
        Ok(())
    }

    pub fn protocol(&self) -> &Homie5ControllerProtocol {
        &self.protocol
    }

    #[allow(dead_code)]
    pub fn homie_client(&self) -> &HomieMQTTClient {
        &self.homie_client
    }
}
