use crate::rules::{WhileCondition, WhileConditionSet};
use hc_homie5::DeviceStore;

pub(crate) fn match_whilecondition_set(while_condition_set: Option<&WhileConditionSet>, devices: &DeviceStore) -> bool {
    if let Some(while_conditions) = while_condition_set {
        match while_conditions {
            WhileConditionSet::Single(while_condition) => {
                if !match_whilecondition(while_condition, devices) {
                    return false;
                }
                true
            }
            WhileConditionSet::Multiple(vec) => {
                if vec.iter().any(|cond| !match_whilecondition(cond, devices)) {
                    return false;
                }
                true
            }
        }
    } else {
        true
    }
}

pub(crate) fn match_whilecondition(while_condition: &WhileCondition, devices: &DeviceStore) -> bool {
    match while_condition {
        WhileCondition::PropertyWhileCondition(property_while_condition) => devices
            .get_device(property_while_condition.subject.device_ref())
            .and_then(|device| {
                device
                    .prop_values
                    .get_value_entry(property_while_condition.subject.prop_pointer())
                    .and_then(|prop_value_entry| prop_value_entry.value.as_ref())
            })
            .is_some_and(|value| property_while_condition.condition.evaluate(value)),
        WhileCondition::TimeWhileCondition(time_while_condition) => time_while_condition.evaluate(),
    }
}
