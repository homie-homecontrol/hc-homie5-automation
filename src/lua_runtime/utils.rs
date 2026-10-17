use hc_homie5::logging::{truncate, PAYLOAD_MAX};
use mlua::{ExternalResult, LuaSerdeExt, UserData};
use rumqttc::QoS;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use super::check_nesting;
use crate::mqtt_client::ManagedMqttClient;

/// Shared HTTP client for all Lua `http_*` utility functions.
///
/// `reqwest::Client` holds an internal connection pool; creating a new client
/// per request (as was done previously) defeats pooling and re-does TLS
/// setup on every call. A single lazily-initialized client is cheap to clone
/// internally (Arc) and safe to use from concurrent script executions.
///
/// Building the client can fail, for example without system CA certificates;
/// `reqwest::Client::new` would panic then, so the error is kept and returned
/// to the script instead.
static HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> =
    LazyLock::new(|| reqwest::Client::builder().build().map_err(|err| err.to_string()));

fn http_client() -> mlua::Result<&'static reqwest::Client> {
    HTTP_CLIENT
        .as_ref()
        .map_err(|err| mlua::Error::RuntimeError(format!("HTTP client unavailable: {err}")))
}

/// Sends an HTTP request of a script and logs its outcome. Only the URL's origin is logged: paths
/// and queries can carry tokens.
async fn send_http(rule: &str, method: &str, uri: &str, request: reqwest::RequestBuilder) -> mlua::Result<LuaHttpBody> {
    let started = Instant::now();
    let response = request.send().await.into_lua_err()?;
    let status = response.status();
    let duration_ms = started.elapsed().as_millis() as u64;
    let host = reqwest::Url::parse(uri)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_default();
    if status.is_success() {
        tracing::info!(rule = %rule, method, host = %host, status = status.as_u16(), duration_ms, "http request sent");
    } else {
        tracing::warn!(rule = %rule, method, host = %host, status = status.as_u16(), duration_ms, "http request failed");
    }
    let text = response.text().await.into_lua_err()?;
    Ok(LuaHttpBody(text))
}

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
    /// Name of the rule running the script, for logs.
    pub rule: String,
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
            check_nesting(&mlua::Value::Table(data.clone()))?;
            let s = serde_json::to_string(&data).into_lua_err()?;
            lua.to_value(&s)
        });
        methods.add_async_method("http_get", |_, this, uri: String| async move {
            send_http(&this.rule, "GET", &uri, http_client()?.get(&uri)).await
        });
        methods.add_async_method("http_post", |_, this, (uri, data): (String, String)| async move {
            send_http(&this.rule, "POST", &uri, http_client()?.post(&uri).body(data)).await
        });
        methods.add_async_method("http_post_json", |_, this, (uri, data): (String, mlua::Table)| async move {
            check_nesting(&mlua::Value::Table(data.clone()))?;
            send_http(&this.rule, "POST", &uri, http_client()?.post(&uri).json(&data)).await
        });
        methods.add_async_method("http_post_form", |_, this, (uri, data): (String, mlua::Table)| async move {
            check_nesting(&mlua::Value::Table(data.clone()))?;
            send_http(&this.rule, "POST", &uri, http_client()?.post(&uri).form(&data)).await
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
                let logged_payload = truncate(&payload, PAYLOAD_MAX);
                this.mqtt_client
                    .publish(topic.as_str(), qos, retained, payload)
                    .await
                    .into_lua_err()?;
                tracing::info!(
                    rule = %this.rule,
                    topic = %topic,
                    payload = logged_payload,
                    retain = retained,
                    source = "lua",
                    "message published"
                );
                Ok(())
            },
        );
    }
}
