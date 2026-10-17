use std::panic::PanicHookInfo;

use hc_homie5_automation::panic_guard::panic_is_contained;

/// Replaces the panic hook installed by `logging::init`.
///
/// Panics from rules, Lua, virtual devices, or configuration are caught by their guard (see
/// `panic_guard`), which reports them as a failure of that action; the hook only logs them at
/// DEBUG and the controller continues. Any other panic is logged as one ERROR line and ends the
/// process.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        if panic_is_contained() {
            log_contained_panic(info);
            return;
        }
        hc_homie5::logging::log_panic(info);
        std::process::exit(1);
    }));
}

fn log_contained_panic(info: &PanicHookInfo<'_>) {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload");
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_default();
    tracing::debug!(error = message, location = %location, "contained panic");
}
