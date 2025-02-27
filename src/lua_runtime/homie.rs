use super::{LuaDeviceRef, LuaHomieValue, LuaPropertyRef};
use crate::device_manager::DeviceManager;
use mlua::{ExternalResult, LuaSerdeExt, UserData};

pub struct LuaHomie {
    pub dm: DeviceManager,
}

impl UserData for LuaHomie {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method(
            "set_command",
            |_, homie, (subject, value): (mlua::Value, LuaHomieValue)| async move {
                // Convert the subject (string or LuaPropertyRef) into a LuaPropertyRef
                let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
                homie.dm.set_command(&prop.0, &value.0).await.into_lua_err()
            },
        );
        methods.add_async_method("get_value", |_, homie, subject: mlua::Value| async move {
            let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
            let devices = homie.dm.read().await;
            let value = devices
                .get_device(prop.0.device_ref())
                .and_then(|device| device.prop_values.get_value_entry(prop.0.prop_pointer()))
                .and_then(|entry| entry.value.clone())
                .map(LuaHomieValue);

            Ok(value)
        });
        methods.add_async_method("get_property_description", |lua, homie, subject: mlua::Value| async move {
            let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
            let devices = homie.dm.read().await;
            let value = devices
                .get_device(prop.0.device_ref())
                .and_then(|device| {
                    device
                        .description
                        .as_ref()
                        .and_then(|desc| desc.get_property(prop.0.prop_pointer()).cloned())
                })
                .map(|pdesc| {
                    let ser: serde_json::Value = serde_json::to_value(pdesc).into_lua_err()?;
                    let val: mlua::Value = lua.to_value(&ser)?;
                    Ok::<_, mlua::Error>(val)
                })
                .transpose()?;

            Ok(value)
        });
        methods.add_async_method("get_device_description", |lua, homie, dop_ref: mlua::Value| async move {
            let device_ref = LuaDeviceRef::try_from(dop_ref.clone()).into_lua_err();
            let prop_ref = LuaPropertyRef::try_from(dop_ref).into_lua_err();

            let dref = if let Ok(device_ref) = device_ref {
                device_ref.0.clone()
            } else if let Ok(prop_ref) = prop_ref {
                prop_ref.0.device_ref().to_owned()
            } else {
                return Ok(None);
            };
            let devices = homie.dm.read().await;
            let value = devices
                .get_device(&dref)
                .and_then(|device| device.description.as_ref())
                .map(|desc| {
                    let ser: serde_json::Value = serde_json::to_value(desc).into_lua_err()?;
                    let val: mlua::Value = lua.to_value(&ser)?;
                    Ok::<_, mlua::Error>(val)
                })
                .transpose()?;

            Ok(value)
        });
        methods.add_async_method("get_device_alerts", |lua, homie, subject: mlua::Value| async move {
            let device = LuaDeviceRef::try_from(subject).into_lua_err()?;
            let devices = homie.dm.read().await;
            let value = devices
                .get_device(&device.0)
                .map(|device| {
                    let ser: serde_json::Value = serde_json::to_value(device.alerts.as_map().clone()).into_lua_err()?;
                    let val: mlua::Value = lua.to_value(&ser)?;
                    Ok::<_, mlua::Error>(val)
                })
                .transpose()?;

            Ok(value)
        });
    }
}
