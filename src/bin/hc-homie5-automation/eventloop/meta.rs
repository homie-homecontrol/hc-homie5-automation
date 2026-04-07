use color_eyre::eyre::Result;
use config_watcher::config_item_watcher::ConfigItemEvent;
use hc_homie5_automation::{app_state::AppState, meta::MetaConfig};

pub async fn handle_meta_changes_event(event: ConfigItemEvent<MetaConfig>, state: &mut AppState) -> Result<bool> {
    state.meta.handle_event(event).await?;
    Ok(false)
}
