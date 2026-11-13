use hc_homie5::controller::MetaOverlayHandler;
use hc_homie5::store::DeviceStore;
use homie5::extensions::meta::MetaMessage;
use homie5::DeviceRef;

/// Applies a `$meta` message to the device store and returns the stored devices whose merged
/// overlay changed, so that queries matching on `$meta` annotations can be re-evaluated for them.
pub fn apply_meta_message(
    handler: &mut MetaOverlayHandler,
    msg: MetaMessage,
    devices: &mut DeviceStore,
) -> Vec<DeviceRef> {
    let candidates: Vec<DeviceRef> = match &msg {
        MetaMessage::ProviderInfo { .. } => Vec::new(),
        MetaMessage::DeviceOverlay {
            homie_domain,
            device_id,
            ..
        }
        | MetaMessage::DeviceOverlayRemoval {
            homie_domain,
            device_id,
            ..
        } => vec![DeviceRef::new(homie_domain.clone(), device_id.clone())],
        MetaMessage::ProviderRemoval {
            homie_domain,
            provider_id,
        } => devices
            .get_device_map(homie_domain)
            .map(|map| {
                map.values()
                    .filter(|device| device.meta_overlays().contains_key(provider_id))
                    .map(|device| device.ident.clone())
                    .collect()
            })
            .unwrap_or_default(),
    };

    let before: Vec<_> = candidates
        .into_iter()
        .filter_map(|device_ref| {
            let merged = devices.get_device(&device_ref)?.merged_meta_overlay().clone();
            Some((device_ref, merged))
        })
        .collect();

    handler.handle_meta_message(msg, devices);

    before
        .into_iter()
        .filter(|(device_ref, merged)| {
            devices
                .get_device(device_ref)
                .is_some_and(|device| device.merged_meta_overlay() != merged)
        })
        .map(|(device_ref, _)| device_ref)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use homie5::extensions::meta::MetaDeviceOverlay;
    use homie5::{HomieDeviceStatus, HomieDomain, HomieID};

    fn overlay(room: &str) -> MetaDeviceOverlay {
        serde_json::from_value(serde_json::json!({
            "schema": 1,
            "device": { "annotations": { "room": room } }
        }))
        .unwrap()
    }

    fn id(s: &str) -> HomieID {
        HomieID::try_from(s.to_owned()).unwrap()
    }

    fn setup() -> (MetaOverlayHandler, DeviceStore, DeviceRef) {
        let device_ref = DeviceRef::new(HomieDomain::Default, id("lamp"));
        let mut devices = DeviceStore::new();
        devices.add(&device_ref, HomieDeviceStatus::Ready);
        (MetaOverlayHandler::new(HomieDomain::Default), devices, device_ref)
    }

    fn device_overlay(provider: &str, device: &str, room: &str) -> MetaMessage {
        MetaMessage::DeviceOverlay {
            homie_domain: HomieDomain::Default,
            provider_id: id(provider),
            device_id: id(device),
            overlay: overlay(room),
        }
    }

    #[test]
    fn overlay_set_and_removed_reports_device() {
        let (mut handler, mut devices, device_ref) = setup();

        let changed = apply_meta_message(&mut handler, device_overlay("manual", "lamp", "hall"), &mut devices);
        assert_eq!(changed, vec![device_ref.clone()]);

        let removal = MetaMessage::DeviceOverlayRemoval {
            homie_domain: HomieDomain::Default,
            provider_id: id("manual"),
            device_id: id("lamp"),
        };
        assert_eq!(apply_meta_message(&mut handler, removal, &mut devices), vec![device_ref]);
    }

    #[test]
    fn identical_overlay_reports_nothing() {
        let (mut handler, mut devices, _) = setup();
        apply_meta_message(&mut handler, device_overlay("manual", "lamp", "hall"), &mut devices);
        let changed = apply_meta_message(&mut handler, device_overlay("manual", "lamp", "hall"), &mut devices);
        assert!(changed.is_empty());
    }

    #[test]
    fn undiscovered_device_reports_nothing() {
        let (mut handler, mut devices, _) = setup();
        let changed = apply_meta_message(&mut handler, device_overlay("manual", "other", "hall"), &mut devices);
        assert!(changed.is_empty());
    }

    #[test]
    fn provider_removal_reports_devices_of_that_provider() {
        let (mut handler, mut devices, device_ref) = setup();
        apply_meta_message(&mut handler, device_overlay("manual", "lamp", "hall"), &mut devices);
        let removal = MetaMessage::ProviderRemoval {
            homie_domain: HomieDomain::Default,
            provider_id: id("manual"),
        };
        assert_eq!(apply_meta_message(&mut handler, removal, &mut devices), vec![device_ref]);
    }
}
