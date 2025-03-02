use std::{collections::HashMap, sync::Arc};

use mlua::{Lua, Result, Value};
use tokio::sync::RwLock;

pub async fn setup_custom_loader(lua: &Lua, modules: Arc<RwLock<HashMap<String, String>>>) -> Result<()> {
    let loader = {
        let modules = Arc::clone(&modules); // Clone into the closure
        lua.create_async_function(move |lua, name: String| {
            let modules = Arc::clone(&modules); // Capture again for async move block
            async move {
                let modules = modules.read().await;
                if let Some(source) = modules.get(&name) {
                    let chunk = lua.load(source).set_name(&name).into_function()?;
                    Ok(Value::Function(chunk))
                } else {
                    Ok(Value::Nil)
                }
            }
        })?
    };

    let package = lua.globals().get::<mlua::Table>("package")?;
    let searchers = package.get::<mlua::Table>("searchers")?;
    let len = searchers.raw_len();
    searchers.raw_set(len + 1, loader)?;

    Ok(())
}
