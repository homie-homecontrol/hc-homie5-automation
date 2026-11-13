//! Values that must not be evaluated as templates (Lua scripts).
//!
//! The engine evaluates every `{{` in a scalar, and Lua table literals (`{{a=1},{b=2}}`) contain
//! `{{`. Before a document or template is checked or expanded, each string value of a verbatim
//! key is replaced by a marker without braces; after expansion the markers are replaced by the
//! original text.

use std::collections::HashMap;

use config_watcher::hash_str;
use serde_yaml_ng::Value;

const MARKER_PREFIX: &str = "hc-automation-verbatim:";

/// Original texts by marker.
pub(crate) type Verbatim = HashMap<String, String>;

/// Replaces the string values of `keys` (at any depth) by markers and records the originals.
pub(crate) fn lift(value: &mut Value, keys: &[String], verbatim: &mut Verbatim) {
    match value {
        Value::Mapping(m) => {
            for (k, v) in m.iter_mut() {
                match (k.as_str(), v) {
                    (Some(key), Value::String(text)) if keys.iter().any(|k| k == key) => {
                        let marker = format!("{MARKER_PREFIX}{:016x}", hash_str(text));
                        verbatim.insert(marker.clone(), std::mem::replace(text, marker));
                    }
                    (_, v) => lift(v, keys, verbatim),
                }
            }
        }
        Value::Sequence(items) => items.iter_mut().for_each(|v| lift(v, keys, verbatim)),
        Value::Tagged(tagged) => lift(&mut tagged.value, keys, verbatim),
        _ => {}
    }
}

/// Puts the original texts back in place of their markers.
pub(crate) fn restore(value: &mut Value, verbatim: &Verbatim) {
    match value {
        Value::String(s) if s.starts_with(MARKER_PREFIX) => {
            if let Some(original) = verbatim.get(s.as_str()) {
                *s = original.clone();
            }
        }
        Value::Mapping(m) => m.iter_mut().for_each(|(_, v)| restore(v, verbatim)),
        Value::Sequence(items) => items.iter_mut().for_each(|v| restore(v, verbatim)),
        Value::Tagged(tagged) => restore(&mut tagged.value, verbatim),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lift_and_restore_round_trip() {
        let original: Value =
            serde_yaml_ng::from_str("actions:\n  - script: \"local t = {{a=1}}\"\n    other: \"{{ x }}\"\n").unwrap();
        let mut tree = original.clone();
        let mut verbatim = Verbatim::new();
        lift(&mut tree, &["script".to_owned()], &mut verbatim);

        let lifted = serde_yaml_ng::to_string(&tree).unwrap();
        assert!(!lifted.contains("{{a=1}}"));
        assert!(lifted.contains("{{ x }}"));

        restore(&mut tree, &verbatim);
        assert_eq!(tree, original);
    }
}
