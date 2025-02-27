# General Concepts

<small> back to [readme](../README.md)</small>

The following chapters describe some general concepts used in the different configuration files.

# Subject

A subject is another term for a homie property. It is identified by 2 different forms in the config.

### Topic like notation

Identifies the property via a mqtt topic like notation.

The simple form is:

```yaml
subject: device/node/property
```

The homie domain is omitted in this form, the one configured via `HACTL_HOMIE_DOMAIN` environment variable will be substituted automatically.

Alternatively the homie-domain can be specified:

```yaml
subject: homie_domain/device/node/property
```

### Object notation

Identifies the property via a object notation.

```yaml
subject:
    home_domain: homie_domain
    device_id: device
    node_id: node
    property_id: property
```

The homie domain is optional, if omitted the one configured via `HACTL_HOMIE_DOMAIN` environment variable will be substituted automatically.

**Note**: As of now, `hc-homie5-automation` only operates within one homie_domain. This might change in the future, to at the moment specifying the homie_domain is more or less useless.

# Homie Value

<small> back to [readme](../README.md)</small>

In order to be type safe homie values are expressed with their datatype in all the rules and virtual devices configuration yaml files.

| **Homie Type** | **Example**                        |
| -------------- | ---------------------------------- |
| **String**     | `String: "open"`                   |
| **Integer**    | `Integer: 42`                      |
| **Float**      | `Float: 3.14`                      |
| **Boolean**    | `Bool: true`                       |
| **Enum**       | `Enum: "toggle"`                   |
| **Color**      | `Color: "rgb,100,120,110"`         |
| **Datetime**   | `DateTime: "2024-10-08T10:15:30Z"` |
| **Duration**   | `Duration: "PT12H5M46S"`           |
| **JSON**       | `JSON: "{\"hello\": \"world\"}"`   |

This means whenever a value needs to be specified (e.g. for a value condition) please use the above mentioned expression.
Also note that homie-value is a yaml object so make sure to always put it in a new line in yaml after the attribute or add `{}` around it.

### Example:

```yaml
   # ...
   trigger_value:
      Bool: true
   # ...
   # Or in one line:
   trigger_value: { Bool: true }
   # ...

```

# Value condition

Whenever a matching value needs to be defined (either in a rule [e.g. `trigger_value`] or for a virtual devices [e.g. in a `mapping definition`]) the type value condition is used.

A value condition can be defined either very simplistically by only defining the matching value itself, by defining an advanced condition set or by specifying a regex pattern to match for string types.

#### Simple:

```yaml
trigger_value: { Integer: 42 }
```

#### Advanced:

```yaml
trigger_value:
    operator: "="
    value:
        - Integer: 42
```

#### Pattern:

```yaml
trigger_value:
    pattern: ".*hello.*world.*"
```

The advanced condition set includes 2 fields:

- `operator`: see supported operators chapter
- `value`: a value or list of values for which the operator is applied

## Supported Operators

Operators define how the `value` is compared to the subject value. The following lists all available operators:

| **Operator**         | **Key**        | **Description**                                                                     |
| -------------------- | -------------- | ----------------------------------------------------------------------------------- |
| **Equal**            | `=`            | Matches if the property value is **equal** to the specified value.                  |
| **Not Equal**        | `<>`           | Matches if the property value is **not equal** to the specified value.              |
| **Greater Than**     | `>`            | Matches if the property value is **greater** than the specified value.              |
| **Less Than**        | `<`            | Matches if the property value is **less** than the specified value.                 |
| **Greater or Equal** | `>=`           | Matches if the property value is **greater than or equal** to the specified value.  |
| **Less or Equal**    | `<=`           | Matches if the property value is **less than or equal** to the specified value.     |
| **Includes Any**     | `includesAny`  | Matches if the property value matches **any value** in the specified list.          |
| **Includes None**    | `includesNone` | Matches if the property value matches **none of the values** in the specified list. |
| **Always Match**     | `matchAlways`  | Matches always regardless of the value.                                             |
| **Match if empty**   | `isEmpty`      | Matches if value is `null`, `none`, `nil`                                           |
| **Value available**  | `exists`       | Matches if value is NOT empty                                                       |

### Operator Examples

#### Equal (`=`):

```yaml
trigger_value:
    operator: "="
    value:
        - Integer: 42
```

Triggers when the property value is exactly `42`.

#### Not Equal (`<>`):

```yaml
trigger_value:
    operator: "<>"
    value:
        - String: "offline"
```

Triggers when the property value is **not** `"offline"`.

#### Greater Than (`>`) and Less Than (`<`):

```yaml
trigger_value:
    operator: ">"
    value:
        - Float: 20.5
```

Triggers when the property value is greater than `20.5`.

#### Includes Any (`includesAny`):

```yaml
trigger_value:
    operator: "includesAny"
    value:
        - Enum: "on"
        - Enum: "off"
```

Triggers when the property value is `"on"` or `"off"`.

#### Includes None (`includesNone`):

```yaml
trigger_value:
    operator: "includesNone"
    value:
        - String: "error"
        - String: "maintenance"
```

Triggers if the property value is **not** `"error"` or `"maintenance"`.

#### Always Match (`matchAlways`):

```yaml
trigger_value:
    operator: "matchAlways"
```

Triggers for **all property values**.

# Query

With queries properties can be queried based on their definition as well as their node's and device's definition.

Usage:

```yaml
queries:
    - device:
          id: optional value condition
          name: optional value condition
          version: optional value condition
          homie: optional value condition
          children: optional value condition
          root: optional value condition
          parent: optional value condition
          extensions: optional value condition
      node:
          id: optional value condition
          name: optional value condition
          type: optional value condition
      property:
          id: optional value condition
          name: optional value condition
          datatype: optional value condition
          format: optional value condition
          settable: optional value condition
          retained: optional value condition
          unit: optional value condition
```

A value condition can be defined for every attribute of the device, node and property definiton.
If a attribute is not defined all values match. The same goes for the `device`, `node` or `property` attribute of the query.

### Examples:

Omitting the whole `device` attribute will match all devices, or the other way around, not specifying a `node` or `property`
attribute but only a `device` will match all properties of a device.

Match all properties below a node of type "humidity" which is of integer or float type, unit % and is retained for any device.

```yaml
queries:
    - node:
          type: "humidity"
      property:
          datatype:
              operator: "="
              value:
                  - integer
                  - float
          retained: true
          unit: "%"
```

Match all properties of device with id "garden-weather-sensor"

```yaml
queries:
    - device:
          id: garden-weather-sensor
```
