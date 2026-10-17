use color_eyre::eyre::Result;
use config_watcher::config_item_watcher::ConfigItemEvent;
use hc_homie5::logging::chain;
use hc_homie5_automation::{app_state::AppState, rules::Rule};

pub async fn handle_rules_changes_event(event: ConfigItemEvent<Rule>, state: &mut AppState) -> Result<bool> {
    match event {
        ConfigItemEvent::New(hash, rule) => {
            let name = rule.name.clone();
            let result = state
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
                .await
                .map(|_| ());
            let file = state.rules.get_filename(hash).map(String::as_str).unwrap_or("-");
            match result {
                Ok(()) => tracing::info!(rule = %name, file = %file, "rule loaded"),
                Err(err) => {
                    tracing::error!(rule = %name, file = %file, error = chain(err.as_ref()), "cannot load rule")
                }
            }
        }
        ConfigItemEvent::Removed(hash) => {
            let rule = state
                .rules
                .remove_rule(hash, &state.cron, &state.mqtt_client, &state.solar_events, &state.timers)
                .await?;
            if let Some(rule) = rule {
                tracing::info!(
                    rule = %rule.name,
                    file = %state.rules.get_filename(hash).map(String::as_str).unwrap_or("-"),
                    "rule removed"
                );
            }
        }
        ConfigItemEvent::NewDocument(filename_hash, path) => {
            state.rules.add_rule_file(filename_hash, path);
        }
        ConfigItemEvent::RemoveDocument(filename_hash) => {
            state.rules.remove_rule_file(filename_hash);
        }
        ConfigItemEvent::Invalid { name, error, .. } => {
            tracing::warn!(file = %name, error, "invalid config item skipped");
        }
    }
    Ok(false)
}
