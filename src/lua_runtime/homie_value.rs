use std::str::FromStr;

use homie5::{device_description::HomiePropertyDescription, HomieColorValue, HomieValue};
use mlua::{ExternalResult, IntoLua, LuaSerdeExt, Value};
use mlua::{FromLua, Lua};

use super::check_nesting;

#[derive(Debug, Clone)]
pub struct LuaHomieValue(pub HomieValue);

impl LuaHomieValue {
    /// Converts a Lua value sent to a property. With the property description known, strings are
    /// parsed against it, so they reach string and enum properties unchanged; without one the
    /// generic conversion (which guesses the type of strings) applies.
    pub fn from_lua_for(value: Value, lua: &Lua, desc: Option<&HomiePropertyDescription>) -> mlua::Result<Self> {
        match (&value, desc) {
            (Value::String(s), Some(desc)) => HomieValue::parse(&s.to_str()?, desc).map(LuaHomieValue).into_lua_err(),
            _ => Self::from_lua(value, lua),
        }
    }
}

impl FromLua for LuaHomieValue {
    fn from_lua(lua_value: Value, lua: &Lua) -> mlua::Result<Self> {
        match lua_value {
            Value::Nil => Ok(LuaHomieValue(HomieValue::Empty)),
            Value::String(ref s) => {
                let s = s.to_str()?.to_string();

                // Try parsing as Color first, then fallback to normal String
                if let Ok(cv) = HomieColorValue::from_str(&s) {
                    return Ok(LuaHomieValue(HomieValue::Color(cv)));
                }

                // Try parsing as DateTime
                if let Ok(dt) = HomieValue::flexible_datetime_parser(&s) {
                    return Ok(LuaHomieValue(HomieValue::DateTime(dt.with_timezone(&chrono::Utc))));
                }

                // Try parsing as Duration (ISO 8601)
                if let Ok(dur) = HomieValue::parse_duration(&s) {
                    return Ok(LuaHomieValue(HomieValue::Duration(dur)));
                }

                Ok(LuaHomieValue(HomieValue::String(s)))
            }
            Value::Integer(i) => Ok(LuaHomieValue(HomieValue::Integer(i))),
            Value::Number(n) => Ok(LuaHomieValue(HomieValue::Float(n))),
            Value::Boolean(b) => Ok(LuaHomieValue(HomieValue::Bool(b))),
            Value::Table(ref table) => {
                // Check if it's a JSON-like table and convert it to JSON
                check_nesting(&lua_value)?;
                let json: serde_json::Value = lua.from_value(Value::Table(table.clone())).into_lua_err()?;
                Ok(LuaHomieValue(HomieValue::JSON(json)))
            }
            _ => Err(mlua::Error::FromLuaConversionError {
                from: lua_value.type_name(),
                to: "HomieValue".to_string(),
                message: Some("Unsupported Lua type for HomieValue".into()),
            }),
        }
    }
}

impl IntoLua for LuaHomieValue {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        match self.0 {
            HomieValue::Empty => Ok(Value::Nil),
            HomieValue::String(s) => Ok(Value::String(lua.create_string(&s)?)),
            HomieValue::Integer(i) => Ok(Value::Integer(i)),
            HomieValue::Float(f) => Ok(Value::Number(f)),
            HomieValue::Bool(b) => Ok(Value::Boolean(b)),
            HomieValue::Enum(s) => Ok(Value::String(lua.create_string(&s)?)),
            HomieValue::Color(c) => Ok(Value::String(lua.create_string(c.to_string())?)),
            HomieValue::DateTime(dt) => Ok(Value::String(lua.create_string(dt.to_rfc3339())?)),
            HomieValue::Duration(dur) => {
                let iso_duration = format!("PT{}S", dur.num_seconds());
                Ok(Value::String(lua.create_string(&iso_duration)?))
            }
            HomieValue::JSON(json) => lua.to_value(&serde_json::to_string(&json).into_lua_err()?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use homie5::device_description::PropertyDescriptionBuilder;

    fn convert(raw: &str, desc: Option<&HomiePropertyDescription>) -> mlua::Result<HomieValue> {
        let lua = Lua::new();
        let value = Value::String(lua.create_string(raw).unwrap());
        LuaHomieValue::from_lua_for(value, &lua, desc).map(|v| v.0)
    }

    #[test]
    fn string_target_keeps_timestamp_like_string() {
        let desc = PropertyDescriptionBuilder::string().build();
        let value = convert("2024-01-01T10:00", Some(&desc)).unwrap();
        assert_eq!(value, HomieValue::String("2024-01-01T10:00".to_string()));
        assert_eq!(value.to_string(), "2024-01-01T10:00");
    }

    #[test]
    fn enum_target_keeps_duration_like_string() {
        let desc = PropertyDescriptionBuilder::enumeration(["PT5S", "off"])
            .unwrap()
            .build();
        assert_eq!(convert("PT5S", Some(&desc)).unwrap(), HomieValue::Enum("PT5S".to_string()));
    }

    #[test]
    fn string_is_parsed_against_numeric_target() {
        let desc = PropertyDescriptionBuilder::integer().build();
        assert_eq!(convert("42", Some(&desc)).unwrap(), HomieValue::Integer(42));
        assert!(convert("forty-two", Some(&desc)).is_err());
    }

    #[test]
    fn without_description_strings_are_still_sniffed() {
        assert!(matches!(convert("PT5S", None).unwrap(), HomieValue::Duration(_)));
    }
}
