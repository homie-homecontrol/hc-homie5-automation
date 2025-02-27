use app::{deinitialize_app, initialize_app};
use app_env::{initialize_logging, initialize_panic_handler};
use color_eyre::eyre::Result;
use eventloop::run_event_loop;
use hc_homie5_automation::app_state::AppEvent;
use tokio::runtime;
mod app;
mod app_env;
mod eventloop;

async fn run_application() -> Result<()> {
    initialize_logging()?;
    initialize_panic_handler()?;

    let (
        mut event_multiplexer,
        homie_discovery_client,
        homie_ctrl_device_client,
        mqtt_client_handle,
        solar_events_handle,
        mut state,
    ) = initialize_app().await?;

    // Set Ctrl-C handler to exit the application cleanly
    let ctrl_sender = state.app_event_sender.clone();
    if let Err(err) = ctrlc::set_handler(move || {
        let rt = runtime::Runtime::new().unwrap();

        let ctrl_sender = ctrl_sender.clone();
        rt.block_on(async move {
            ctrl_sender
                .send(AppEvent::Exit)
                .await
                .expect("Error during application shutdown!");
        });
    }) {
        log::error!("Fatal Error: Cannot set ctrl-c app exit handler:\n{:#?}", err);
        panic!("Will exit now");
    }

    run_event_loop(&mut event_multiplexer, &mut state).await?;

    deinitialize_app(homie_discovery_client, homie_ctrl_device_client, mqtt_client_handle, solar_events_handle).await?;

    // make sure the channels stay open until the end...
    drop(event_multiplexer);

    Ok(())
}

// #[tokio::main]
//#[tokio::main(worker_threads = 1)]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if let Err(e) = run_application().await {
        eprintln!("{} fatal error: {:?}", env!("CARGO_PKG_NAME"), e);
        Err(e)
    } else {
        Ok(())
    }
}
