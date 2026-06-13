//! Materialization of group virtual devices.
//!
//! A [`VirtualDeviceSpec`] with `group: Some(GroupSpec)` does not describe its
//! nodes/properties explicitly. Instead, the structure is derived ("materialized")
//! from the discovered [`HomieDeviceDescription`]s of the listed member devices:
//!
//! 1. Nodes and properties of all members are unioned (member list order
//!    defines priority for conflict resolution).
//! 2. Properties sharing the same node/property ID across members become a
//!    single compound property whose members are the source properties.
//! 3. Property descriptions are merged: widest number range, union of
//!    enum/color values, settable OR, first-wins for name/unit/retained.
//! 4. Aggregate functions default by merged datatype:
//!    boolean -> `or`, integer/float -> `avg`, everything else -> `equal`.
//! 5. The spec's own `nodes` act as an overlay on the generated structure:
//!    description fields patch the generated ones, `compound_spec` entries
//!    override aggregate/debounce/mapping (a non-empty `members` list replaces
//!    the generated members), `exclude: true` removes a generated node or
//!    property, and unknown IDs are added as-is (incl. `from_smarthome`).
//!
//! The result is an expanded [`VirtualDeviceSpec`] (with `group` consumed)
//! that feeds the unchanged `VirtualDevice::new()` construction path, plus a
//! stable hash over the expanded structure used as a rebuild gate.
//!
//! Materialization is a pure, synchronous function over [`DeviceStore`] data
//! and therefore fully unit-testable without MQTT.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use hc_homie5::store::DeviceStore;
use homie5::device_description::{FloatRange, HomiePropertyDescription, HomiePropertyFormat, IntegerRange};
use homie5::{DeviceRef, HomieDataType, HomieDomain, HomieID, PropertyRef};

use super::{AggregateFunctionType, CompoundSpec, GroupSpec, MemberSpec, VirtualDeviceSpec, VirtualNodeSpec, VirtualPropertySpec};

/// Result of materializing a group device spec.
#[derive(Debug)]
pub struct MaterializedGroup {
    /// Expanded spec ready for `VirtualDevice::new` (nodes fully populated,
    /// `group` field consumed).
    pub spec: VirtualDeviceSpec,
    /// Stable hash over the expanded structure; used to detect whether a
    /// discovery change actually altered the result (rebuild gate).
    /// Only comparable within the running process.
    pub structure_hash: u64,
}

/// Materialize a group device spec into an expanded [`VirtualDeviceSpec`].
///
/// Member devices that are not present in the store (or have no description
/// yet) are skipped -- the device is published partially and grows when the
/// members get discovered (a rebuild is triggered by the manager).
///
/// A spec without a `group` is passed through unchanged (`exclude` flags are
/// meaningless there and ignored with a warning).
pub fn materialize(spec: &VirtualDeviceSpec, devices: &DeviceStore, domain: &HomieDomain) -> MaterializedGroup {
    let Some(group) = &spec.group else {
        warn_about_ignored_excludes(spec);
        let spec = spec.clone();
        let structure_hash = structure_hash(&spec);
        return MaterializedGroup { spec, structure_hash };
    };

    let generated = generate_nodes(&spec.id, group, devices, domain);
    let nodes = apply_overlays(&spec.id, generated, &spec.nodes);

    let spec = VirtualDeviceSpec {
        id: spec.id.clone(),
        name: spec.name.clone(),
        version: spec.version,
        children: spec.children.clone(),
        parent: spec.parent.clone(),
        extensions: spec.extensions.clone(),
        group: None,
        nodes,
    };
    let structure_hash = structure_hash(&spec);

    MaterializedGroup { spec, structure_hash }
}

/// `exclude` flags only make sense as overlays on a group device. On plain
/// devices they are ignored -- tell the user instead of failing.
fn warn_about_ignored_excludes(spec: &VirtualDeviceSpec) {
    for node in &spec.nodes {
        if node.exclude.is_some() {
            log::warn!(
                "[vdevice:{}] 'exclude' on node '{}' is ignored: the device is not a group device",
                spec.id,
                node.id
            );
        }
        for prop in &node.properties {
            if prop.exclude.is_some() {
                log::warn!(
                    "[vdevice:{}] 'exclude' on property '{}/{}' is ignored: the device is not a group device",
                    spec.id,
                    node.id,
                    prop.id
                );
            }
        }
    }
}

/// A property description of one group member, tagged with its position in
/// the member list (priority) and the owning device id.
struct PropCandidate<'a> {
    member_idx: usize,
    device_id: &'a HomieID,
    desc: &'a HomiePropertyDescription,
}

/// Per-node accumulator while unioning member descriptions.
#[derive(Default)]
struct NodeAccumulator<'a> {
    /// First non-`None` node name across members (list order).
    name: Option<String>,
    /// First non-`None` node type across members (list order).
    r#type: Option<String>,
    props: BTreeMap<HomieID, Vec<PropCandidate<'a>>>,
}

/// Union all nodes/properties of the group members into generated
/// [`VirtualNodeSpec`]s (sorted by node id; property order within a node is
/// sorted by property id -- descriptions are `BTreeMap`-based anyway).
fn generate_nodes(
    group_id: &HomieID,
    group: &GroupSpec,
    devices: &DeviceStore,
    domain: &HomieDomain,
) -> Vec<VirtualNodeSpec> {
    let mut nodes: BTreeMap<HomieID, NodeAccumulator<'_>> = BTreeMap::new();

    for (member_idx, device_id) in group.devices.iter().enumerate() {
        let device_ref = DeviceRef::new(domain.clone(), device_id.clone());
        let Some(description) = devices.get_device(&device_ref).and_then(|d| d.description.as_ref()) else {
            log::debug!(
                "[group:{}] member device '{}' not discovered yet (or no description) -- skipping",
                group_id,
                device_id
            );
            continue;
        };

        for (node_id, node_desc) in &description.nodes {
            if group.exclude.iter().any(|e| e.excludes_node(node_id)) {
                continue;
            }
            let acc = nodes.entry(node_id.clone()).or_default();
            if acc.name.is_none() {
                acc.name = node_desc.name.clone();
            }
            if acc.r#type.is_none() {
                acc.r#type = node_desc.r#type.clone();
            }

            for (prop_id, prop_desc) in &node_desc.properties {
                if group.exclude.iter().any(|e| e.excludes_property(node_id, prop_id)) {
                    continue;
                }
                acc.props.entry(prop_id.clone()).or_default().push(PropCandidate {
                    member_idx,
                    device_id,
                    desc: prop_desc,
                });
            }
        }
    }

    nodes
        .into_iter()
        .filter_map(|(node_id, acc)| {
            let properties: Vec<VirtualPropertySpec> = acc
                .props
                .into_iter()
                .map(|(prop_id, candidates)| merge_property(group_id, group, domain, &node_id, prop_id, candidates))
                .collect();
            if properties.is_empty() {
                log::debug!("[group:{}] node '{}' has no remaining properties -- dropping", group_id, node_id);
                return None;
            }
            Some(VirtualNodeSpec {
                id: node_id,
                name: acc.name,
                r#type: acc.r#type,
                from_smarthome: None,
                pass_through: None,
                property_opts: None,
                exclude: None,
                properties,
            })
        })
        .collect()
}

/// Merge the candidate descriptions of one property id into a single
/// generated [`VirtualPropertySpec`] with a compound over the surviving
/// source properties.
fn merge_property(
    group_id: &HomieID,
    group: &GroupSpec,
    domain: &HomieDomain,
    node_id: &HomieID,
    prop_id: HomieID,
    candidates: Vec<PropCandidate<'_>>,
) -> VirtualPropertySpec {
    // --- datatype vote: majority wins, ties broken by member list order ---
    let datatype = vote_datatype(&candidates);
    let (winners, losers): (Vec<_>, Vec<_>) = candidates.into_iter().partition(|c| c.desc.datatype == datatype);
    for loser in &losers {
        log::warn!(
            "[group:{}] dropping '{}/{}/{}' from compound: datatype {} conflicts with majority datatype {}",
            group_id,
            loser.device_id,
            node_id,
            prop_id,
            loser.desc.datatype,
            datatype
        );
    }

    // --- description merge over the winning-datatype members ---
    let mut format = HomiePropertyFormat::Empty;
    let mut settable = false;
    let mut name: Option<String> = None;
    let mut unit: Option<String> = None;
    let retained = winners[0].desc.retained;

    for winner in &winners {
        format = merge_formats(group_id, node_id, &prop_id, winner.device_id, format, &winner.desc.format);
        settable |= winner.desc.settable;
        if name.is_none() {
            name = winner.desc.name.clone();
        }
        if unit.is_none() {
            unit = winner.desc.unit.clone();
        }
        if winner.desc.retained != retained {
            log::warn!(
                "[group:{}] retained mismatch for '{}/{}/{}': using retained={} from first member",
                group_id,
                winner.device_id,
                node_id,
                prop_id,
                retained
            );
        }
    }

    // --- compound generation ---
    let members = winners
        .iter()
        .map(|w| {
            MemberSpec::PropertyRef(PropertyRef::new(
                domain.clone(),
                w.device_id.clone(),
                node_id.clone(),
                prop_id.clone(),
            ))
        })
        .collect();
    let aggregate_function = default_aggregate(&datatype);
    let aggregation_debounce = if matches!(aggregate_function, AggregateFunctionType::Equal) {
        None
    } else {
        group.aggregation_debounce
    };

    VirtualPropertySpec {
        id: prop_id,
        name,
        datatype: Some(datatype),
        // The expanded spec carries the format in its string form; guarded by
        // is_empty() because Display on empty ranges is an error.
        format: (!format.is_empty()).then(|| format.to_string()),
        settable: Some(settable),
        retained: Some(retained),
        unit,
        pass_through: None,
        property_opts: None,
        exclude: None,
        compound_spec: Some(CompoundSpec {
            members,
            mapping: None,
            aggregate_function: Some(aggregate_function),
            aggregation_debounce,
        }),
    }
}

// ================ overlay application

/// Apply the spec's `nodes` overlays onto the generated nodes.
///
/// - An overlay node matching a generated node ID patches it (description
///   fields, `from_smarthome`, config flags, per-property overlays) or
///   removes it (`exclude: true`).
/// - Overlay nodes with unknown IDs are appended as-is (manual nodes like a
///   `scene` node keep working, incl. `from_smarthome`).
///
/// Generated node order is preserved; new overlay nodes follow in overlay
/// order.
fn apply_overlays(group_id: &HomieID, generated: Vec<VirtualNodeSpec>, overlays: &[VirtualNodeSpec]) -> Vec<VirtualNodeSpec> {
    let generated_ids: Vec<HomieID> = generated.iter().map(|n| n.id.clone()).collect();

    let mut nodes: Vec<VirtualNodeSpec> = generated
        .into_iter()
        .filter_map(|node| match overlays.iter().find(|o| o.id == node.id) {
            Some(overlay) if overlay.exclude == Some(true) => {
                log::debug!("[group:{}] node '{}' removed by overlay exclude", group_id, node.id);
                None
            }
            Some(overlay) => overlay_node(group_id, node, overlay),
            None => Some(node),
        })
        .collect();

    // append overlay nodes that did not match any generated node
    for overlay in overlays {
        if generated_ids.contains(&overlay.id) {
            continue;
        }
        if overlay.exclude == Some(true) {
            log::debug!(
                "[group:{}] overlay node '{}' has exclude set but nothing was generated for it -- skipping",
                group_id,
                overlay.id
            );
            continue;
        }
        nodes.push(overlay.clone());
    }
    nodes
}

/// Patch one generated node with its overlay. Returns `None` when the node
/// ends up without properties (and has no `from_smarthome` base that could
/// still produce some).
fn overlay_node(group_id: &HomieID, generated: VirtualNodeSpec, overlay: &VirtualNodeSpec) -> Option<VirtualNodeSpec> {
    let properties = overlay_properties(group_id, &generated.id, generated.properties, &overlay.properties);
    if properties.is_empty() && overlay.from_smarthome.is_none() {
        log::debug!(
            "[group:{}] node '{}' has no remaining properties after overlay -- dropping",
            group_id,
            generated.id
        );
        return None;
    }
    Some(VirtualNodeSpec {
        id: generated.id,
        name: overlay.name.clone().or(generated.name),
        r#type: overlay.r#type.clone().or(generated.r#type),
        // an overlay `from_smarthome` replaces the (always empty) generated
        // base; the merged properties are re-applied on top by
        // VirtualDevice::new()
        from_smarthome: overlay.from_smarthome.clone().or(generated.from_smarthome),
        pass_through: overlay.pass_through.or(generated.pass_through),
        property_opts: overlay.property_opts.clone().or(generated.property_opts),
        exclude: None,
        properties,
    })
}

/// Patch the generated properties of one node with the overlay's property
/// entries; overlay properties with unknown IDs are appended as-is.
fn overlay_properties(
    group_id: &HomieID,
    node_id: &HomieID,
    generated: Vec<VirtualPropertySpec>,
    overlays: &[VirtualPropertySpec],
) -> Vec<VirtualPropertySpec> {
    let generated_ids: Vec<HomieID> = generated.iter().map(|p| p.id.clone()).collect();

    let mut properties: Vec<VirtualPropertySpec> = generated
        .into_iter()
        .filter_map(|prop| match overlays.iter().find(|o| o.id == prop.id) {
            Some(overlay) if overlay.exclude == Some(true) => {
                log::debug!("[group:{}] property '{}/{}' removed by overlay exclude", group_id, node_id, prop.id);
                None
            }
            Some(overlay) => Some(overlay_property(prop, overlay)),
            None => Some(prop),
        })
        .collect();

    for overlay in overlays {
        if generated_ids.contains(&overlay.id) {
            continue;
        }
        if overlay.exclude == Some(true) {
            log::debug!(
                "[group:{}] overlay property '{}/{}' has exclude set but nothing was generated for it -- skipping",
                group_id,
                node_id,
                overlay.id
            );
            continue;
        }
        properties.push(overlay.clone());
    }
    properties
}

/// Patch one generated property with its overlay: description fields
/// (`VirtualPropertyDescription::patch` semantics on the spec level), config
/// fields, and the compound spec.
fn overlay_property(generated: VirtualPropertySpec, overlay: &VirtualPropertySpec) -> VirtualPropertySpec {
    VirtualPropertySpec {
        id: generated.id,
        name: overlay.name.clone().or(generated.name),
        datatype: overlay.datatype.or(generated.datatype),
        format: overlay.format.clone().or(generated.format),
        settable: overlay.settable.or(generated.settable),
        retained: overlay.retained.or(generated.retained),
        unit: overlay.unit.clone().or(generated.unit),
        pass_through: overlay.pass_through.or(generated.pass_through),
        property_opts: overlay.property_opts.clone().or(generated.property_opts),
        exclude: None,
        compound_spec: overlay_compound(generated.compound_spec, overlay.compound_spec.as_ref()),
    }
}

/// Merge an overlay compound spec into the generated one:
/// aggregate/debounce/mapping override when present; a non-empty `members`
/// list **replaces** the generated members.
fn overlay_compound(generated: Option<CompoundSpec>, overlay: Option<&CompoundSpec>) -> Option<CompoundSpec> {
    match (generated, overlay) {
        (generated, None) => generated,
        (None, Some(overlay)) => Some(overlay.clone()),
        (Some(generated), Some(overlay)) => Some(CompoundSpec {
            members: if overlay.members.is_empty() {
                generated.members
            } else {
                overlay.members.clone()
            },
            mapping: overlay.mapping.clone().or(generated.mapping),
            aggregate_function: overlay.aggregate_function.or(generated.aggregate_function),
            aggregation_debounce: overlay.aggregation_debounce.or(generated.aggregation_debounce),
        }),
    }
}

// ================ structure hash

/// Stable hash over the expanded structure of a materialized spec.
///
/// Covers everything that influences the published device description and the
/// compound wiring (node/property IDs, descriptions, members, aggregates).
/// Uses [`DefaultHasher`], so values are only comparable within the running
/// process -- exactly the lifetime of the rebuild gate that consumes them.
fn structure_hash(spec: &VirtualDeviceSpec) -> u64 {
    let mut hasher = DefaultHasher::new();
    spec.id.as_str().hash(&mut hasher);
    spec.name.hash(&mut hasher);
    spec.version.hash(&mut hasher);
    for child in &spec.children {
        child.as_str().hash(&mut hasher);
    }
    spec.parent.as_ref().map(|p| p.as_str()).hash(&mut hasher);
    spec.extensions.hash(&mut hasher);
    for node in &spec.nodes {
        node.id.as_str().hash(&mut hasher);
        node.name.hash(&mut hasher);
        node.r#type.hash(&mut hasher);
        format!("{:?}", node.from_smarthome).hash(&mut hasher);
        node.pass_through.hash(&mut hasher);
        format!("{:?}", node.property_opts).hash(&mut hasher);
        for prop in &node.properties {
            prop.id.as_str().hash(&mut hasher);
            prop.name.hash(&mut hasher);
            format!("{:?}", prop.datatype).hash(&mut hasher);
            prop.format.hash(&mut hasher);
            prop.settable.hash(&mut hasher);
            prop.retained.hash(&mut hasher);
            prop.unit.hash(&mut hasher);
            prop.pass_through.hash(&mut hasher);
            format!("{:?}", prop.property_opts).hash(&mut hasher);
            format!("{:?}", prop.compound_spec).hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Majority vote over the candidate datatypes; ties are broken by the
/// earliest member (list order) carrying the datatype.
fn vote_datatype(candidates: &[PropCandidate<'_>]) -> HomieDataType {
    // (datatype, count, first member index) -- in first-appearance order
    let mut votes: Vec<(HomieDataType, usize, usize)> = Vec::new();
    for candidate in candidates {
        if let Some(entry) = votes.iter_mut().find(|(dt, _, _)| *dt == candidate.desc.datatype) {
            entry.1 += 1;
        } else {
            votes.push((candidate.desc.datatype, 1, candidate.member_idx));
        }
    }
    votes
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
        .map(|(dt, _, _)| dt)
        .expect("vote_datatype called with at least one candidate")
}

/// Default aggregate function by merged datatype.
fn default_aggregate(datatype: &HomieDataType) -> AggregateFunctionType {
    match datatype {
        HomieDataType::Boolean => AggregateFunctionType::Or,
        HomieDataType::Integer | HomieDataType::Float => AggregateFunctionType::Avg,
        _ => AggregateFunctionType::Equal,
    }
}

/// Merge one member format into the accumulated format.
///
/// Rules (widest capability wins):
/// - empty vs non-empty: non-empty wins
/// - integer/float ranges: min of mins / max of maxes (an unbounded side stays
///   unbounded), smallest defined step
/// - enum: union of values, first-appearance order
/// - color: union of color formats, first-appearance order
/// - anything else: first wins; mismatches are logged
fn merge_formats(
    group_id: &HomieID,
    node_id: &HomieID,
    prop_id: &HomieID,
    device_id: &HomieID,
    acc: HomiePropertyFormat,
    next: &HomiePropertyFormat,
) -> HomiePropertyFormat {
    match (acc, next) {
        (HomiePropertyFormat::Empty, next) => next.clone(),
        (acc, HomiePropertyFormat::Empty) => acc,
        (HomiePropertyFormat::IntegerRange(a), HomiePropertyFormat::IntegerRange(b)) => {
            let merged = IntegerRange {
                min: a.min.and_then(|x| b.min.map(|y| x.min(y))),
                max: a.max.and_then(|x| b.max.map(|y| x.max(y))),
                step: merge_steps(a.step, b.step, |x, y| x.min(y)),
            };
            if merged.is_empty() {
                HomiePropertyFormat::Empty
            } else {
                HomiePropertyFormat::IntegerRange(merged)
            }
        }
        (HomiePropertyFormat::FloatRange(a), HomiePropertyFormat::FloatRange(b)) => {
            let merged = FloatRange {
                min: a.min.and_then(|x| b.min.map(|y| x.min(y))),
                max: a.max.and_then(|x| b.max.map(|y| x.max(y))),
                step: merge_steps(a.step, b.step, |x, y| x.min(y)),
            };
            if merged.is_empty() {
                HomiePropertyFormat::Empty
            } else {
                HomiePropertyFormat::FloatRange(merged)
            }
        }
        (HomiePropertyFormat::Enum(values), HomiePropertyFormat::Enum(other)) => {
            HomiePropertyFormat::Enum(union_in_order(values, other))
        }
        (HomiePropertyFormat::Color(formats), HomiePropertyFormat::Color(other)) => {
            HomiePropertyFormat::Color(union_in_order(formats, other))
        }
        (acc, next) => {
            if &acc != next {
                log::warn!(
                    "[group:{}] format conflict for '{}/{}/{}': keeping '{}' over '{}' (first wins)",
                    group_id,
                    device_id,
                    node_id,
                    prop_id,
                    acc,
                    next
                );
            }
            acc
        }
    }
}

/// Smallest defined step (`None` only when neither side defines one).
fn merge_steps<T: Copy>(a: Option<T>, b: Option<T>, min: impl Fn(T, T) -> T) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(min(x, y)),
        (step, None) | (None, step) => step,
    }
}

/// Union of two value lists preserving first-appearance order.
fn union_in_order<T: PartialEq + Clone>(mut acc: Vec<T>, other: &[T]) -> Vec<T> {
    for value in other {
        if !acc.contains(value) {
            acc.push(value.clone());
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_devices::SmarthomeSpec;
    use homie5::device_description::{ColorFormat, HomieDeviceDescription, HomieNodeDescription};
    use homie5::HomieDeviceStatus;
    use std::time::Duration;

    fn id(s: &str) -> HomieID {
        s.to_string().try_into().expect("valid homie id")
    }

    fn domain() -> HomieDomain {
        HomieDomain::Default
    }

    fn prop(datatype: HomieDataType, format: HomiePropertyFormat) -> HomiePropertyDescription {
        HomiePropertyDescription {
            name: None,
            datatype,
            format,
            settable: false,
            retained: true,
            unit: None,
        }
    }

    /// (node-id, node-name, [(prop-id, description)])
    type NodeFixture<'a> = (&'a str, Option<&'a str>, Vec<(&'a str, HomiePropertyDescription)>);

    /// Insert a device with the given nodes into the store.
    fn add_device(store: &mut DeviceStore, device_id: &str, nodes: Vec<NodeFixture<'_>>) {
        let device_ref = DeviceRef::new(domain(), id(device_id));
        let nodes: BTreeMap<HomieID, HomieNodeDescription> = nodes
            .into_iter()
            .map(|(node_id, name, props)| {
                (
                    id(node_id),
                    HomieNodeDescription {
                        name: name.map(str::to_string),
                        r#type: None,
                        properties: props.into_iter().map(|(prop_id, desc)| (id(prop_id), desc)).collect(),
                    },
                )
            })
            .collect();
        let description = HomieDeviceDescription {
            version: 1,
            nodes,
            ..Default::default()
        };
        store.add(&device_ref, HomieDeviceStatus::Ready);
        store.store_description(&device_ref, description);
    }

    fn group_spec(yaml: &str) -> VirtualDeviceSpec {
        serde_yaml_ng::from_str::<VirtualDeviceSpec>(yaml).expect("group spec must parse")
    }

    fn get_node<'a>(spec: &'a VirtualDeviceSpec, node_id: &str) -> &'a VirtualNodeSpec {
        spec.nodes
            .iter()
            .find(|n| n.id == id(node_id))
            .unwrap_or_else(|| panic!("node '{}' must be generated", node_id))
    }

    fn get_prop<'a>(spec: &'a VirtualDeviceSpec, node_id: &str, prop_id: &str) -> &'a VirtualPropertySpec {
        get_node(spec, node_id)
            .properties
            .iter()
            .find(|p| p.id == id(prop_id))
            .unwrap_or_else(|| panic!("property '{}/{}' must be generated", node_id, prop_id))
    }

    fn member_refs(prop: &VirtualPropertySpec) -> Vec<String> {
        prop.compound_spec
            .as_ref()
            .expect("compound spec must be generated")
            .members
            .iter()
            .map(|m| match m {
                MemberSpec::PropertyRef(p) => p.to_string(),
                other => panic!("generated members must be plain PropertyRefs, got {:?}", other),
            })
            .collect()
    }

    fn bool_prop(settable: bool) -> HomiePropertyDescription {
        HomiePropertyDescription {
            settable,
            ..prop(HomieDataType::Boolean, HomiePropertyFormat::Empty)
        }
    }

    fn int_range(min: Option<i64>, max: Option<i64>, step: Option<i64>) -> HomiePropertyFormat {
        HomiePropertyFormat::IntegerRange(IntegerRange { min, max, step })
    }

    fn float_range(min: Option<f64>, max: Option<f64>, step: Option<f64>) -> HomiePropertyFormat {
        HomiePropertyFormat::FloatRange(FloatRange { min, max, step })
    }

    fn enum_format(values: &[&str]) -> HomiePropertyFormat {
        HomiePropertyFormat::Enum(values.iter().map(|v| v.to_string()).collect())
    }

    // --- tests ---

    /// Core scenario: a switch-only member unioned with two full lights.
    #[test]
    fn unions_nodes_and_properties_across_members() {
        let mut store = DeviceStore::new();
        add_device(&mut store, "dev-a", vec![("switch", None, vec![("state", bool_prop(true))])]);
        add_device(
            &mut store,
            "dev-b",
            vec![
                ("switch", Some("Switch"), vec![("state", bool_prop(true))]),
                (
                    "level",
                    None,
                    vec![("value", prop(HomieDataType::Integer, int_range(Some(0), Some(100), None)))],
                ),
                (
                    "color",
                    None,
                    vec![(
                        "color",
                        prop(HomieDataType::Color, HomiePropertyFormat::Color(vec![ColorFormat::Rgb])),
                    )],
                ),
            ],
        );
        add_device(
            &mut store,
            "dev-c",
            vec![
                ("switch", Some("Other Name"), vec![("state", bool_prop(true))]),
                (
                    "level",
                    None,
                    vec![("value", prop(HomieDataType::Integer, int_range(Some(0), Some(254), None)))],
                ),
                (
                    "color",
                    None,
                    vec![(
                        "color",
                        prop(HomieDataType::Color, HomiePropertyFormat::Color(vec![ColorFormat::Hsv])),
                    )],
                ),
            ],
        );

        let spec = group_spec(
            r#"
id: my-lights
name: My Lights
group:
  devices: [dev-a, dev-b, dev-c]
  aggregation_debounce: 150ms
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        assert!(result.group.is_none());
        assert_eq!(result.id, id("my-lights"));
        assert_eq!(result.nodes.len(), 3);

        // switch/state: 3 members in list order, or-aggregate, debounced
        let state = get_prop(&result, "switch", "state");
        assert_eq!(
            member_refs(state),
            vec![
                "homie/dev-a/switch/state".to_string(),
                "homie/dev-b/switch/state".to_string(),
                "homie/dev-c/switch/state".to_string(),
            ]
        );
        assert_eq!(state.datatype, Some(HomieDataType::Boolean));
        assert_eq!(state.settable, Some(true));
        let compound = state.compound_spec.as_ref().unwrap();
        assert!(matches!(compound.aggregate_function, Some(AggregateFunctionType::Or)));
        assert_eq!(compound.aggregation_debounce, Some(Duration::from_millis(150)));

        // node name: first member that defines one wins (dev-b)
        assert_eq!(get_node(&result, "switch").name.as_deref(), Some("Switch"));

        // level/value: only dev-b + dev-c, avg, widened range
        let level = get_prop(&result, "level", "value");
        assert_eq!(
            member_refs(level),
            vec!["homie/dev-b/level/value".to_string(), "homie/dev-c/level/value".to_string()]
        );
        assert_eq!(level.format.as_deref(), Some("0:254"));
        let compound = level.compound_spec.as_ref().unwrap();
        assert!(matches!(compound.aggregate_function, Some(AggregateFunctionType::Avg)));

        // color/color: equal aggregate (no debounce), color format union
        let color = get_prop(&result, "color", "color");
        assert_eq!(color.format.as_deref(), Some("rgb,hsv"));
        let compound = color.compound_spec.as_ref().unwrap();
        assert!(matches!(compound.aggregate_function, Some(AggregateFunctionType::Equal)));
        assert_eq!(compound.aggregation_debounce, None);
    }

    #[test]
    fn missing_or_undescribed_members_are_skipped() {
        let mut store = DeviceStore::new();
        add_device(&mut store, "dev-a", vec![("switch", None, vec![("state", bool_prop(false))])]);
        // dev-b: present but no description yet
        store.add(&DeviceRef::new(domain(), id("dev-b")), HomieDeviceStatus::Init);
        // dev-c: not in the store at all

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a, dev-b, dev-c]
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        assert_eq!(result.nodes.len(), 1);
        let state = get_prop(&result, "switch", "state");
        assert_eq!(member_refs(state), vec!["homie/dev-a/switch/state".to_string()]);
    }

    #[test]
    fn datatype_majority_vote_drops_losers() {
        let mut store = DeviceStore::new();
        add_device(&mut store, "dev-a", vec![("n", None, vec![("p", bool_prop(false))])]);
        add_device(
            &mut store,
            "dev-b",
            vec![("n", None, vec![("p", prop(HomieDataType::String, HomiePropertyFormat::Empty))])],
        );
        add_device(&mut store, "dev-c", vec![("n", None, vec![("p", bool_prop(true))])]);

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a, dev-b, dev-c]
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        let p = get_prop(&result, "n", "p");
        assert_eq!(p.datatype, Some(HomieDataType::Boolean));
        // dev-b (string) is dropped from the compound
        assert_eq!(member_refs(p), vec!["homie/dev-a/n/p".to_string(), "homie/dev-c/n/p".to_string()]);
        // settable OR over the surviving members
        assert_eq!(p.settable, Some(true));
    }

    #[test]
    fn datatype_tie_is_broken_by_member_list_order() {
        let mut store = DeviceStore::new();
        add_device(
            &mut store,
            "dev-a",
            vec![("n", None, vec![("p", prop(HomieDataType::String, HomiePropertyFormat::Empty))])],
        );
        add_device(&mut store, "dev-b", vec![("n", None, vec![("p", bool_prop(false))])]);

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a, dev-b]
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        let p = get_prop(&result, "n", "p");
        assert_eq!(p.datatype, Some(HomieDataType::String));
        assert_eq!(member_refs(p), vec!["homie/dev-a/n/p".to_string()]);
        // string -> equal aggregate
        assert!(matches!(
            p.compound_spec.as_ref().unwrap().aggregate_function,
            Some(AggregateFunctionType::Equal)
        ));
    }

    #[test]
    fn integer_and_float_ranges_widen() {
        let mut store = DeviceStore::new();
        add_device(
            &mut store,
            "dev-a",
            vec![(
                "n",
                None,
                vec![
                    ("int", prop(HomieDataType::Integer, int_range(Some(10), Some(20), Some(5)))),
                    ("float", prop(HomieDataType::Float, float_range(Some(0.5), Some(2.5), None))),
                    ("unbounded", prop(HomieDataType::Integer, int_range(Some(0), Some(100), None))),
                ],
            )],
        );
        add_device(
            &mut store,
            "dev-b",
            vec![(
                "n",
                None,
                vec![
                    ("int", prop(HomieDataType::Integer, int_range(Some(0), Some(100), Some(2)))),
                    ("float", prop(HomieDataType::Float, float_range(Some(1.0), Some(5.0), Some(0.1)))),
                    ("unbounded", prop(HomieDataType::Integer, int_range(None, Some(50), None))),
                ],
            )],
        );

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a, dev-b]
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        // min of mins, max of maxes, smallest defined step
        assert_eq!(get_prop(&result, "n", "int").format.as_deref(), Some("0:100:2"));
        assert_eq!(get_prop(&result, "n", "float").format.as_deref(), Some("0.5:5:0.1"));
        // a missing min on one member keeps the merged range unbounded below
        assert_eq!(get_prop(&result, "n", "unbounded").format.as_deref(), Some(":100"));
    }

    #[test]
    fn enum_union_keeps_first_appearance_order() {
        let mut store = DeviceStore::new();
        add_device(
            &mut store,
            "dev-a",
            vec![("n", None, vec![("p", prop(HomieDataType::Enum, enum_format(&["on", "off"])))])],
        );
        add_device(
            &mut store,
            "dev-b",
            vec![("n", None, vec![("p", prop(HomieDataType::Enum, enum_format(&["off", "toggle"])))])],
        );

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a, dev-b]
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;
        assert_eq!(get_prop(&result, "n", "p").format.as_deref(), Some("on,off,toggle"));
    }

    #[test]
    fn empty_format_loses_against_non_empty() {
        let mut store = DeviceStore::new();
        add_device(
            &mut store,
            "dev-a",
            vec![("n", None, vec![("p", prop(HomieDataType::Enum, HomiePropertyFormat::Empty))])],
        );
        add_device(
            &mut store,
            "dev-b",
            vec![("n", None, vec![("p", prop(HomieDataType::Enum, enum_format(&["a", "b"])))])],
        );

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a, dev-b]
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;
        assert_eq!(get_prop(&result, "n", "p").format.as_deref(), Some("a,b"));

        // all-empty stays empty (no format string at all)
        let spec_a_only = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a]
"#,
        );
        let result = materialize(&spec_a_only, &store, &domain()).spec;
        assert_eq!(get_prop(&result, "n", "p").format, None);
    }

    #[test]
    fn group_excludes_suppress_nodes_and_properties() {
        let mut store = DeviceStore::new();
        add_device(
            &mut store,
            "dev-a",
            vec![
                ("switch", None, vec![("state", bool_prop(true)), ("action", bool_prop(true))]),
                (
                    "color",
                    None,
                    vec![(
                        "color",
                        prop(HomieDataType::Color, HomiePropertyFormat::Color(vec![ColorFormat::Rgb])),
                    )],
                ),
                ("button", None, vec![("event", prop(HomieDataType::Enum, enum_format(&["press"])))]),
            ],
        );

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a]
  exclude:
    - color
    - switch/action
    - button/event
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        // color node excluded, switch/action property excluded,
        // button dropped because its only property is excluded
        assert_eq!(result.nodes.len(), 1);
        let switch = get_node(&result, "switch");
        assert_eq!(switch.properties.len(), 1);
        assert_eq!(switch.properties[0].id, id("state"));
    }

    #[test]
    fn retained_and_unit_first_wins() {
        let mut store = DeviceStore::new();
        add_device(
            &mut store,
            "dev-a",
            vec![(
                "n",
                None,
                vec![(
                    "p",
                    HomiePropertyDescription {
                        retained: false,
                        ..prop(HomieDataType::Float, HomiePropertyFormat::Empty)
                    },
                )],
            )],
        );
        add_device(
            &mut store,
            "dev-b",
            vec![(
                "n",
                None,
                vec![(
                    "p",
                    HomiePropertyDescription {
                        retained: true,
                        unit: Some("°C".to_string()),
                        name: Some("Temperature".to_string()),
                        ..prop(HomieDataType::Float, HomiePropertyFormat::Empty)
                    },
                )],
            )],
        );

        let spec = group_spec(
            r#"
id: my-group
group:
  devices: [dev-a, dev-b]
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;
        let p = get_prop(&result, "n", "p");
        // retained: first member wins (mismatch is only warned about)
        assert_eq!(p.retained, Some(false));
        // name/unit: first member that defines them wins
        assert_eq!(p.unit.as_deref(), Some("°C"));
        assert_eq!(p.name.as_deref(), Some("Temperature"));
    }

    #[test]
    fn non_group_spec_passes_through_unchanged() {
        let store = DeviceStore::new();
        let spec = group_spec(
            r#"
id: plain-device
nodes:
  - id: switch
    exclude: true
    properties:
      - id: state
        datatype: boolean
        exclude: true
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;
        assert!(result.group.is_none());
        // exclude flags are meaningless on a non-group device: ignored
        // (with a warning), nothing is removed
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].id, id("switch"));
        assert_eq!(result.nodes[0].properties[0].id, id("state"));
    }

    // --- WP-2.2: overlays & structure hash ---

    /// Two identical lights for the overlay tests: switch (state bool,
    /// action enum) + level node. `dev_b_level_max` makes the structural
    /// merge result tweakable for the hash sensitivity test.
    fn overlay_store_with(dev_b_level_max: i64) -> DeviceStore {
        let mut store = DeviceStore::new();
        for (device, level_max) in [("dev-a", 254), ("dev-b", dev_b_level_max)] {
            add_device(
                &mut store,
                device,
                vec![
                    (
                        "switch",
                        None,
                        vec![
                            ("state", bool_prop(true)),
                            ("action", prop(HomieDataType::Enum, enum_format(&["press", "hold"]))),
                        ],
                    ),
                    (
                        "level",
                        None,
                        vec![("value", prop(HomieDataType::Integer, int_range(Some(0), Some(level_max), None)))],
                    ),
                ],
            );
        }
        store
    }

    fn overlay_store() -> DeviceStore {
        overlay_store_with(254)
    }

    /// The `toggle` case from lights.vdevice.yaml: overlay description
    /// fields patch the generated ones, everything else stays generated.
    #[test]
    fn overlay_patches_generated_descriptions() {
        let store = overlay_store();
        let spec = group_spec(
            r#"
id: my-lights
group:
  devices: [dev-a, dev-b]
  aggregation_debounce: 150ms
nodes:
  - id: switch
    name: Main Switch
    properties:
      - id: action
        format: toggle
        retained: false
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        let switch = get_node(&result, "switch");
        assert_eq!(switch.name.as_deref(), Some("Main Switch"));

        // patched fields
        let action = get_prop(&result, "switch", "action");
        assert_eq!(action.format.as_deref(), Some("toggle"));
        assert_eq!(action.retained, Some(false));
        // untouched generated fields: datatype, members, aggregate
        assert_eq!(action.datatype, Some(HomieDataType::Enum));
        assert_eq!(
            member_refs(action),
            vec!["homie/dev-a/switch/action".to_string(), "homie/dev-b/switch/action".to_string()]
        );

        // sibling property and other nodes are untouched
        let state = get_prop(&result, "switch", "state");
        assert_eq!(state.datatype, Some(HomieDataType::Boolean));
        assert_eq!(get_prop(&result, "level", "value").format.as_deref(), Some("0:254"));
    }

    #[test]
    fn overlay_compound_overrides_and_member_replacement() {
        let store = overlay_store();
        let spec = group_spec(
            r#"
id: my-lights
group:
  devices: [dev-a, dev-b]
  aggregation_debounce: 150ms
nodes:
  - id: switch
    properties:
      - id: state
        compound_spec:
          aggregate_function: and
          aggregation_debounce: 500ms
          mapping:
            output:
              - { to: !Bool false }
  - id: level
    properties:
      - id: value
        compound_spec:
          members:
            - other-dev/level/value
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        // aggregate/debounce/mapping override; members stay generated
        let state = get_prop(&result, "switch", "state");
        let compound = state.compound_spec.as_ref().unwrap();
        assert!(matches!(compound.aggregate_function, Some(AggregateFunctionType::And)));
        assert_eq!(compound.aggregation_debounce, Some(Duration::from_millis(500)));
        assert!(compound.mapping.is_some());
        assert_eq!(
            member_refs(state),
            vec!["homie/dev-a/switch/state".to_string(), "homie/dev-b/switch/state".to_string()]
        );

        // non-empty overlay members replace the generated list; absent
        // aggregate/debounce keep the generated values
        let level = get_prop(&result, "level", "value");
        assert_eq!(member_refs(level), vec!["homie/other-dev/level/value".to_string()]);
        let compound = level.compound_spec.as_ref().unwrap();
        assert!(matches!(compound.aggregate_function, Some(AggregateFunctionType::Avg)));
        assert_eq!(compound.aggregation_debounce, Some(Duration::from_millis(150)));
    }

    #[test]
    fn overlay_exclude_removes_generated_nodes_and_properties() {
        let store = overlay_store();
        let spec = group_spec(
            r#"
id: my-lights
group:
  devices: [dev-a, dev-b]
nodes:
  - id: level
    exclude: true
  - id: switch
    properties:
      - id: action
        exclude: true
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        // level node and switch/action removed; switch/state survives
        assert_eq!(result.nodes.len(), 1);
        let switch = get_node(&result, "switch");
        assert_eq!(switch.properties.len(), 1);
        assert_eq!(switch.properties[0].id, id("state"));
        // consumed overlay flags are cleared in the expanded spec
        assert_eq!(switch.exclude, None);
        assert_eq!(switch.properties[0].exclude, None);
    }

    #[test]
    fn overlay_excluding_all_properties_drops_the_node() {
        let store = overlay_store();
        let spec = group_spec(
            r#"
id: my-lights
group:
  devices: [dev-a, dev-b]
nodes:
  - id: switch
    properties:
      - id: state
        exclude: true
      - id: action
        exclude: true
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;
        assert!(result.nodes.iter().all(|n| n.id != id("switch")));
    }

    #[test]
    fn overlay_new_ids_pass_through_untouched() {
        let store = overlay_store();
        let spec = group_spec(
            r#"
id: my-lights
group:
  devices: [dev-a, dev-b]
nodes:
  - id: scene
    from_smarthome:
      type: contact
  - id: switch
    properties:
      - id: extra
        datatype: string
        compound_spec:
          members:
            - other-dev/some/prop
"#,
        );
        let result = materialize(&spec, &store, &domain()).spec;

        // new node appended as-is, incl. from_smarthome
        let scene = get_node(&result, "scene");
        assert_eq!(scene.from_smarthome, Some(SmarthomeSpec::Contact));

        // new property appended to the generated node, compound untouched
        let extra = get_prop(&result, "switch", "extra");
        assert_eq!(extra.datatype, Some(HomieDataType::String));
        assert_eq!(member_refs(extra), vec!["homie/other-dev/some/prop".to_string()]);
        // generated properties still present
        assert_eq!(get_node(&result, "switch").properties.len(), 3);
    }

    #[test]
    fn structure_hash_is_stable_and_sensitive() {
        let store = overlay_store();
        let spec = group_spec(
            r#"
id: my-lights
group:
  devices: [dev-a, dev-b]
nodes:
  - id: switch
    properties:
      - id: action
        format: toggle
"#,
        );

        // identical inputs -> identical hash
        let first = materialize(&spec, &store, &domain());
        let second = materialize(&spec, &store, &domain());
        assert_eq!(first.structure_hash, second.structure_hash);

        // a structural change in a member (widened range) changes the hash
        let changed_store = overlay_store_with(500);
        let changed = materialize(&spec, &changed_store, &domain());
        assert_ne!(first.structure_hash, changed.structure_hash);

        // an overlay change (different format override) changes the hash
        let other_spec = group_spec(
            r#"
id: my-lights
group:
  devices: [dev-a, dev-b]
nodes:
  - id: switch
    properties:
      - id: action
        format: press
"#,
        );
        let other = materialize(&other_spec, &store, &domain());
        assert_ne!(first.structure_hash, other.structure_hash);
    }

    /// D-7(3): generated formats are emitted as strings into the expanded
    /// spec and re-parsed by `VirtualDevice::new()` -- the round trip must be
    /// lossless.
    #[test]
    fn generated_formats_round_trip_through_string_form() {
        let cases: Vec<(HomieDataType, HomiePropertyFormat)> = vec![
            (HomieDataType::Integer, int_range(Some(0), Some(254), Some(2))),
            (HomieDataType::Integer, int_range(None, Some(50), None)),
            (HomieDataType::Integer, int_range(Some(-10), None, None)),
            (HomieDataType::Float, float_range(Some(0.5), Some(5.0), Some(0.1))),
            (HomieDataType::Float, float_range(None, Some(2.5), None)),
            (HomieDataType::Enum, enum_format(&["on", "off", "toggle"])),
            (
                HomieDataType::Color,
                HomiePropertyFormat::Color(vec![ColorFormat::Rgb, ColorFormat::Hsv]),
            ),
        ];
        for (datatype, format) in cases {
            let rendered = format.to_string();
            let parsed = HomiePropertyFormat::parse(&rendered, &datatype)
                .unwrap_or_else(|e| panic!("'{}' must parse back as {:?}: {:?}", rendered, datatype, e));
            assert_eq!(parsed, format, "round trip of '{}' must be lossless", rendered);
        }
    }
}
