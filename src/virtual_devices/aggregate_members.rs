use hc_homie5::store::DeviceStore;
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

#[cfg(test)]
mod aggregate_tests {
    use super::*;
    use homie5::{HomieDataType, HomieValue};

    // --- equal ---

    #[test]
    fn equal_all_same_returns_value() {
        let v = [HomieValue::Bool(true), HomieValue::Bool(true)];
        assert_eq!(aggregate_equal(v.iter()), Some(HomieValue::Bool(true)));
    }

    #[test]
    fn equal_differing_returns_none() {
        let v = [HomieValue::Bool(true), HomieValue::Bool(false)];
        assert_eq!(aggregate_equal(v.iter()), None);
    }

    #[test]
    fn equal_empty_returns_none() {
        let v: [HomieValue; 0] = [];
        assert_eq!(aggregate_equal(v.iter()), None);
    }

    // --- or ---

    #[test]
    fn or_happy_path() {
        let v = [HomieValue::Bool(false), HomieValue::Bool(true)];
        assert_eq!(aggregate_or(v.iter()), Some(HomieValue::Bool(true)));
        let v = [HomieValue::Bool(false), HomieValue::Bool(false)];
        assert_eq!(aggregate_or(v.iter()), Some(HomieValue::Bool(false)));
    }

    #[test]
    fn or_empty_returns_none() {
        let v: [HomieValue; 0] = [];
        assert_eq!(aggregate_or(v.iter()), None);
    }

    #[test]
    fn or_ignores_non_boolean_values() {
        // Integer/String values are filtered out; only the Bool participates.
        let v = [
            HomieValue::Integer(5),
            HomieValue::Bool(true),
            HomieValue::String("x".into()),
        ];
        assert_eq!(aggregate_or(v.iter()), Some(HomieValue::Bool(true)));
        // All non-boolean → no values left → None.
        let v = [HomieValue::Integer(1), HomieValue::Float(2.0)];
        assert_eq!(aggregate_or(v.iter()), None);
    }

    // --- and ---

    #[test]
    fn and_happy_path() {
        let v = [HomieValue::Bool(true), HomieValue::Bool(true)];
        assert_eq!(aggregate_and(v.iter()), Some(HomieValue::Bool(true)));
        let v = [HomieValue::Bool(true), HomieValue::Bool(false)];
        assert_eq!(aggregate_and(v.iter()), Some(HomieValue::Bool(false)));
    }

    #[test]
    fn and_empty_returns_none() {
        let v: [HomieValue; 0] = [];
        assert_eq!(aggregate_and(v.iter()), None);
    }

    #[test]
    fn and_ignores_non_boolean_values() {
        let v = [HomieValue::Integer(5), HomieValue::Bool(false)];
        assert_eq!(aggregate_and(v.iter()), Some(HomieValue::Bool(false)));
    }

    // --- nor ---

    #[test]
    fn nor_happy_path() {
        // nor = !(or)
        let v = [HomieValue::Bool(false), HomieValue::Bool(false)];
        assert_eq!(aggregate_nor(v.iter()), Some(HomieValue::Bool(true)));
        let v = [HomieValue::Bool(true), HomieValue::Bool(false)];
        assert_eq!(aggregate_nor(v.iter()), Some(HomieValue::Bool(false)));
    }

    #[test]
    fn nor_empty_returns_none() {
        let v: [HomieValue; 0] = [];
        assert_eq!(aggregate_nor(v.iter()), None);
    }

    #[test]
    fn nor_ignores_non_boolean_values() {
        let v = [HomieValue::Integer(5), HomieValue::Bool(false)];
        assert_eq!(aggregate_nor(v.iter()), Some(HomieValue::Bool(true)));
    }

    // --- nand ---

    #[test]
    fn nand_happy_path() {
        // nand = !(and)
        let v = [HomieValue::Bool(true), HomieValue::Bool(true)];
        assert_eq!(aggregate_nand(v.iter()), Some(HomieValue::Bool(false)));
        let v = [HomieValue::Bool(true), HomieValue::Bool(false)];
        assert_eq!(aggregate_nand(v.iter()), Some(HomieValue::Bool(true)));
    }

    #[test]
    fn nand_empty_returns_none() {
        let v: [HomieValue; 0] = [];
        assert_eq!(aggregate_nand(v.iter()), None);
    }

    #[test]
    fn nand_ignores_non_boolean_values() {
        let v = [HomieValue::Integer(5), HomieValue::Bool(true)];
        assert_eq!(aggregate_nand(v.iter()), Some(HomieValue::Bool(false)));
    }

    // --- avg / avgceil ---

    #[test]
    fn avg_float_happy_path() {
        let v = [HomieValue::Float(1.0), HomieValue::Float(2.0)];
        assert_eq!(aggregate_avg(v.iter(), HomieDataType::Float), Some(HomieValue::Float(1.5)));
    }

    #[test]
    fn avg_integer_rounds_to_nearest() {
        // 4 / 3 = 1.333... → round → 1
        let v = [HomieValue::Integer(1), HomieValue::Integer(1), HomieValue::Integer(2)];
        assert_eq!(
            aggregate_avg(v.iter(), HomieDataType::Integer),
            Some(HomieValue::Integer(1))
        );
    }

    #[test]
    fn avgceil_integer_rounds_up() {
        // 4 / 3 = 1.333... → ceil → 2 (distinct from avg's 1)
        let v = [HomieValue::Integer(1), HomieValue::Integer(1), HomieValue::Integer(2)];
        assert_eq!(
            aggregate_avgceil(v.iter(), HomieDataType::Integer),
            Some(HomieValue::Integer(2))
        );
    }

    #[test]
    fn avgceil_float_ceils() {
        // (1.0 + 1.0 + 2.0) / 3 = 1.333... → ceil → 2.0
        let v = [HomieValue::Float(1.0), HomieValue::Float(1.0), HomieValue::Float(2.0)];
        assert_eq!(
            aggregate_avgceil(v.iter(), HomieDataType::Float),
            Some(HomieValue::Float(2.0))
        );
    }

    #[test]
    fn avg_empty_returns_none() {
        let v: [HomieValue; 0] = [];
        assert_eq!(aggregate_avg(v.iter(), HomieDataType::Integer), None);
        assert_eq!(aggregate_avgceil(v.iter(), HomieDataType::Float), None);
    }

    #[test]
    fn avg_ignores_non_numeric_values() {
        // Bool/String ignored; only the two numerics (2, 4) averaged → 3.
        let v = [
            HomieValue::Integer(2),
            HomieValue::Bool(true),
            HomieValue::String("x".into()),
            HomieValue::Integer(4),
        ];
        assert_eq!(
            aggregate_avg(v.iter(), HomieDataType::Integer),
            Some(HomieValue::Integer(3))
        );
    }

    // --- max / min ---

    #[test]
    fn max_happy_path() {
        let v = [HomieValue::Integer(1), HomieValue::Integer(5), HomieValue::Integer(3)];
        assert_eq!(
            aggregate_max(v.iter(), HomieDataType::Integer),
            Some(HomieValue::Integer(5))
        );
        let v = [HomieValue::Float(1.5), HomieValue::Float(0.5)];
        assert_eq!(aggregate_max(v.iter(), HomieDataType::Float), Some(HomieValue::Float(1.5)));
    }

    #[test]
    fn min_happy_path() {
        let v = [HomieValue::Integer(1), HomieValue::Integer(5), HomieValue::Integer(3)];
        assert_eq!(
            aggregate_min(v.iter(), HomieDataType::Integer),
            Some(HomieValue::Integer(1))
        );
        let v = [HomieValue::Float(1.5), HomieValue::Float(0.5)];
        assert_eq!(aggregate_min(v.iter(), HomieDataType::Float), Some(HomieValue::Float(0.5)));
    }

    #[test]
    fn max_min_empty_returns_none() {
        let v: [HomieValue; 0] = [];
        assert_eq!(aggregate_max(v.iter(), HomieDataType::Integer), None);
        assert_eq!(aggregate_min(v.iter(), HomieDataType::Float), None);
    }

    #[test]
    fn max_min_ignore_non_numeric_values() {
        let v = [
            HomieValue::Bool(true),
            HomieValue::Integer(7),
            HomieValue::String("x".into()),
            HomieValue::Integer(2),
        ];
        assert_eq!(
            aggregate_max(v.iter(), HomieDataType::Integer),
            Some(HomieValue::Integer(7))
        );
        assert_eq!(
            aggregate_min(v.iter(), HomieDataType::Integer),
            Some(HomieValue::Integer(2))
        );
    }
}
