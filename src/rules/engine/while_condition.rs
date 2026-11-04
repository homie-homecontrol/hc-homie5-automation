use crate::rules::{WhileCondition, WhileConditionSet};
use hc_homie5::store::DeviceStore;

/// No `while` always holds.
pub(crate) fn match_whilecondition_set(while_condition_set: Option<&WhileConditionSet>, devices: &DeviceStore) -> bool {
    while_condition_set
        .is_none_or(|conditions| conditions.evaluate(|condition| match_whilecondition(condition, devices)))
}

pub(crate) fn match_whilecondition(while_condition: &WhileCondition, devices: &DeviceStore) -> bool {
    match while_condition {
        WhileCondition::PropertyWhileCondition(property_while_condition) => devices
            .get_device(property_while_condition.property.device_ref())
            .and_then(|device| {
                device
                    .prop_values
                    .get_value_entry(property_while_condition.property.prop_pointer())
                    .and_then(|prop_value_entry| prop_value_entry.value.as_ref())
            })
            .is_some_and(|value| property_while_condition.condition.evaluate(value)),
        WhileCondition::TimeWhileCondition(time_while_condition) => time_while_condition.evaluate(),
    }
}
