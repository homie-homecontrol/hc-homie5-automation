use color_eyre::eyre::Result;
use config_watcher::config_item_watcher::ConfigItemEvent;
use hc_homie5_automation::app_state::AppState;

pub async fn handle_lua_files_changes_event(event: ConfigItemEvent<String>, state: &mut AppState) -> Result<bool> {
    state.lua_module_manager.handle_event(event).await;
    Ok(false)
}
