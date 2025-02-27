use super::{LuaDeviceRef, LuaHomieValue, LuaPropertyRef};
use crate::virtual_devices::VirtualDeviceManagerProxy;
use hc_homie5::HomieDeviceCore;
use homie5::HomieID;
use mlua::{ExternalResult, LuaSerdeExt, UserData};

pub struct LuaVirtualDecvice {
    pub vdm: VirtualDeviceManagerProxy,
}

impl UserData for LuaVirtualDecvice {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method(
            "set_str_value",
            |_, homie, (subject, value): (mlua::Value, mlua::String)| async move {
                // Convert the subject (string or LuaPropertyRef) into a LuaPropertyRef
                let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;

                homie
                    .vdm
                    .set_str_value(&prop.0, value.to_string_lossy().as_str())
                    .await
                    .into_lua_err()
            },
        );
        methods.add_async_method("set_value", |_, homie, (subject, value): (mlua::Value, LuaHomieValue)| async move {
            // Convert the subject (string or LuaPropertyRef) into a LuaPropertyRef
            let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
            homie.vdm.set_value(&prop.0, value.0).await.into_lua_err()
        });
        methods.add_async_method(
            "set_command",
            |_, homie, (subject, value): (mlua::Value, LuaHomieValue)| async move {
                // Convert the subject (string or LuaPropertyRef) into a LuaPropertyRef
                let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
                homie.vdm.set_command(&prop.0, value.0).await.into_lua_err()
            },
        );
        methods.add_async_method("get_value", |_, homie, subject: mlua::Value| async move {
            let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
            let devices = homie.vdm.read().await;
            let value = devices
                .get(prop.0.device_ref())
                .and_then(|device| device.properties.get(prop.0.prop_pointer()))
                .and_then(|entry| entry.value.clone())
                .map(LuaHomieValue);

            Ok(value)
        });
        methods.add_async_method("get_property_description", |lua, homie, subject: mlua::Value| async move {
            let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
            let devices = homie.vdm.read().await;
            let value = devices
                .get(prop.0.device_ref())
                .and_then(|device| device.description().get_property(prop.0.prop_pointer()).cloned())
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
            let devices = homie.vdm.read().await;
            let value = devices
                .get(&dref)
                .map(|device| device.description())
                .map(|desc| {
                    let ser: serde_json::Value = serde_json::to_value(desc).into_lua_err()?;
                    let val: mlua::Value = lua.to_value(&ser)?;
                    Ok::<_, mlua::Error>(val)
                })
                .transpose()?;

            Ok(value)
        });
        methods.add_async_method(
            "set_device_alert",
            |_, homie, (dop_ref, alert_id, alert): (mlua::Value, mlua::String, mlua::String)| async move {
                let device_ref = LuaDeviceRef::try_from(dop_ref.clone()).into_lua_err();
                let prop_ref = LuaPropertyRef::try_from(dop_ref).into_lua_err();

                let dref = if let Ok(device_ref) = device_ref {
                    device_ref.0.clone()
                } else if let Ok(prop_ref) = prop_ref {
                    prop_ref.0.device_ref().to_owned()
                } else {
                    return Ok(false);
                };
                let mut devices = homie.vdm.write().await;
                if let Some(device) = devices.get_mut(&dref) {
                    device
                        .set_alert(
                            HomieID::try_from(alert_id.to_string_lossy()).into_lua_err()?,
                            alert.to_string_lossy(),
                        )
                        .await
                        .into_lua_err()?;
                }
                Ok(true)
            },
        );
        methods.add_async_method(
            "clear_device_alert",
            |_, homie, (dop_ref, alert_id): (mlua::Value, mlua::String)| async move {
                let device_ref = LuaDeviceRef::try_from(dop_ref.clone()).into_lua_err();
                let prop_ref = LuaPropertyRef::try_from(dop_ref).into_lua_err();

                let dref = if let Ok(device_ref) = device_ref {
                    device_ref.0.clone()
                } else if let Ok(prop_ref) = prop_ref {
                    prop_ref.0.device_ref().to_owned()
                } else {
                    return Ok(false);
                };
                let mut devices = homie.vdm.write().await;
                if let Some(device) = devices.get_mut(&dref) {
                    device
                        .clear_alert(&HomieID::try_from(alert_id.to_string_lossy()).into_lua_err()?)
                        .await
                        .into_lua_err()?;
                }
                Ok(true)
            },
        );
        methods.add_async_method("get_device_alerts", |lua, homie, subject: mlua::Value| async move {
            let device = LuaDeviceRef::try_from(subject).into_lua_err()?;
            let devices = homie.vdm.read().await;
            let value = devices
                .get(&device.0)
                .map(|device| {
                    let ser: serde_json::Value = serde_json::to_value(device.alerts.clone()).into_lua_err()?;
                    let val: mlua::Value = lua.to_value(&ser)?;
                    Ok::<_, mlua::Error>(val)
                })
                .transpose()?;

            Ok(value)
        });
    }
}
