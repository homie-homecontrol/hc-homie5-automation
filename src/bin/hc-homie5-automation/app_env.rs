// #![allow(dead_code)]

use std::path::PathBuf;

use crate::settings::ENV_PREFIX;
use color_eyre::eyre::Result;
use directories::ProjectDirs;
use tracing_error::ErrorLayer;
use tracing_subscriber::{self, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, Layer};

use once_cell::sync::Lazy;

pub static DATA_FOLDER: Lazy<Option<PathBuf>> =
    Lazy::new(|| std::env::var(format!("{}_DATA", *ENV_PREFIX)).ok().map(PathBuf::from));

pub static CONFIG_FOLDER: Lazy<Option<PathBuf>> =
    Lazy::new(|| std::env::var(format!("{}_CONFIG", *ENV_PREFIX)).ok().map(PathBuf::from));

pub static LOG_ENV: Lazy<String> = Lazy::new(|| format!("{}_LOGLEVEL", *ENV_PREFIX));

pub static LOG_FILE: Lazy<String> = Lazy::new(|| format!("{}.log", env!("CARGO_PKG_NAME")));

pub static LOG_TO_FILE: Lazy<bool> = Lazy::new(|| {
    std::env::var(format!("{}_LOG_TO_FILE", *ENV_PREFIX))
        .map(|v| v.parse::<bool>().unwrap_or(false))
        .unwrap_or(false)
});

pub static ENV_COLOR_LOG: Lazy<bool> = Lazy::new(|| {
    std::env::var(format!("{}_ENV_COLOR_LOG", *ENV_PREFIX))
        .map(|v| v.parse::<bool>().unwrap_or(true))
        .unwrap_or(true)
});

pub static LOG_SOURCE_FILES: Lazy<bool> = Lazy::new(|| {
    std::env::var(format!("{}_LOG_SOURCE_FILES", *ENV_PREFIX))
        .map(|v| v.parse::<bool>().unwrap_or(false))
        .unwrap_or(false)
});

fn project_directory() -> Option<ProjectDirs> {
    ProjectDirs::from("org", "schaze", env!("CARGO_PKG_NAME"))
}

pub fn initialize_panic_handler() -> Result<()> {
    // Set up a custom panic hook
    std::panic::set_hook(Box::new(move |panic_info| {
        #[cfg(not(debug_assertions))]
        {
            use human_panic::{handle_dump, print_msg, Metadata};
            let meta = Metadata::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

            let file_path = handle_dump(&meta, panic_info);
            // Prints the human-panic message
            print_msg(file_path, &meta).expect("human-panic: failed to print error message to console");
        }

        // Log the panic message
        let panic_message = format!("{}", panic_info);
        log::error!("Panic occurred: {}", panic_message);

        #[cfg(debug_assertions)]
        {
            // Use better-panic for enhanced stack trace in debug mode
            better_panic::Settings::auto()
                .most_recent_first(false)
                .lineno_suffix(true)
                .verbosity(better_panic::Verbosity::Full)
                .create_panic_handler()(panic_info);
        }

        // Exit the process
        std::process::exit(1);
    }));

    Ok(())
}

pub fn get_data_dir() -> PathBuf {
    let directory = if let Some(s) = DATA_FOLDER.clone() {
        s
    } else if let Some(proj_dirs) = project_directory() {
        proj_dirs.data_local_dir().to_path_buf()
    } else {
        PathBuf::from(".").join(".data")
    };
    directory
}

#[allow(unused)]
pub fn get_config_dir() -> PathBuf {
    let directory = if let Some(s) = CONFIG_FOLDER.clone() {
        s
    } else if let Some(proj_dirs) = project_directory() {
        proj_dirs.config_local_dir().to_path_buf()
    } else {
        PathBuf::from(".").join(".config")
    };
    directory
}

pub fn initialize_logging() -> Result<()> {
    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG")
            .or_else(|_| {
                std::env::var(LOG_ENV.clone())
                    .map(|log_level| format!("{}={}", env!("CARGO_CRATE_NAME").replace('-', "_"), log_level))
            })
            .unwrap_or_else(|_| format!("{}=info", env!("CARGO_CRATE_NAME"))),
    );
    let env_subscriber = tracing_subscriber::fmt::layer()
        .with_file(*LOG_SOURCE_FILES)
        .with_line_number(*LOG_SOURCE_FILES)
        .with_target(false)
        .with_ansi(*ENV_COLOR_LOG)
        .with_filter(tracing_subscriber::filter::EnvFilter::from_default_env());
    let registry = tracing_subscriber::registry()
        .with(env_subscriber)
        .with(ErrorLayer::default());
    if *LOG_TO_FILE {
        let directory = get_data_dir();
        std::fs::create_dir_all(directory.clone())?;
        let log_path = directory.join(LOG_FILE.clone());
        let log_file = std::fs::File::create(log_path)?;
        let file_subscriber = tracing_subscriber::fmt::layer()
            .with_file(*LOG_SOURCE_FILES)
            .with_line_number(*LOG_SOURCE_FILES)
            .with_writer(log_file)
            .with_target(false)
            .with_ansi(false)
            .with_filter(tracing_subscriber::filter::EnvFilter::from_default_env());
        registry.with(file_subscriber).init();
    } else {
        registry.init();
    }
    Ok(())
}

// /// Similar to the `std::dbg!` macro, but generates `tracing` events rather
// /// than printing to stdout.
// ///
// /// By default, the verbosity level for the generated events is `DEBUG`, but
// /// this can be customized.
// #[macro_export]
// macro_rules! trace_dbg {
//     (target: $target:expr, level: $level:expr, $ex:expr) => {{
//         match $ex {
//             value => {
//                 tracing::event!(target: $target, $level, ?value, stringify!($ex));
//                 value
//             }
//         }
//     }};
//     (level: $level:expr, $ex:expr) => {
//         trace_dbg!(target: module_path!(), level: $level, $ex)
//     };
//     (target: $target:expr, $ex:expr) => {
//         trace_dbg!(target: $target, level: tracing::Level::DEBUG, $ex)
//     };
//     ($ex:expr) => {
//         trace_dbg!(level: tracing::Level::DEBUG, $ex)
//     };
// }
