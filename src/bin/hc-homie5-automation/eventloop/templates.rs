use color_eyre::eyre::Result;
use hc_homie5_automation::{
    app_state::AppState,
    panic_guard::contain_sync,
    rules::parse_rule,
    templates::{DeviceContext, GeneratedDocument},
    virtual_devices::VirtualDeviceSpec,
};

use super::rules::{load_rule, unload_rule};
use super::virtual_devices::{load_virtual_device, unload_virtual_device};

/// Re-expands all template-using rule and virtual-device documents and applies the difference:
/// items whose expansion is unchanged keep their hash, so rules keep their timers and virtual
/// devices stay published; changed items are replaced, vanished ones removed.
pub async fn expand_templates(state: &mut AppState) -> Result<()> {
    // Expand under the device-store read lock, apply after releasing it: adding rules and
    // virtual devices takes the lock again.
    let (vdevices, rules) = {
        let devices = state.dm.read().await;
        let ctx = DeviceContext::new(&devices);
        (state.vdevice_templates.reconcile(&ctx), state.rule_templates.reconcile(&ctx))
    };

    for error in vdevices.errors.iter() {
        tracing::error!(error = %error, "virtual device template expansion failed; document skipped");
    }
    for error in rules.errors.iter() {
        tracing::error!(error = %error, "rule template expansion failed; document skipped");
    }

    // Rules first out, virtual devices first in: on-set rule queries see the new devices.
    for hash in rules.removed {
        unload_rule(state, hash).await?;
    }
    for hash in vdevices.removed {
        unload_virtual_device(state, hash).await?;
    }
    for generated in vdevices.added {
        match parse_generated(&generated, |doc| serde_yaml_ng::from_value::<VirtualDeviceSpec>(doc.clone())) {
            Ok(spec) => load_virtual_device(state, generated.hash, spec, &generated.file).await,
            Err(error) => {
                tracing::error!(file = %generated.file, error, "generated virtual device is invalid; skipped")
            }
        }
    }
    for generated in rules.added {
        let rule = parse_generated(&generated, |doc| {
            serde_yaml_ng::to_string(doc)
                .map_err(|e| <serde_yaml_ng::Error as serde::de::Error>::custom(e.to_string()))
                .and_then(|text| parse_rule(&text))
        });
        match rule {
            Ok(rule) => load_rule(state, generated.hash, rule, &generated.file).await,
            Err(error) => tracing::error!(file = %generated.file, error, "generated rule is invalid; skipped"),
        }
    }
    Ok(())
}

/// Deserializes a generated document; a panic while deserializing is reported as an error.
fn parse_generated<T>(
    generated: &GeneratedDocument,
    parse: impl FnOnce(&serde_yaml_ng::Value) -> Result<T, serde_yaml_ng::Error>,
) -> Result<T, String> {
    match contain_sync(|| parse(&generated.doc)) {
        Ok(Ok(item)) => Ok(item),
        Ok(Err(error)) => Err(error.to_string()),
        Err(panic) => Err(panic.to_string()),
    }
}
