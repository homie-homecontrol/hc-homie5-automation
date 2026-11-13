//! Templates in the rules and virtual-devices config (`*.tpl.yaml`, `use:`, `for_each`, `when:`,
//! `{{ }}` expressions), expanded with hc-homie5's template engine.
//!
//! Documents without template constructs load as before. Template documents fill a registry;
//! documents using templates or expressions are kept as YAML trees and expanded by a
//! [`TemplateStage`] whenever a template, a source document or (with `for_each`) the discovered
//! devices or their `$meta` change. Lua `script` values are never evaluated.

mod context;
mod document;
mod scripts;
mod stage;

pub use context::{device_snapshot, DeviceContext};
pub use document::{is_dynamic, is_template_file, parse_config_document, ConfigDocument, TEMPLATE_FILE_SUFFIX};
pub use stage::{GeneratedDocument, Reconciliation, TemplateAdded, TemplateStage};

/// Rule keys holding Lua code: their values are kept verbatim.
pub const RULE_VERBATIM_KEYS: &[&str] = &["script"];
/// Virtual devices hold no Lua code.
pub const VIRTUAL_DEVICE_VERBATIM_KEYS: &[&str] = &[];
