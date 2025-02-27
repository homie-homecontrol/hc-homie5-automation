use std::sync::Arc;

use color_eyre::eyre::Result;
use hc_homie5::{
    run_homie_client, DeviceStore, DiscoveryAction, HomieClientEvent, HomieClientHandle, HomieDiscovery,
    MqttClientConfig,
};
use homie5::{Homie5ControllerProtocol, Homie5Message, HomieDomain, HomieValue, PropertyRef};
use tokio::sync::{mpsc, RwLock};

use crate::homie::HomieControllerClient;

#[derive(Clone)]
pub struct DeviceManager {
    devices: Arc<RwLock<DeviceStore>>,
    ctrl_client: HomieControllerClient,
    discovery: HomieDiscovery,
    homie_domain: HomieDomain,
}

impl DeviceManager {
    pub fn new(
        homie_domain: HomieDomain,
        homie_client_options: &MqttClientConfig,
    ) -> Result<(Self, HomieClientHandle, mpsc::Receiver<HomieClientEvent>)> {
        let (homie_client_handle, homie_mqtt_client, homie_event_receiver) =
            run_homie_client(homie_client_options.to_mqtt_options(), homie_client_options.mqtt_channel_size)?;

        let devices = Arc::new(RwLock::new(DeviceStore::new()));
        let discovery = HomieDiscovery::new(homie_mqtt_client.clone());
        let ctrl_client = HomieControllerClient::new(Homie5ControllerProtocol::new(), homie_mqtt_client);

        Ok((
            Self {
                devices,
                discovery,
                ctrl_client,
                homie_domain,
            },
            homie_client_handle,
            homie_event_receiver,
        ))
    }

    pub async fn discover(&self) -> Result<()> {
        self.discovery.discover(&self.homie_domain).await?;
        Ok(())
    }

    pub async fn stop_discover(&self) -> Result<()> {
        self.discovery.stop_discover(&self.homie_domain).await?;
        Ok(())
    }

    pub async fn discovery_handle_event(&self, message: Homie5Message) -> Result<Option<DiscoveryAction>> {
        let mut devices = self.devices.write().await;
        Ok(self.discovery.handle_event(message, &mut devices).await?)
    }

    pub async fn set_command(&self, target: &PropertyRef, value: &HomieValue) -> Result<()> {
        self.ctrl_client.set_command(target, value).await?;
        Ok(())
    }

    pub async fn disconnect_client(&self) -> Result<()> {
        self.ctrl_client.homie_client().disconnect().await?;
        Ok(())
    }

    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, DeviceStore> {
        self.devices.read().await
    }

    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, DeviceStore> {
        self.devices.write().await
    }
}
