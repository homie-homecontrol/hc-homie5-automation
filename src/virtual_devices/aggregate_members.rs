use hc_homie5::DeviceStore;
use homie5::{HomieDataType, HomieDeviceStatus, HomieValue};

use super::{
    compound_member::{MqttCompoundMembers, PropertyCompoundMember},
    AggregateFunctionType,
};

pub fn update_compound_value<'a>(
    prop_compound_members: impl Iterator<Item = &'a PropertyCompoundMember>,
    mqtt_compoung_members: &MqttCompoundMembers,
    datatype: HomieDataType,
    devices: &DeviceStore,
    aggregate_function: AggregateFunctionType,
) -> Option<HomieValue> {
    let values = prop_compound_members
        .filter(|m| {
            matches!(
                devices
                    .device_state_resolved(m.prop.device_ref())
                    .unwrap_or(HomieDeviceStatus::Disconnected),
                HomieDeviceStatus::Ready
            )
        })
        .map(|m| m.value.as_ref())
        .chain(mqtt_compoung_members.values().map(|m| m.value.as_ref()))
        .flatten();

    // Select the aggregate function based on the AggregateFunctionType
    match aggregate_function {
        AggregateFunctionType::Equal => aggregate_equal(values),
        AggregateFunctionType::Or if matches!(datatype, HomieDataType::Boolean) => aggregate_or(values),
        AggregateFunctionType::And if matches!(datatype, HomieDataType::Boolean) => aggregate_and(values),
        AggregateFunctionType::Nor if matches!(datatype, HomieDataType::Boolean) => aggregate_nor(values),
        AggregateFunctionType::Nand if matches!(datatype, HomieDataType::Boolean) => aggregate_nand(values),
        AggregateFunctionType::Avg if matches!(datatype, HomieDataType::Integer | HomieDataType::Float) => {
            aggregate_avg(values, datatype)
        }
        AggregateFunctionType::AvgCeil if matches!(datatype, HomieDataType::Integer | HomieDataType::Float) => {
            aggregate_avgceil(values, datatype)
        }
        AggregateFunctionType::Max if matches!(datatype, HomieDataType::Integer | HomieDataType::Float) => {
            aggregate_max(values, datatype)
        }
        AggregateFunctionType::Min if matches!(datatype, HomieDataType::Integer | HomieDataType::Float) => {
            aggregate_min(values, datatype)
        }
        _ => None,
    }
}

pub fn aggregate_equal<'a>(mut states: impl Iterator<Item = &'a HomieValue> + 'a) -> Option<HomieValue> {
    let first = states.next()?;
    if states.all(|x| x == first) {
        Some(first.clone())
    } else {
        None
    }
}

pub fn aggregate_or<'a>(states: impl Iterator<Item = &'a HomieValue> + 'a) -> Option<HomieValue> {
    states
        .filter_map(|x| match x {
            HomieValue::Bool(b) => Some(*b),
            _ => None,
        })
        .reduce(|acc, b| acc || b)
        .map(HomieValue::Bool)
}

pub fn aggregate_and<'a>(states: impl Iterator<Item = &'a HomieValue> + 'a) -> Option<HomieValue> {
    states
        .filter_map(|x| match x {
            HomieValue::Bool(b) => Some(*b),
            _ => None,
        })
        .reduce(|acc, b| acc && b)
        .map(HomieValue::Bool)
}

pub fn aggregate_nor<'a>(states: impl Iterator<Item = &'a HomieValue> + 'a) -> Option<HomieValue> {
    states
        .filter_map(|x| match x {
            HomieValue::Bool(b) => Some(*b),
            _ => None,
        })
        .reduce(|acc, b| acc || b)
        .map(|b| HomieValue::Bool(!b))
}

pub fn aggregate_nand<'a>(states: impl Iterator<Item = &'a HomieValue> + 'a) -> Option<HomieValue> {
    states
        .filter_map(|x| match x {
            HomieValue::Bool(b) => Some(*b),
            _ => None,
        })
        .reduce(|acc, b| acc && b)
        .map(|b| HomieValue::Bool(!b)) // Default to true if no boolean values exist
}

pub fn aggregate_avg<'a>(
    states: impl Iterator<Item = &'a HomieValue> + 'a,
    datatype: HomieDataType,
) -> Option<HomieValue> {
    let values: Vec<f64> = states
        .filter_map(|x| match x {
            HomieValue::Integer(i) => Some(*i as f64),
            HomieValue::Float(f) => Some(*f),
            _ => None, // Ignore non-numeric values
        })
        .collect();

    if values.is_empty() {
        None
    } else {
        match datatype {
            HomieDataType::Float => {
                let sum: f64 = values.iter().sum();
                Some(HomieValue::Float(sum / values.len() as f64))
            }
            HomieDataType::Integer => {
                let sum: f64 = values.iter().sum();
                Some(HomieValue::Integer((sum / values.len() as f64).round() as i64))
            }
            _ => None,
        }
    }
}

pub fn aggregate_avgceil<'a>(
    states: impl Iterator<Item = &'a HomieValue> + 'a,
    datatype: HomieDataType,
) -> Option<HomieValue> {
    let values: Vec<f64> = states
        .filter_map(|x| match x {
            HomieValue::Integer(i) => Some(*i as f64),
            HomieValue::Float(f) => Some(*f),
            _ => None,
        })
        .collect();

    if values.is_empty() {
        None
    } else {
        match datatype {
            HomieDataType::Float => {
                let sum: f64 = values.iter().sum();
                Some(HomieValue::Float((sum / values.len() as f64).ceil()))
            }
            HomieDataType::Integer => {
                let sum: f64 = values.iter().sum();
                Some(HomieValue::Integer((sum / values.len() as f64).ceil() as i64))
            }
            _ => None,
        }
    }
}

pub fn aggregate_max<'a>(
    states: impl Iterator<Item = &'a HomieValue> + 'a,
    datatype: HomieDataType,
) -> Option<HomieValue> {
    states
        .filter_map(|x| match x {
            HomieValue::Integer(i) => Some(*i as f64),
            HomieValue::Float(f) => Some(*f),
            _ => None, // Ignore non-numeric values
        })
        .reduce(f64::max)
        .and_then(|v| match datatype {
            HomieDataType::Float => Some(HomieValue::Float(v)),
            HomieDataType::Integer => Some(HomieValue::Integer(v as i64)),
            _ => None,
        })
}

pub fn aggregate_min<'a>(
    states: impl Iterator<Item = &'a HomieValue> + 'a,
    datatype: HomieDataType,
) -> Option<HomieValue> {
    states
        .filter_map(|x| match x {
            HomieValue::Integer(i) => Some(*i as f64),
            HomieValue::Float(f) => Some(*f),
            _ => None,
        })
        .reduce(f64::min)
        .and_then(|v| match datatype {
            HomieDataType::Float => Some(HomieValue::Float(v)),
            HomieDataType::Integer => Some(HomieValue::Integer(v as i64)),
            _ => None,
        })
}
