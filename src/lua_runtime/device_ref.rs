use crate::rules::{FromSubjectStr, ToSubjectStr};
use homie5::DeviceRef;
use mlua::{FromLua, Lua};
use mlua::{UserData, Value};

#[derive(Clone)]
pub struct LuaDeviceRef(pub(crate) DeviceRef);

impl UserData for LuaDeviceRef {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        // Define the __tostring metamethod
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            // Convert the LuaDeviceRef to a string representation
            Ok(this.0.to_subject_string())
        });
    }
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("homie_domain", |_, this| Ok(this.0.homie_domain().to_string()));
        fields.add_field_method_get("device_id", |_, this| Ok(this.0.device_id().to_string()));
        fields.add_field_method_get("subject", |_, this| Ok(this.0.to_subject_string()));
    }
}

impl FromLua for LuaDeviceRef {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        if let Value::UserData(ud) = value {
            // Borrow instead of take, keeping Lua's ownership intact
            let prop_ref = ud.borrow::<LuaDeviceRef>()?;
            Ok(prop_ref.clone()) // Clone the borrowed reference to return a new instance
        } else {
            Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "LuaDeviceRef".to_string(),
                message: Some("expected UserData type".to_string()),
            })
        }
    }
}

impl TryFrom<mlua::Value> for LuaDeviceRef {
    type Error = mlua::Error;

    fn try_from(value: Value) -> mlua::Result<Self> {
        match value {
            Value::String(s) => {
                // Parse the string into a DeviceRef
                let subject_str = &s.to_str()?;
                let parsed_ref: DeviceRef = DeviceRef::from_subject_string(subject_str)
                    .map_err(|e| mlua::Error::external(format!("Failed to parse subject: {e}")))?;
                Ok(LuaDeviceRef(parsed_ref))
            }
            Value::UserData(ud) => {
                // Attempt to borrow as LuaDeviceRef
                let prop_ref = ud.borrow::<LuaDeviceRef>()?;
                Ok(prop_ref.clone())
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "LuaDeviceRef".to_string(),
                message: Some("Expected a string or LuaDeviceRef".to_string()),
            }),
        }
    }
}
