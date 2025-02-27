use std::str::FromStr;

use homie5::{HomieColorValue, HomieValue};
use mlua::{ExternalResult, IntoLua, LuaSerdeExt, Value};
use mlua::{FromLua, Lua};

#[derive(Debug, Clone)]
pub struct LuaHomieValue(pub HomieValue);

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
