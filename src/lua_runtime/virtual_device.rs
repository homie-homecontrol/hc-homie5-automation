use super::{LuaDeviceRef, LuaHomieValue, LuaPropertyRef};
use crate::virtual_devices::VirtualDeviceManagerProxy;
use hc_homie5::device::HomieDeviceCore;
use hc_homie5::logging::{truncate, PAYLOAD_MAX};
use homie5::HomieID;
use mlua::{ExternalResult, LuaSerdeExt, UserData};

pub struct LuaVirtualDevice {
    pub vdm: VirtualDeviceManagerProxy,
    /// Name of the rule running the script, for logs.
    pub rule: String,
}

impl LuaVirtualDevice {
    /// Converts a Lua value for `prop`, parsing strings against its description.
    async fn convert_value(
        &self,
        prop: &LuaPropertyRef,
        value: mlua::Value,
        lua: &mlua::Lua,
    ) -> mlua::Result<LuaHomieValue> {
        let desc = self
            .vdm
            .read()
            .await
            .get(prop.0.device_ref())
            .and_then(|device| device.description().get_property(prop.0.prop_pointer()).cloned());
        LuaHomieValue::from_lua_for(value, lua, desc.as_ref())
    }
}

impl UserData for LuaVirtualDevice {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method(
            "set_str_value",
            |_, homie, (subject, value): (mlua::Value, mlua::String)| async move {
                // Convert the subject (string or LuaPropertyRef) into a LuaPropertyRef
                let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
                let value = value.to_string_lossy();

                homie.vdm.set_str_value(&prop.0, value.as_str()).await.into_lua_err()?;
                tracing::debug!(
                    rule = %homie.rule,
                    vdevice = %prop.0.device_id(),
                    node = %prop.0.node_id(),
                    property = %prop.0.prop_id(),
                    value = truncate(&value, PAYLOAD_MAX),
                    source = "lua",
                    "virtual device value set"
                );
                Ok(())
            },
        );
        methods.add_async_method("set_value", |lua, homie, (subject, value): (mlua::Value, mlua::Value)| async move {
            // Convert the subject (string or LuaPropertyRef) into a LuaPropertyRef
            let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
            let value = homie.convert_value(&prop, value, &lua).await?;
            let logged_value = truncate(&value.0.to_string(), PAYLOAD_MAX);
            homie.vdm.set_value(&prop.0, value.0).await.into_lua_err()?;
            tracing::debug!(
                rule = %homie.rule,
                vdevice = %prop.0.device_id(),
                node = %prop.0.node_id(),
                property = %prop.0.prop_id(),
                value = logged_value,
                source = "lua",
                "virtual device value set"
            );
            Ok(())
        });
        methods.add_async_method(
            "set_command",
            |lua, homie, (subject, value): (mlua::Value, mlua::Value)| async move {
                // Convert the subject (string or LuaPropertyRef) into a LuaPropertyRef
                let prop = LuaPropertyRef::try_from(subject).into_lua_err()?;
                let value = homie.convert_value(&prop, value, &lua).await?;
                let logged_value = truncate(&value.0.to_string(), PAYLOAD_MAX);
                homie.vdm.set_command(&prop.0, value.0).await.into_lua_err()?;
                tracing::info!(
                    rule = %homie.rule,
                    device = %prop.0.device_id(),
                    node = %prop.0.node_id(),
                    property = %prop.0.prop_id(),
                    value = logged_value,
                    source = "lua",
                    "command sent"
                );
                Ok(())
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
                    let alert_id = HomieID::try_from(alert_id.to_string_lossy()).into_lua_err()?;
                    let alert = alert.to_string_lossy();
                    let changed = device.alerts.get(&alert_id) != Some(&alert);
                    device.set_alert(alert_id.clone(), alert.clone()).await.into_lua_err()?;
                    if changed {
                        tracing::info!(
                            rule = %homie.rule,
                            device = %dref.device_id(),
                            alert = %alert_id,
                            alert_message = alert,
                            source = "lua",
                            "alert raised"
                        );
                    }
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
                    let alert_id = HomieID::try_from(alert_id.to_string_lossy()).into_lua_err()?;
                    let active = device.alerts.contains_key(&alert_id);
                    device.clear_alert(&alert_id).await.into_lua_err()?;
                    if active {
                        tracing::info!(
                            rule = %homie.rule,
                            device = %dref.device_id(),
                            alert = %alert_id,
                            source = "lua",
                            "alert cleared"
                        );
                    }
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
