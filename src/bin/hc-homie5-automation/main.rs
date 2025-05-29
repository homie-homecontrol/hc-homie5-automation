use app::{deinitialize_app, initialize_app};
use app_env::{initialize_logging, initialize_panic_handler};
use color_eyre::eyre::Result;
use eventloop::run_event_loop;
use hc_homie5_automation::app_state::AppEvent;
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::mpsc,
};
mod app;
mod app_env;
mod eventloop;
mod settings;

// Check for SIGINT, SIGTERM and SIGQUIT signals to exit the application cleanly
async fn signal_handler(app_event_sender: mpsc::Sender<AppEvent>) {
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
    let mut sigquit = signal(SignalKind::quit()).expect("Failed to register SIGQUIT handler");

    tokio::select! {
        _ = sigint.recv() => log::info!("Received SIGINT"),
        _ = sigterm.recv() => log::info!("Received SIGTERM"),
        _ = sigquit.recv() => log::info!("Received SIGQUIT"),
    }

    if let Err(err) = app_event_sender.send(AppEvent::Exit).await {
        log::error!("Error sending exit event: {:#?}", err);
    }
}

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

    // Set handler to exit the application cleanly
    let exit_sender = state.app_event_sender.clone();
    tokio::spawn(signal_handler(exit_sender));

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
