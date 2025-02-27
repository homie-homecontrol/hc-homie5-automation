use color_eyre::eyre;
use hc_homie5::{homie_device, HomieDevice};
use homie5::{device_description::DeviceDescriptionBuilder, PropertyRef};

#[homie_device]
#[derive(Debug)]
pub struct VirtualDeviceRoot {}

impl VirtualDeviceRoot {
    pub fn new(
        homie_id: HomieID,
        name: String,
        homie_proto: Homie5DeviceProtocol,
        homie_client: &HomieMQTTClient,
    ) -> Result<Self, eyre::Error> {
        let device_desc = DeviceDescriptionBuilder::new().name(name);

        Ok(Self {
            device_ref: DeviceRef::new(homie_proto.homie_domain().clone(), homie_id),
            status: HomieDeviceStatus::Init,
            device_desc: device_desc.build(),
            homie_proto,
            homie_client: homie_client.clone(),
        })
    }
}

impl HomieDevice for VirtualDeviceRoot {
    type ResultError = eyre::Error;

    async fn publish_property_values(&mut self) -> Result<(), Self::ResultError> {
        log::debug!("This is the overritten implementation");
        Ok(())
    }

    async fn handle_set_command(&mut self, _property: &PropertyRef, _set_value: &str) -> Result<(), Self::ResultError> {
        Ok(())
    }
}
