use color_eyre::eyre::Result;
use config_watcher::config_item_watcher::ConfigItemEvent;
use hc_homie5_automation::{app_state::AppState, rules::Rule};

pub async fn handle_rules_changes_event(event: ConfigItemEvent<Rule>, state: &mut AppState) -> Result<bool> {
    match event {
        ConfigItemEvent::New(hash, rule) => {
            log::trace!(
                "New Rule detected: {} ({})",
                rule.name,
                state.rules.get_filename(hash).unwrap_or(&"-".to_string()),
            );
            state
                .rules
                .add_rule(
                    hash,
                    rule,
                    &state.cron,
                    &state.mqtt_client,
                    &state.solar_events,
                    &state.dm,
                    &*state.vdm.read().await,
                )
                .await?;
        }
        ConfigItemEvent::Removed(hash) => {
            let rule = state
                .rules
                .remove_rule(hash, &state.cron, &state.mqtt_client, &state.solar_events, &state.timers)
                .await?;
            if let Some(rule) = rule {
                log::debug!(
                    "Rule removed: {} ({})",
                    rule.name,
                    state.rules.get_filename(hash).unwrap_or(&"-".to_string())
                );
            }
        }
        ConfigItemEvent::NewDocument(filename_hash, path) => {
            state.rules.add_rule_file(filename_hash, path);
        }
        ConfigItemEvent::RemoveDocument(filename_hash) => {
            state.rules.remove_rule_file(filename_hash);
        }
    }
    Ok(false)
}
