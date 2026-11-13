use std::collections::{BTreeMap, BTreeSet, HashMap};

use config_watcher::hash_str;
use hc_homie5::template::{Engine, ExpandContext, Template, TemplateError, TemplateRegistry};
use serde_yaml_ng::Value;

use super::{
    document::has_for_each,
    scripts::{lift, restore, Verbatim},
};
use crate::config_item::ItemHash;

/// A template loaded from a `*.tpl.yaml` file.
struct TemplateEntry {
    file: String,
    template: Template,
    verbatim: Verbatim,
    for_each: bool,
}

/// A document with template constructs or expressions.
struct Source {
    file: String,
    tree: Value,
    verbatim: Verbatim,
    for_each: bool,
}

/// Result of [`TemplateStage::add_template`].
#[derive(Debug, PartialEq, Eq)]
pub struct TemplateAdded {
    pub name: String,
    /// The registry changed: documents must be re-expanded.
    pub changed: bool,
    /// Another loaded file defines a template with the same name; this one wins.
    pub shadows: Option<String>,
}

/// One document produced by expanding a source document.
#[derive(Debug, Clone)]
pub struct GeneratedDocument {
    /// Source file hash plus a hash of the expanded document: equal output, equal key.
    pub hash: ItemHash,
    pub source: ItemHash,
    pub file: String,
    pub doc: Value,
}

/// What changed since the previous [`TemplateStage::reconcile`].
#[derive(Debug, Default)]
pub struct Reconciliation {
    /// Generated items that are no longer produced; remove them first.
    pub removed: Vec<ItemHash>,
    /// Newly produced items.
    pub added: Vec<GeneratedDocument>,
    /// New or changed errors of source documents (each source is skipped while it fails).
    pub errors: Vec<String>,
}

/// Templates and template-using documents of one config kind (rules or virtual devices).
///
/// Sources keep their original tree and are expanded on every [`TemplateStage::reconcile`]; the
/// result is diffed by content hash against what was applied before, so unchanged results keep
/// their [`ItemHash`] (and with it rule timers and virtual devices).
pub struct TemplateStage {
    engine: Engine,
    verbatim_keys: Vec<String>,
    templates: HashMap<ItemHash, TemplateEntry>,
    registry: TemplateRegistry,
    sources: BTreeMap<ItemHash, Source>,
    applied: BTreeSet<ItemHash>,
    reported: HashMap<ItemHash, String>,
}

impl TemplateStage {
    /// `verbatim_keys`: keys whose string values are never evaluated (Lua `script`).
    pub fn new(verbatim_keys: &[&str]) -> Self {
        Self {
            engine: Engine::new(),
            verbatim_keys: verbatim_keys.iter().map(|k| (*k).to_owned()).collect(),
            templates: HashMap::new(),
            registry: TemplateRegistry::new(),
            sources: BTreeMap::new(),
            applied: BTreeSet::new(),
            reported: HashMap::new(),
        }
    }

    /// Loads a template document (from a `*.tpl.yaml` file).
    // TemplateError is large by design (hc-homie5 allows the same); it is only built on failure.
    #[allow(clippy::result_large_err)]
    pub fn add_template(&mut self, hash: ItemHash, file: &str, doc: Value) -> Result<TemplateAdded, TemplateError> {
        let mut tree = doc;
        let mut verbatim = Verbatim::new();
        lift(&mut tree, &self.verbatim_keys, &mut verbatim);
        let template = Template::from_value(&tree).map_err(|e| e.with_document(file))?;
        let name = template.name.clone();
        let shadows = self
            .templates
            .iter()
            .find(|(h, e)| **h != hash && e.template.name == name)
            .map(|(_, e)| e.file.clone());
        let changed = self.registry.insert(template.clone());
        self.templates.insert(
            hash,
            TemplateEntry {
                file: file.to_owned(),
                for_each: has_for_each(&template.body) || template.defaults.values().any(has_for_each),
                template,
                verbatim,
            },
        );
        Ok(TemplateAdded { name, changed, shadows })
    }

    /// Unloads a template document; returns `false` when `hash` is not a loaded template.
    pub fn remove_template(&mut self, hash: ItemHash) -> bool {
        let Some(entry) = self.templates.remove(&hash) else {
            return false;
        };
        let name = &entry.template.name;
        match self.templates.values().find(|e| &e.template.name == name) {
            Some(other) => {
                self.registry.insert(other.template.clone());
            }
            None => {
                self.registry.remove(name);
            }
        }
        true
    }

    /// Loads a document that uses templates or expressions.
    pub fn add_source(&mut self, hash: ItemHash, file: &str, doc: Value) {
        let mut tree = doc;
        let mut verbatim = Verbatim::new();
        lift(&mut tree, &self.verbatim_keys, &mut verbatim);
        let for_each = has_for_each(&tree);
        self.sources.insert(
            hash,
            Source {
                file: file.to_owned(),
                tree,
                verbatim,
                for_each,
            },
        );
    }

    /// Unloads a source document; returns `false` when `hash` is not a loaded source.
    pub fn remove_source(&mut self, hash: ItemHash) -> bool {
        self.reported.remove(&hash);
        self.sources.remove(&hash).is_some()
    }

    /// True when an expansion depends on the discovered devices (a `for_each` in a source or a
    /// template), so device and `$meta` changes must re-expand.
    pub fn uses_devices(&self) -> bool {
        self.sources.values().any(|s| s.for_each) || self.templates.values().any(|t| t.for_each)
    }

    /// Expands every source and returns the difference to the previous call. The caller removes
    /// `removed` and adds `added`; a source with check or expansion errors yields nothing.
    pub fn reconcile(&mut self, ctx: &dyn ExpandContext) -> Reconciliation {
        let mut verbatim = Verbatim::new();
        for entry in self.templates.values() {
            verbatim.extend(entry.verbatim.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        let mut desired: BTreeMap<ItemHash, GeneratedDocument> = BTreeMap::new();
        let mut errors = Vec::new();
        let mut reported = HashMap::new();
        for (source_hash, source) in &self.sources {
            let result = self.expand_source(source, ctx).and_then(|docs| {
                let mut generated = Vec::with_capacity(docs.len());
                for mut doc in docs {
                    restore(&mut doc, &source.verbatim);
                    restore(&mut doc, &verbatim);
                    let text = serde_yaml_ng::to_string(&doc).map_err(|e| format!("{}: {e}", source.file))?;
                    let hash = ItemHash::new(source_hash.filename_hash(), hash_str(&text));
                    if desired.contains_key(&hash) || generated.iter().any(|g: &GeneratedDocument| g.hash == hash) {
                        tracing::debug!(file = %source.file, "duplicate generated document ignored");
                        continue;
                    }
                    generated.push(GeneratedDocument {
                        hash,
                        source: *source_hash,
                        file: source.file.clone(),
                        doc,
                    });
                }
                Ok(generated)
            });
            match result {
                Ok(generated) => desired.extend(generated.into_iter().map(|g| (g.hash, g))),
                Err(message) => {
                    if self.reported.get(source_hash) != Some(&message) {
                        errors.push(message.clone());
                    }
                    reported.insert(*source_hash, message);
                }
            }
        }
        self.reported = reported;

        let removed: Vec<ItemHash> = self
            .applied
            .iter()
            .filter(|h| !desired.contains_key(h))
            .copied()
            .collect();
        let added: Vec<GeneratedDocument> = desired
            .values()
            .filter(|g| !self.applied.contains(&g.hash))
            .cloned()
            .collect();
        self.applied = desired.into_keys().collect();
        Reconciliation { removed, added, errors }
    }

    fn expand_source(&self, source: &Source, ctx: &dyn ExpandContext) -> Result<Vec<Value>, String> {
        let join = |errors: Vec<TemplateError>| {
            errors
                .into_iter()
                .map(|e| e.with_document(&source.file).to_string())
                .collect::<Vec<_>>()
                .join("; ")
        };
        let problems = self.engine.check(&source.tree, &self.registry);
        if !problems.is_empty() {
            return Err(join(problems));
        }
        self.engine
            .expand_documents(&source.tree, &self.registry, ctx)
            .map_err(|e| join(vec![e]))
    }
}

#[cfg(test)]
mod tests {
    use hc_homie5::{query::QueryDefinition, template::EmptyContext};

    use super::*;

    fn yaml(text: &str) -> Value {
        serde_yaml_ng::from_str(text).unwrap()
    }

    fn hash(file: u64, item: u64) -> ItemHash {
        ItemHash::new(file, item)
    }

    /// Yields the given elements for every query.
    struct Items(Vec<Value>);

    impl ExpandContext for Items {
        fn query(&self, _query: &QueryDefinition) -> Result<Vec<Value>, String> {
            Ok(self.0.clone())
        }
    }

    const RULE_TEMPLATE: &str = r#"
template: switch-rule
params: [device]
body:
  name: "switch {{ device }}"
  triggers:
    - properties: ["{{ device }}/switch/state"]
      changed: {}
  actions:
    - type: run
      script: |
        local t = {{a=1},{b=2}}
"#;

    #[test]
    fn script_values_are_not_evaluated() {
        let mut stage = TemplateStage::new(&["script"]);
        stage
            .add_template(hash(1, 1), "t.tpl.yaml", yaml(RULE_TEMPLATE))
            .unwrap();
        stage.add_source(hash(2, 1), "r.yaml", yaml("use: switch-rule\nwith: { device: lamp }"));

        let result = stage.reconcile(&EmptyContext);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.added.len(), 1);
        let doc = &result.added[0].doc;
        assert_eq!(doc["name"], yaml("switch lamp"));
        assert_eq!(doc["triggers"][0]["properties"][0], yaml("lamp/switch/state"));
        assert_eq!(doc["actions"][0]["script"], Value::String("local t = {{a=1},{b=2}}\n".into()));
    }

    #[test]
    fn script_in_source_document_is_untouched() {
        let mut stage = TemplateStage::new(&["script"]);
        stage.add_source(
            hash(2, 1),
            "r.yaml",
            yaml("name: \"{{ 'x' }}\"\nactions:\n  - type: run\n    script: \"{{a=1}}\"\n"),
        );
        let result = stage.reconcile(&EmptyContext);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.added[0].doc, yaml("name: x\nactions:\n  - type: run\n    script: \"{{a=1}}\"\n"));
    }

    #[test]
    fn literal_braces_outside_scripts() {
        let mut stage = TemplateStage::new(&["script"]);
        stage.add_source(hash(2, 1), "r.yaml", yaml("name: \"a {{ '{' ~ '{' }} b\""));
        let result = stage.reconcile(&EmptyContext);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.added[0].doc, yaml("name: a {{ b"));
    }

    #[test]
    fn unchanged_results_keep_their_hash() {
        let mut stage = TemplateStage::new(&["script"]);
        stage
            .add_template(hash(1, 1), "t.tpl.yaml", yaml(RULE_TEMPLATE))
            .unwrap();
        stage.add_source(
            hash(2, 1),
            "r.yaml",
            yaml("for_each: { query: {} }\nuse: switch-rule\nwith: { device: \"{{ item.id }}\" }"),
        );
        let items = |ids: &[&str]| Items(ids.iter().map(|id| yaml(&format!("id: {id}"))).collect());

        let first = stage.reconcile(&items(&["a", "b"]));
        assert_eq!(first.added.len(), 2);
        let hash_of_a = first
            .added
            .iter()
            .find(|g| g.doc["name"] == yaml("switch a"))
            .unwrap()
            .hash;

        // Same devices: nothing to do.
        let again = stage.reconcile(&items(&["a", "b"]));
        assert!(again.added.is_empty() && again.removed.is_empty());

        // A device appears: only its rule is added; `a` keeps its hash.
        let grown = stage.reconcile(&items(&["a", "b", "c"]));
        assert_eq!(grown.added.len(), 1);
        assert_eq!(grown.added[0].doc["name"], yaml("switch c"));
        assert!(grown.removed.is_empty());

        // The source document is edited (watcher: removed + added under a new hash) without
        // changing its output: nothing is replaced.
        stage.remove_source(hash(2, 1));
        stage.add_source(
            hash(2, 2),
            "r.yaml",
            yaml("when: true\nfor_each: { query: {} }\nuse: switch-rule\nwith: { device: \"{{ item.id }}\" }"),
        );
        let edited = stage.reconcile(&items(&["a", "b", "c"]));
        assert!(edited.added.is_empty() && edited.removed.is_empty());

        // A template change replaces every rule built from it.
        stage.remove_template(hash(1, 1));
        stage
            .add_template(hash(1, 2), "t.tpl.yaml", yaml(&RULE_TEMPLATE.replace("switch {{", "light {{")))
            .unwrap();
        let changed = stage.reconcile(&items(&["a", "b", "c"]));
        assert_eq!(changed.added.len(), 3);
        assert_eq!(changed.removed.len(), 3);
        assert!(changed.removed.contains(&hash_of_a));
    }

    #[test]
    fn check_errors_skip_only_the_faulty_document() {
        let mut stage = TemplateStage::new(&["script"]);
        stage
            .add_template(hash(1, 1), "t.tpl.yaml", yaml(RULE_TEMPLATE))
            .unwrap();
        stage.add_source(hash(2, 1), "good.yaml", yaml("use: switch-rule\nwith: { device: lamp }"));
        stage.add_source(hash(3, 1), "bad.yaml", yaml("use: switch-rule\nwith: { lamp: x }"));
        stage.add_source(hash(4, 1), "syntax.yaml", yaml("name: \"{{ a + }}\""));

        let result = stage.reconcile(&EmptyContext);

        assert_eq!(result.added.len(), 1);
        assert_eq!(result.added[0].file, "good.yaml");
        assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
        assert!(result
            .errors
            .iter()
            .any(|e| e.starts_with("bad.yaml: ") && e.contains("lamp")));
        assert!(result.errors.iter().any(|e| e.starts_with("syntax.yaml: at name")));

        // Errors are reported once, not on every re-expansion.
        assert!(stage.reconcile(&EmptyContext).errors.is_empty());
    }

    #[test]
    fn invalid_template_is_rejected_with_its_file() {
        let mut stage = TemplateStage::new(&[]);
        let err = stage
            .add_template(hash(1, 1), "t.tpl.yaml", yaml("template: x\nparams: [a, a]\nbody: {}"))
            .unwrap_err();
        assert!(err.to_string().starts_with("t.tpl.yaml: "), "{err}");
    }

    #[test]
    fn template_moved_between_files_stays_registered() {
        let mut stage = TemplateStage::new(&[]);
        stage
            .add_template(hash(1, 1), "a.tpl.yaml", yaml("template: x\nbody: { name: n }"))
            .unwrap();
        let added = stage
            .add_template(hash(2, 1), "b.tpl.yaml", yaml("template: x\nbody: { name: n }"))
            .unwrap();
        assert_eq!(added.shadows.as_deref(), Some("a.tpl.yaml"));
        assert!(stage.remove_template(hash(1, 1)));
        stage.add_source(hash(3, 1), "r.yaml", yaml("use: x"));
        let result = stage.reconcile(&EmptyContext);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.added.len(), 1);
    }

    #[test]
    fn uses_devices_only_with_for_each() {
        let mut stage = TemplateStage::new(&[]);
        stage.add_source(hash(1, 1), "r.yaml", yaml("use: x"));
        assert!(!stage.uses_devices());
        stage.add_source(hash(1, 2), "r.yaml", yaml("items:\n  - for_each: { query: {} }\n    item: {}"));
        assert!(stage.uses_devices());
    }
}
