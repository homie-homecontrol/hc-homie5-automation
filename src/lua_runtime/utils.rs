use mlua::{ExternalResult, LuaSerdeExt, UserData};
use rumqttc::QoS;
use std::time::Duration;

use crate::mqtt_client::ManagedMqttClient;

pub struct LuaHttpBody(String);

impl UserData for LuaHttpBody {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("text", |_, body, ()| Ok(body.0.clone()));
        methods.add_method("json", |lua, body, ()| {
            let json_value: serde_json::Value = serde_json::from_str(&body.0).into_lua_err()?;
            lua.to_value(&json_value)
        });
    }
}

pub struct LuaUtils {
    pub mqtt_client: ManagedMqttClient,
}

impl UserData for LuaUtils {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("sleep", |_, _, time: u64| async move {
            tokio::time::sleep(Duration::from_millis(time)).await;
            Ok(())
        });
        methods.add_async_method("from_json", |lua, _, data: String| async move {
            let json_value: serde_json::Value = serde_json::from_str(&data).into_lua_err()?;
            lua.to_value(&json_value)
        });
        methods.add_async_method("to_json", |lua, _, data: mlua::Table| async move {
            let s = serde_json::to_string(&data).into_lua_err()?;
            lua.to_value(&s)
        });
        methods.add_async_method("http_get", |_, _, uri: String| async move {
            let text = reqwest::get(&uri).await.into_lua_err()?.text().await.into_lua_err()?;
            Ok(LuaHttpBody(text))
        });
        methods.add_async_method("http_post", |_, _, (uri, data): (String, String)| async move {
            // Create a reqwest client
            let client = reqwest::Client::new();
            // Make the POST request
            let response = client.post(&uri).body(data).send().await.into_lua_err()?;

            let text = response.text().await.into_lua_err()?;
            Ok(LuaHttpBody(text))
        });
        methods.add_async_method("http_post_json", |_, _, (uri, data): (String, mlua::Table)| async move {
            // Create a reqwest client
            let client = reqwest::Client::new();
            // Make the POST request
            let response = client.post(&uri).json(&data).send().await.into_lua_err()?;

            let text = response.text().await.into_lua_err()?;
            Ok(LuaHttpBody(text))
        });
        methods.add_async_method("http_post_form", |_, _, (uri, data): (String, mlua::Table)| async move {
            // Create a reqwest client
            let client = reqwest::Client::new();
            // Make the POST request
            let response = client.post(&uri).form(&data).send().await.into_lua_err()?;

            let text = response.text().await.into_lua_err()?;
            Ok(LuaHttpBody(text))
        });
        methods.add_async_method(
            "mqtt_publish",
            |_, this, (topic, payload, qos, retained): (String, String, Option<i64>, Option<bool>)| async move {
                let qos = match qos.unwrap_or(0) {
                    0 => QoS::AtMostOnce,
                    1 => QoS::AtLeastOnce,
                    2 => QoS::ExactlyOnce,
                    _ => QoS::AtMostOnce,
                };
                let retained = retained.unwrap_or(false);
                this.mqtt_client
                    .publish(topic, qos, retained, payload)
                    .await
                    .into_lua_err()?;
                Ok(())
            },
        );
    }
}
