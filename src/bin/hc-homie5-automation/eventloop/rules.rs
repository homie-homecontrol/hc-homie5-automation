use color_eyre::eyre::Result;
use config_watcher::config_item_watcher::ConfigItemEvent;
use hc_homie5::logging::chain;
use hc_homie5_automation::{
    app_state::AppState,
    config_item::ItemHash,
    rules::Rule,
    templates::{is_template_file, ConfigDocument, TEMPLATE_FILE_SUFFIX},
};

pub async fn handle_rules_changes_event(
    event: ConfigItemEvent<ConfigDocument<Rule>>,
    state: &mut AppState,
) -> Result<bool> {
    match event {
        ConfigItemEvent::New(hash, document) => {
            let hash = ItemHash::from(hash);
            let file = state
                .rules
                .get_filename(hash)
                .cloned()
                .unwrap_or_else(|| "-".to_owned());
            match document {
                ConfigDocument::Template(tree) if is_template_file(&file) => {
                    match state.rule_templates.add_template(hash, &file, tree) {
                        Ok(added) => {
                            if let Some(other) = &added.shadows {
                                tracing::warn!(template = %added.name, file = %file, other = %other, "template defined twice; this definition wins");
                            }
                            tracing::info!(template = %added.name, file = %file, "rule template loaded");
                            if added.changed {
                                state.schedule_template_expansion().await;
                            }
                        }
                        Err(err) => tracing::error!(file = %file, error = %err, "invalid rule template skipped"),
                    }
                }
                ConfigDocument::Template(_) => {
                    tracing::warn!(file = %file, "template documents belong in *{TEMPLATE_FILE_SUFFIX} files; skipped")
                }
                _ if is_template_file(&file) => {
                    tracing::warn!(file = %file, "only template documents are allowed in template files; skipped")
                }
                ConfigDocument::Dynamic(tree) => {
                    state.rule_templates.add_source(hash, &file, tree);
                    state.schedule_template_expansion().await;
                }
                ConfigDocument::Static(rule) => load_rule(state, hash, rule, &file).await,
            }
        }
        ConfigItemEvent::Removed(hash) => {
            let hash = ItemHash::from(hash);
            if state.rule_templates.remove_template(hash) || state.rule_templates.remove_source(hash) {
                state.schedule_template_expansion().await;
            } else {
                unload_rule(state, hash).await?;
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

/// Registers a rule read from a config document or generated from a template.
pub async fn load_rule(state: &mut AppState, hash: ItemHash, rule: Rule, file: &str) {
    let name = rule.name.clone();
    for problem in rule.validate() {
        tracing::warn!(rule = %name, file = %file, problem, "rule misconfigured");
    }
    let result = state
        .rules
        .add_rule(hash, rule, &state.cron, &state.mqtt_client, &state.solar_events, &state.dm, &*state.vdm.read().await)
        .await
        .map(|_| ());
    match result {
        Ok(()) => tracing::info!(rule = %name, file = %file, "rule loaded"),
        Err(err) => {
            tracing::error!(rule = %name, file = %file, error = chain(err.as_ref()), "cannot load rule")
        }
    }
}

/// Unregisters a rule; its timers, cron schedules, solar triggers and MQTT subscriptions end.
pub async fn unload_rule(state: &mut AppState, hash: ItemHash) -> Result<()> {
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
    Ok(())
}
