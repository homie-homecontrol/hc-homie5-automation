use color_eyre::eyre::Result;
use hc_homie5::HomieMQTTClient;
use homie5::{Homie5ControllerProtocol, HomieValue, PropertyRef};

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
