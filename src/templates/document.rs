use std::fmt::Display;

use hc_homie5::template::Template;
use serde_yaml_ng::Value;

/// File name suffix of template files in the rules and virtual-devices config folders.
pub const TEMPLATE_FILE_SUFFIX: &str = ".tpl.yaml";

/// Keys that open a template construct.
const TEMPLATE_KEYS: &[&str] = &["use", "for_each", "when"];

/// True for a template file (`*.tpl.yaml`).
pub fn is_template_file(file: &str) -> bool {
    file.ends_with(TEMPLATE_FILE_SUFFIX)
}

/// One document of a rules or virtual-devices config file.
#[derive(Debug)]
pub enum ConfigDocument<T> {
    /// A document without template constructs or expressions, deserialized as before.
    Static(T),
    /// A template definition (`template:`, `params:`, `defaults:`, `body:`).
    Template(Value),
    /// A document with `use:`, `for_each:`, `when:` or `{{ }}` expressions, expanded later.
    Dynamic(Value),
}

/// Classifies a config document. Static documents are deserialized with `parse_static`, so they
/// load and report errors exactly as before; the values of `verbatim_keys` (Lua scripts) never
/// make a document dynamic.
pub fn parse_config_document<T, E: Display>(
    text: &str,
    verbatim_keys: &[&str],
    parse_static: impl FnOnce(&str) -> Result<T, E>,
) -> Result<ConfigDocument<T>, String> {
    let tree = match serde_yaml_ng::from_str::<Value>(text) {
        Ok(tree) => tree,
        // Report the typed parser's error, which carries more context.
        Err(err) => {
            return Err(parse_static(text)
                .err()
                .map_or_else(|| err.to_string(), |e| e.to_string()))
        }
    };
    if Template::is_template_document(&tree) {
        Ok(ConfigDocument::Template(tree))
    } else if is_dynamic(&tree, verbatim_keys) {
        Ok(ConfigDocument::Dynamic(tree))
    } else {
        parse_static(text)
            .map(ConfigDocument::Static)
            .map_err(|e| e.to_string())
    }
}

/// True if the tree has a template construct or a `{{` outside the values of `verbatim_keys`.
pub fn is_dynamic(value: &Value, verbatim_keys: &[&str]) -> bool {
    match value {
        Value::String(s) => s.contains("{{"),
        Value::Sequence(items) => items.iter().any(|v| is_dynamic(v, verbatim_keys)),
        Value::Mapping(m) => m.iter().any(|(k, v)| match k.as_str() {
            Some(k) if TEMPLATE_KEYS.contains(&k) => true,
            Some(k) if verbatim_keys.contains(&k) => false,
            _ => is_dynamic(v, verbatim_keys),
        }),
        Value::Tagged(tagged) => is_dynamic(&tagged.value, verbatim_keys),
        _ => false,
    }
}

/// True if the tree has a `for_each` (its expansion depends on the discovered devices).
pub(crate) fn has_for_each(value: &Value) -> bool {
    match value {
        Value::Sequence(items) => items.iter().any(has_for_each),
        Value::Mapping(m) => m.iter().any(|(k, v)| k.as_str() == Some("for_each") || has_for_each(v)),
        Value::Tagged(tagged) => has_for_each(&tagged.value),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<ConfigDocument<Value>, String> {
        parse_config_document(text, &["script"], |t: &str| serde_yaml_ng::from_str::<Value>(t))
    }

    #[test]
    fn classifies_documents() {
        assert!(matches!(parse("name: a\nactions: []"), Ok(ConfigDocument::Static(_))));
        assert!(matches!(parse("template: t\nbody: {}"), Ok(ConfigDocument::Template(_))));
        assert!(matches!(parse("use: t\nwith: {}"), Ok(ConfigDocument::Dynamic(_))));
        assert!(matches!(parse("name: \"{{ x }}\""), Ok(ConfigDocument::Dynamic(_))));
        assert!(matches!(parse("triggers:\n  - for_each: {}\n"), Ok(ConfigDocument::Dynamic(_))));
        assert!(parse("a: [").is_err());
    }

    #[test]
    fn script_braces_keep_a_document_static() {
        let doc = "name: a\nactions:\n  - type: run\n    script: |\n      local t = {{a=1},{b=2}}\n";
        assert!(matches!(parse(doc), Ok(ConfigDocument::Static(_))));
    }

    #[test]
    fn template_files() {
        assert!(is_template_file("heating.tpl.yaml"));
        assert!(!is_template_file("heating.yaml"));
        assert!(!is_template_file("tpl.yaml.bak"));
    }
}
