use super::LuaDeviceRef;
use homie5::PropertyRef;
use mlua::{FromLua, Lua};
use mlua::{UserData, Value};
use serde::{Deserialize, Deserializer};
use serde::{Serialize, Serializer};
use std::str::FromStr;

#[derive(Clone)]
pub struct LuaPropertyRef(pub(crate) PropertyRef);

impl Serialize for LuaPropertyRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LuaPropertyRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let prop_ref = PropertyRef::deserialize(deserializer)?;
        Ok(LuaPropertyRef(prop_ref))
    }
}

impl UserData for LuaPropertyRef {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(this.0.to_string())
        });
    }
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("homie_domain", |_, this| Ok(this.0.homie_domain().to_string()));
        fields.add_field_method_get("device_id", |_, this| Ok(this.0.device_id().to_string()));
        fields.add_field_method_get("node_id", |_, this| Ok(this.0.node_id().to_string()));
        fields.add_field_method_get("prop_id", |_, this| Ok(this.0.prop_id().to_string()));
        fields.add_field_method_get("subject", |_, this| Ok(this.0.to_string()));
        fields.add_field_method_get("device_ref", |_, this| {
            let d = LuaDeviceRef(this.0.device_ref().to_owned());
            Ok(d)
        });
    }
}

impl FromLua for LuaPropertyRef {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        if let Value::UserData(ud) = value {
            let prop_ref = ud.borrow::<LuaPropertyRef>()?;
            Ok(prop_ref.clone())
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
                let subject_str = &s.to_str()?;
                let parsed_ref = PropertyRef::from_str(subject_str)
                    .map_err(|e| mlua::Error::external(format!("Failed to parse PropertyRef: {e}")))?;
                Ok(LuaPropertyRef(parsed_ref))
            }
            Value::UserData(ud) => {
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
