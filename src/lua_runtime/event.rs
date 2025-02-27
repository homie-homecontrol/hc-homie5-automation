use crate::{lua_runtime::LuaPropertyRef, rules::RuleTriggerEvent};
use mlua::{IntoLua, UserData};

use super::LuaHomieValue;

pub struct LuaEvent {
    pub event: RuleTriggerEvent<'static>,
}

impl UserData for LuaEvent {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("type", |_, this| Ok(this.event.trigger_type().to_string()));
        fields.add_field_method_get("prop", |_, this| {
            let res = this.event.property_ref().map(|value| LuaPropertyRef(value.clone()));
            Ok(res)
        });
        fields.add_field_method_get("on_set_value", |lua, this| {
            let res = if let Some(value) = this.event.on_set_value().map(|v| v.to_string()) {
                value.into_lua(lua)?
            } else {
                mlua::Value::Nil
            };
            Ok(res)
        });
        fields.add_field_method_get("value", |lua, this| {
            let res = if let Some(value) = this.event.value().map(|v| LuaHomieValue(v.clone())) {
                value.into_lua(lua)?
            } else {
                mlua::Value::Nil
            };
            Ok(res)
        });
        fields.add_field_method_get("from_value", |lua, this| {
            let res = if let Some(value) = this.event.from().map(|v| LuaHomieValue(v.clone())) {
                value.into_lua(lua)?
            } else {
                mlua::Value::Nil
            };
            Ok(res)
        });
        fields.add_field_method_get("timer_id", |lua, this| {
            let res = if let Some(value) = this.event.timer_id().map(|v| v.to_string()) {
                value.into_lua(lua)?
            } else {
                mlua::Value::Nil
            };
            Ok(res)
        });
    }
}
