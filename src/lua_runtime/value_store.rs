use mlua::{ExternalResult, LuaSerdeExt, ObjectLike, UserData};
use simple_kv_store::{normalize_key, KeyValueStore};

pub struct LuaValueStore {
    pub store: KeyValueStore,
}

impl UserData for LuaValueStore {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("set", |lua, vs, (key, value): (mlua::String, mlua::Value)| async move {
            let n_key = normalize_key(&key.to_str()?);

            let serialized_value = if let mlua::Value::UserData(ud) = value {
                // Try calling the `__tostring` metamethod
                match ud.to_string() {
                    Ok(lua_str) => {
                        serde_json::Value::String(lua_str) // Store as a string
                    }
                    Err(_) => {
                        log::error!("Error in script: unsupported userdata type (no __tostring metamethod)");
                        return Err(mlua::Error::external("Unsupported userdata type (no __tostring metamethod)"));
                    }
                }
            } else {
                // Normal JSON serialization for everything else
                lua.from_value::<serde_json::Value>(value)?
            };

            vs.store.set(&n_key, &serialized_value).await.into_lua_err()
        });
        methods.add_async_method("get", |lua, homie, key: mlua::String| async move {
            let n_key = normalize_key(&key.to_str()?);
            let value: Option<serde_json::Value> = homie.store.get(&n_key).await;
            if let Some(v) = value {
                Ok(lua.to_value(&v)?)
            } else {
                Ok(mlua::Value::Nil)
            }
        });
        methods.add_async_method("delete", |_, homie, key: mlua::String| async move {
            let n_key = normalize_key(&key.to_str()?);
            homie.store.delete(&n_key).await.into_lua_err()?;
            Ok(())
        });
    }
}
