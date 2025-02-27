use super::LuaDeviceRef;
use crate::rules::{FromSubjectStr, Subject, ToSubjectStr};
use homie5::{PropertyRef, ToTopic};
use mlua::{FromLua, Lua};
use mlua::{UserData, Value};
use serde::{Deserialize, Deserializer};
use serde::{Serialize, Serializer};

#[derive(Clone)]
pub struct LuaPropertyRef(pub(crate) PropertyRef);

impl Serialize for LuaPropertyRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize the inner `PropertyRef` directly
        serializer.serialize_str(&self.0.to_topic().build())
    }
}

impl<'de> Deserialize<'de> for LuaPropertyRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Subject::deserialize(deserializer)?; // Deserialize into a string
        Ok(LuaPropertyRef(s.into()))
    }
}

impl UserData for LuaPropertyRef {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        // Define the __tostring metamethod
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            // Convert the LuaPropertyRef to a string representation
            Ok(this.0.to_subject_string())
        });
    }
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("homie_domain", |_, this| Ok(this.0.homie_domain().to_string()));
        fields.add_field_method_get("device_id", |_, this| Ok(this.0.device_id().to_string()));
        fields.add_field_method_get("node_id", |_, this| Ok(this.0.node_id().to_string()));
        fields.add_field_method_get("prop_id", |_, this| Ok(this.0.prop_id().to_string()));
        fields.add_field_method_get("subject", |_, this| Ok(this.0.to_subject_string()));
        fields.add_field_method_get("device_ref", |_, this| {
            let d = LuaDeviceRef(this.0.device_ref().to_owned());
            Ok(d)
        });
    }
}

impl FromLua for LuaPropertyRef {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        if let Value::UserData(ud) = value {
            // Borrow instead of take, keeping Lua's ownership intact
            let prop_ref = ud.borrow::<LuaPropertyRef>()?;
            Ok(prop_ref.clone()) // Clone the borrowed reference to return a new instance
        } else {
            Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "LuaPropertyRef".to_string(),
                message: Some("expected UserData type".to_string()),
            })
        }
    }
}

impl TryFrom<mlua::Value> for LuaPropertyRef {
    type Error = mlua::Error;

    fn try_from(value: Value) -> mlua::Result<Self> {
        match value {
            Value::String(s) => {
                // Parse the string into a PropertyRef
                let subject_str = &s.to_str()?;
                let parsed_ref: PropertyRef = PropertyRef::from_subject_string(subject_str)
                    .map_err(|e| mlua::Error::external(format!("Failed to parse subject: {e}")))?;
                Ok(LuaPropertyRef(parsed_ref))
            }
            Value::UserData(ud) => {
                // Attempt to borrow as LuaPropertyRef
                let prop_ref = ud.borrow::<LuaPropertyRef>()?;
                Ok(prop_ref.clone())
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "LuaPropertyRef".to_string(),
                message: Some("Expected a string or LuaPropertyRef".to_string()),
            }),
        }
    }
}
