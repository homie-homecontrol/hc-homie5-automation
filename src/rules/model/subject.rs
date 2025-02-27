use color_eyre::eyre::{self, eyre, Result};
use homie5::{DeviceRef, HomieDomain, HomieID, NodeRef, PropertyRef};
use serde::de;
use serde::{Deserialize, Deserializer};
use std::ops::Deref;

use crate::settings::SETTINGS;

// Enum to handle both string and object representations
#[derive(Clone, Deserialize, Debug)]
#[serde(untagged)]
pub enum SubjectInput {
    StringRepresentation(String),
    ObjectRepresentation {
        #[serde(default)]
        homie_domain: Option<HomieDomain>,
        device_id: HomieID,
        node_id: HomieID,
        prop_id: HomieID,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subject(PropertyRef);

impl Deref for Subject {
    type Target = PropertyRef;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Assuming external_crate::PropertyRef exists
impl PartialEq<PropertyRef> for Subject {
    fn eq(&self, other: &PropertyRef) -> bool {
        // Delegate comparison to the inner PropertyRef
        &self.0 == other
    }
}

// Optional: Implement PartialEq for reverse comparison
impl PartialEq<Subject> for PropertyRef {
    fn eq(&self, other: &Subject) -> bool {
        self == &other.0
    }
}

// Implement custom deserialization for PropertyRef
impl<'de> Deserialize<'de> for Subject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let subject = SubjectInput::deserialize(deserializer)?;
        let property_ref = match subject {
            SubjectInput::StringRepresentation(s) => {
                PropertyRef::from_subject_string(&s).map_err(|e| de::Error::custom(e.to_string()))
            }
            SubjectInput::ObjectRepresentation {
                homie_domain,
                device_id,
                node_id,
                prop_id,
            } => Ok(PropertyRef::new(
                homie_domain.unwrap_or(SETTINGS.homie.homie_domain.clone()),
                device_id,
                node_id,
                prop_id,
            )),
        };
        property_ref.map(Subject)
    }
}

impl From<Subject> for PropertyRef {
    fn from(value: Subject) -> Self {
        value.0
    }
}

pub trait ToSubjectStr {
    fn to_subject_string(&self) -> String;
}

pub trait FromSubjectStr
where
    Self: Sized,
{
    type ConverError;
    fn from_subject_string(s: &str) -> std::result::Result<Self, Self::ConverError>;
}

impl ToSubjectStr for PropertyRef {
    fn to_subject_string(&self) -> String {
        format!("{}/{}/{}/{}", self.homie_domain(), self.device_id(), self.node_id(), self.prop_id())
    }
}

impl FromSubjectStr for PropertyRef {
    type ConverError = eyre::Error;

    fn from_subject_string(s: &str) -> Result<PropertyRef> {
        // Split the string into parts
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() < 3 || parts.len() > 4 {
            return Err(eyre!(format!("Invalid string format for PropertyRef: '{}'", s)));
        }
        let homie_domain = if parts.len() == 4 {
            HomieDomain::try_from(parts[0].to_string()).map_err(|e| eyre!(format!("{}", e)))?
        } else {
            SETTINGS.homie.homie_domain.clone()
        };
        let device_id = parts[parts.len() - 3]
            .to_string()
            .try_into()
            .map_err(|e| eyre!(format!("{}", e)))?;
        let node_id = parts[parts.len() - 2]
            .to_string()
            .try_into()
            .map_err(|e| eyre!(format!("{}", e)))?;
        let prop_id = parts[parts.len() - 1]
            .to_string()
            .try_into()
            .map_err(|e| eyre!(format!("{}", e)))?;

        Ok(PropertyRef::new(homie_domain, device_id, node_id, prop_id))
    }
}

impl ToSubjectStr for NodeRef {
    fn to_subject_string(&self) -> String {
        format!("{}/{}/{}", self.homie_domain(), self.device_id(), self.node_id())
    }
}

impl FromSubjectStr for NodeRef {
    type ConverError = eyre::Error;

    fn from_subject_string(s: &str) -> Result<NodeRef> {
        // Split the string into parts
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(eyre!(format!("Invalid string format for NodeRef: '{}'", s)));
        }
        let homie_domain = if parts.len() == 3 {
            HomieDomain::try_from(parts[0].to_string()).map_err(|e| eyre!(format!("{}", e)))?
        } else {
            SETTINGS.homie.homie_domain.clone()
        };
        let device_id = parts[parts.len() - 2]
            .to_string()
            .try_into()
            .map_err(|e| eyre!(format!("{}", e)))?;
        let node_id = parts[parts.len() - 1]
            .to_string()
            .try_into()
            .map_err(|e| eyre!(format!("{}", e)))?;

        Ok(NodeRef::new(homie_domain, device_id, node_id))
    }
}

impl ToSubjectStr for DeviceRef {
    fn to_subject_string(&self) -> String {
        format!("{}/{}", self.homie_domain(), self.device_id())
    }
}

impl FromSubjectStr for DeviceRef {
    type ConverError = eyre::Error;

    fn from_subject_string(s: &str) -> Result<DeviceRef> {
        // Split the string into parts
        let parts: Vec<&str> = s.split('/').collect();
        if parts.is_empty() || parts.len() > 2 {
            return Err(eyre!(format!("Invalid string format for DeviceRef: '{}'", s)));
        }
        let homie_domain = if parts.len() == 2 {
            HomieDomain::try_from(parts[0].to_string()).map_err(|e| eyre!(format!("{}", e)))?
        } else {
            SETTINGS.homie.homie_domain.clone()
        };
        let device_id = parts[parts.len() - 1]
            .to_string()
            .try_into()
            .map_err(|e| eyre!(format!("{}", e)))?;

        Ok(DeviceRef::new(homie_domain, device_id))
    }
}
