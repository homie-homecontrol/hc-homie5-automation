use color_eyre::eyre::Result;
use hc_homie5_automation::{
    app_state::AppState,
    rules::{execute_rule_action, run_timer_rules},
    timer_manager::TimerEvent,
};

pub async fn handle_timer_event(event: TimerEvent, state: &mut AppState) -> Result<bool> {
    log::debug!("Timerevent: {:?}", event);

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
        match execute_rule_action(event.rule_hash, name, &action, &trigger_event, &state.as_rule_ctx(), true).await {
            Ok(_) => {
                log::debug!("{} -- Rule completed: {}", name, name);
            }
            Err(err) => {
                log::error!("{} -- Error executing rule: {}", name, err);
            }
        }
    }
    Ok(false)
}
