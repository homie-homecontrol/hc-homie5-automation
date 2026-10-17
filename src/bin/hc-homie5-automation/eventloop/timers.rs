use color_eyre::eyre::Result;
use hc_homie5::logging::chain;
use hc_homie5_automation::{
    app_state::AppState,
    panic_guard::contain,
    rules::{execute_rule_action, run_timer_rules},
    timer_manager::TimerEvent,
};

pub async fn handle_timer_event(event: TimerEvent, state: &mut AppState) -> Result<bool> {
    tracing::debug!(
        timer = %event.id,
        rule = state.rules.get(&event.rule_hash).map(|r| r.name.as_str()),
        delayed_action = event.rule_action.is_some(),
        "timer fired"
    );

    if event.rule_action.is_none() {
        run_timer_rules(&event, &state.as_rule_ctx()).await;
    } else {
        let Some(action) = event.rule_action else {
            return Ok(false);
        };

        let Some(trigger_event) = event.trigger_event else {
            return Ok(false);
        };

        let Some(name) = state.rules.get(&event.rule_hash).map(|r| r.name.as_str()) else {
            return Ok(false);
        };
        tracing::info!(rule = %name, timer = %event.id, "delayed rule action started");
        match contain(execute_rule_action(event.rule_hash, name, &action, &trigger_event, &state.as_rule_ctx(), true))
            .await
        {
            Ok(Ok(_)) => {
                tracing::debug!(rule = %name, timer = %event.id, "delayed rule action finished");
            }
            Ok(Err(err)) => {
                tracing::error!(rule = %name, timer = %event.id, error = chain(err.as_ref()), "delayed rule action failed");
            }
            Err(panic) => {
                tracing::error!(rule = %name, timer = %event.id, error = chain(&panic), "delayed rule action panicked");
            }
        }
    }
    Ok(false)
}
