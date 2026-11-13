# General Concepts

<small> back to [readme](../README.md)</small>

The following chapters describe some general concepts used in the different configuration files.

## Property Reference

A property reference identifies a homie property using a slash-separated topic-like notation.

### Simple form (without domain)

```yaml
property: device/node/property
```

The homie domain is omitted in this form. The domain configured via `HCACTL_HOMIE_DOMAIN` is substituted automatically.

### Full form (with domain)

```yaml
property: homie_domain/device/node/property
```

**Note**: As of now, `hc-homie5-automation` only operates within one homie_domain. This might change in the future, so at the moment specifying the homie_domain is more or less useless.

## Homie Value

To keep values type-safe, Homie values are expressed with their datatype in rule and virtual-device YAML files.

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

## Value condition

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

Operators define how the `value` is compared to the property value. The following lists all available operators:

| **Operator**         | **Key**        | **Description**                                                                     |
| -------------------- | -------------- | ----------------------------------------------------------------------------------- |
| **Equal**            | `=`            | Matches if the property value is **equal** to the specified value.                  |
| **Not Equal**        | `<>`           | Matches if the property value is **not equal** to the specified value.              |
| **Greater Than**     | `>`            | Matches if the property value is **greater** than the specified value.              |
| **Less Than**        | `<`            | Matches if the property value is **less** than the specified value.                 |
| **Greater or Equal** | `>=`           | Matches if the property value is **greater than or equal** to the specified value.  |
| **Less or Equal**    | `<=`           | Matches if the property value is **less than or equal** to the specified value.     |
| **Includes Any**     | `includesAny`  | Matches if the property value **is one of** the values in the specified list.       |
| **Includes None**    | `includesNone` | Matches if the property value **is none of** the values in the specified list.       |
| **Always Match**     | `matchAlways`  | Matches always regardless of the value.                                             |
| **Match if empty**   | `isEmpty`      | Matches if value is `null`, `none`, `nil`                                           |
| **Value available**  | `exists`       | Matches if value is NOT empty                                                       |
| **Between**          | `between`      | Matches if the property value lies between two values `[min, max]`, both included.  |

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

Triggers if the property value is **none of** `"error"` and `"maintenance"`.

`includesAny` and `includesNone` compare a single value against a list; they behave like `=` and `<>` with a value list.

#### Between (`between`):

```yaml
trigger_value:
    operator: "between"
    value:
        - Float: 18.0
        - Float: 22.0
```

Triggers when the property value is at least `18.0` and at most `22.0`. The list must contain exactly two values, the lower bound first.

#### Always Match (`matchAlways`):

```yaml
trigger_value:
    operator: "matchAlways"
```

Triggers for **all property values**.

## Query

With queries properties can be queried based on their definition as well as their node's and device's definition.

Usage:

```yaml
queries:
    - device:
          id: optional value condition
          name: optional value condition
          version: optional value condition
          homie: optional value condition
          children: optional list condition
          root: optional value condition
          parent: optional value condition
          extensions: optional list condition
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
      meta:
          annotations:
              <annotation key>: list condition
```

A value condition can be defined for every attribute of the device, node and property definiton.
If a attribute is not defined all values match. The same goes for the `device`, `node`, `property` or `meta` attribute of the query.

### List conditions

Attributes that hold a list (`device.children`, `device.extensions` and every `$meta` annotation) take a
list condition: a mapping with exactly one quantifier key, each wrapping a value condition for the elements.

| Form            | Holds when                                      | Empty or missing list |
| --------------- | ----------------------------------------------- | --------------------- |
| `any: <cond>`   | at least one element matches                    | no                    |
| `all: <cond>`   | the list is non-empty and every element matches | no                    |
| `none: <cond>`  | no element matches                              | yes                   |
| `first: <cond>` | the first element matches                       | no                    |
| `empty: true`   | the list is empty (`false`: not)                | `true` / `false`      |

A bare value condition (for example `extensions: org.homie.legacy-stats`) is rejected; write
`extensions: { any: org.homie.legacy-stats }`.

### Meta annotations

`meta.annotations` matches the device-level annotations of the device's merged `$meta` overlay (all
providers, including the automation's own manual overlays). A single annotation value counts as a
one-element list, a missing annotation as an empty list. Annotation values are always strings, so compare
with `"true"`, not `true`.

When a device's `$meta` overlay changes, rule queries and query-based compound members are re-evaluated
for it: a device that now matches is added, one that no longer matches is dropped.

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

Match the `state` property of every contact sensor tagged `window` whose first room is the bathroom or the hall

```yaml
queries:
    - node:
          type: "hc-smarthome/v2/cap/contact"
      property:
          id: state
      meta:
          annotations:
              tags: { any: window }
              room:
                  first:
                      operator: "="
                      value: [bathroom, hall]
              heating: { none: "true" }
```
