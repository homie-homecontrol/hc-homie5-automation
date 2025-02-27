use color_eyre::eyre::Result;
use hc_homie5_automation::{app_state::AppState, rules::run_solar_rules, solar_events::SolarEvent};

pub async fn handle_solar_event(event: SolarEvent, state: &mut AppState) -> Result<bool> {
    log::debug!("Solarevent: {:?}", event);
    run_solar_rules(&event, &state.as_rule_ctx()).await;
    Ok(false)
}
