use color_eyre::eyre::Result;
use hc_homie5_automation::{app_state::AppState, cron_manager::CronEvent, rules::run_cron_rules};

pub async fn handle_cron_event(event: CronEvent, state: &mut AppState) -> Result<bool> {
    tracing::debug!(
        rule = state.rules.get(&event.rule_hash).map(|r| r.name.as_str()),
        trigger = event.trigger_index,
        "cron schedule fired"
    );
    run_cron_rules(&event, &state.as_rule_ctx()).await;
    Ok(false)
}
