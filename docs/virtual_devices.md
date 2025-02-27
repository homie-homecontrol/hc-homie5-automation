# Virtual Device Overview

<small> back to [readme](../README.md)</small>

A **virtual device** is a Homie device definition enhanced with extra options that simplify configuration and add functionality. In essence, it is a virtual description of a Homie device with additional features that help automate node/property creation and manage device state.

**Extra Options Include:**

- **Predefined Node Types:**  
  The `from_smarthome` section lets you specify a predefined smart home device type (e.g., `switch`, `dimmer`) that automatically generates the necessary node and property definitions. This means you don't have to define every detail from scratch.

- **Set Value Pass-Through:**  
  With the `pass_through` option enabled on a node or property, any value received on the set command is immediately published as the property value.

- **MQTT State Restoration:**  
  The `property_opts` option can instruct the device to read the current state of a property from MQTT on startup. This is useful for restoring the last known state after a service restart.

## Simplified YAML Example

```yaml
id: virtual-light-device
name: Living Room Light
nodes:
    - id: switch
      from_smarthome:
          type: switch
          config:
              settable: true
      pass_through: true
      property_opts:
          read_from_mqtt: true
          read_timeout: 1000ms
      properties:
          - id: state
            name: Light State
            datatype: boolean
            settable: true
            retained: true
            pass_through: true
```

## Explanation

- Device Definition:

    - The device is uniquely identified by id: virtual-light-device and is named "Living Room Light."

- Node Configuration:

    - The node with id: switch uses a predefined smart home configuration via the from_smarthome section to automatically generate the necessary details for a switch.
    - pass_through: true means that any set command received by the node immediately becomes the property value.
    - property_opts is configured to read the current state from MQTT on startup, allowing state restoration after a restart.

- Property Definition:
    - The property with id: state represents the on/off state of the light.
    - It is defined as a boolean value, is settable, retained, and uses pass-through behavior to immediately reflect changes.

This example illustrates the essential structure and extra options available when defining a virtual Homie device.

# Device Specification

A virtual Homie device is defined at the top level of the YAML configuration file. Each device must have a unique `id` and can optionally include additional metadata such as a human-readable `name`, a `version` number, and hierarchical relationships (`children` and `parent`).

## Device Fields

| Key            | Required | Type    | Description                                                                             |
| -------------- | -------- | ------- | --------------------------------------------------------------------------------------- |
| **id**         | Yes      | string  | Unique identifier for the device. Must follow the HomieID pattern: `^[a-z0-9\-]+$`.     |
| **name**       | No       | string  | Human-readable name for the device.                                                     |
| **version**    | No       | integer | Optional version number for the configuration.                                          |
| **children**   | No       | array   | List of child device IDs if this device is a parent. Default: `[]`.                     |
| **parent**     | No       | string  | ID of a parent device if applicable.                                                    |
| **extensions** | No       | array   | List of extension names applied to the device. Default: `[]`.                           |
| **nodes**      | Yes      | array   | List of nodes that define the functional parts of the device (e.g., switches, sensors). |

## Example

```yaml
id: virtual-light-device
name: Living Room Light
version: 1
nodes:
    - id: switch
      from_smarthome:
          type: switch
          config:
              settable: true
      pass_through: true
      property_opts:
          read_from_mqtt: true
          read_timeout: 1000ms
```

# Node Specification

Nodes represent the functional parts of a Homie device. A node might correspond to a switch, dimmer, sensor, or other smart home component. Each node must have a unique `id` and can either be manually defined or automatically generated using `from_smarthome`.

## Node Fields

| Key                | Required | Type    | Description                                                                                     |
| ------------------ | -------- | ------- | ----------------------------------------------------------------------------------------------- |
| **id**             | Yes      | string  | Unique identifier for the node. Must follow the HomieID pattern.                                |
| **name**           | No       | string  | Human-readable name for the node.                                                               |
| **type**           | No       | string  | Type descriptor for the node.                                                                   |
| **from_smarthome** | Yes      | object  | Specifies a predefined smart home device type (`switch`, `dimmer`, etc.) and its configuration. |
| **pass_through**   | No       | boolean | If `true`, immediately publishes the set value as the property value. Default: `false`.         |
| **property_opts**  | No       | object  | Options for property state retrieval (`read_from_mqtt`, `read_timeout`).                        |
| **properties**     | No       | array   | List of properties defining the node's capabilities.                                            |

## Example

```yaml
id: virtual-device
name: Test Device
nodes:
    - id: switch
      from_smarthome:
          type: switch
          config:
              settable: true
      pass_through: true
    - id: dimmer
      from_smarthome:
          type: dimmer
      properties:
          - id: brightness
            datatype: integer
            settable: true
            retained: true
```

# Property Specification

Properties define individual characteristics of a node, such as on/off state, brightness, or temperature. Each property must have an `id` and a `datatype`. Additional options allow properties to be settable, retained, or aggregated from multiple sources.

## Property Fields

| Key               | Required | Type    | Description                                                                         |
| ----------------- | -------- | ------- | ----------------------------------------------------------------------------------- |
| **id**            | Yes      | string  | Unique identifier for the property. Must follow the HomieID pattern.                |
| **name**          | No       | string  | Human-readable name for the property.                                               |
| **datatype**      | Yes      | string  | Data type of the property (e.g., `integer`, `boolean`, `string`).                   |
| **format**        | No       | object  | Constraints on the allowed values (e.g., range, enum list, color format).           |
| **settable**      | No       | boolean | If `true`, the property value can be changed remotely. Default: `false`.            |
| **retained**      | No       | boolean | If `true`, the property value is retained in MQTT. Default: `false`.                |
| **unit**          | No       | string  | Unit of measurement (e.g., `"°C"`, `"lm"`).                                         |
| **pass_through**  | No       | boolean | If `true`, directly passes the set value without processing. Default: `false`.      |
| **property_opts** | No       | object  | Additional options for property state retrieval (`read_from_mqtt`, `read_timeout`). |
| **compound_spec** | No       | object  | Aggregates multiple values with an `aggregate_function` and `members`.              |

## Example

```yaml
id: virtual-dimmer
name: Virtual Dimmer
nodes:
    - id: dimmer
      from_smarthome:
          type: dimmer
      properties:
          - id: brightness
            name: Brightness Level
            datatype: integer
            format:
                IntegerRange:
                    min: 0
                    max: 100
            settable: true
            retained: true
            property_opts:
                read_from_mqtt: true
                read_timeout: 2s
```

# Compound Specification & Mapping

In a virtual Homie device, properties can be defined using **compound specifications**. This allows a property to be computed from multiple sources instead of just having a single value. These sources can be other Homie properties, MQTT topics, or even transformation functions.

Mappings define how values are transformed between different formats, which is particularly useful when combining multiple sources or converting between Homie values and MQTT messages.

---

## Compound Specification

A **compound specification** (`compound_spec`) allows a property to aggregate values from multiple sources, apply transformations, and define an output based on the combined values.

### Compound Specification Fields

| Key                      | Required | Type     | Description                                                                                              |
| ------------------------ | -------- | -------- | -------------------------------------------------------------------------------------------------------- |
| **members**              | Yes      | array    | List of sources that contribute to the property value. Can be Homie properties, MQTT topics, or queries. |
| **aggregate_function**   | No       | string   | Specifies how multiple values should be combined. Default: `"equal"`.                                    |
| **aggregation_debounce** | No       | duration | Time period to wait before applying aggregation. Useful for stabilizing fluctuating values.              |

### Aggregate Functions

The `aggregate_function` determines how multiple values from `members` are combined:

| Function    | Description                                                  |
| ----------- | ------------------------------------------------------------ |
| `"equal"`   | Uses the first non-null value.                               |
| `"or"`      | If any value is `true`, the result is `true`.                |
| `"and"`     | If all values are `true`, the result is `true`.              |
| `"nor"`     | Opposite of `"or"` (only `true` if all values are `false`).  |
| `"nand"`    | Opposite of `"and"` (only `false` if all values are `true`). |
| `"avg"`     | Computes the average of all values.                          |
| `"avgceil"` | Computes the average and rounds up.                          |
| `"max"`     | Returns the maximum value.                                   |
| `"min"`     | Returns the minimum value.                                   |

### Example: Aggregating Window Contacts

The following example creates a virtual property that indicates if any windows are open. It checks multiple contact sensors and combines them using an `"or"` function.

```yaml
id: windows
name: Indicates if any windows are open
nodes:
    - id: window-contacts
      name: Window Contacts
      from_smarthome:
          type: contact
      properties:
          - id: state
            datatype: boolean
            compound_spec:
                members:
                    - oeq0429851/contact/state
                    - meq0753110/contact/state
                    - query:
                          node:
                              id: contact
                          property:
                              id: state
                aggregate_function: or
```

**Explanation:**

- The `state` property checks multiple window contact sensors.
- The `members` list includes two specific devices (`oeq0429851`, `meq0753110`) and a `query` to match all `contact` nodes.
- The `"or"` function ensures the value is `true` if _any_ window is open.

---

## Value Mappings

Mappings allow transforming values between different formats. This is useful when:

- Converting between Homie and MQTT values.
- Translating between different value types (e.g., `"on"` → `true`).
- Handling non-standard device responses.

A mapping consists of **input-to-output transformations**. It can be used in both **input mapping** (reading values) and **output mapping** (writing values).

### Mapping Fields

| Key      | Required | Type             | Description                                               |
| -------- | -------- | ---------------- | --------------------------------------------------------- |
| **from** | No       | condition object | Defines the condition for matching the input value.       |
| **to**   | Yes      | value object     | Defines the transformed output value when `from` matches. |

The `from` condition can be:

- A **specific value** (e.g., `"on"`).
- A **comparison operator** (e.g., `"operator": "="` with `value: ["on"]`).
- A **catch-all default** (by omitting `from`).

### Example: Switch State Mapping

```yaml
id: virtual-switch
name: Switch with Custom Mapping
nodes:
    - id: switch
      from_smarthome:
          type: switch
      properties:
          - id: state
            compound_spec:
                members:
                    - mqtt_input:
                          topic: shellies/shelly1-C45BBE77B9C9/relay/0
                          mapping:
                              - from: "off"
                                to: { Bool: false }
                              - from: "on"
                                to: { Bool: true }
                      mqtt_output:
                          topic: shellies/shelly1-C45BBE77B9C9/relay/0/command
                          mapping:
                              - from: { Bool: true }
                                to: "on"
                              - from: { Bool: false }
                                to: "off"
```

**Explanation:**

- The `state` property is mapped to an MQTT topic (`shellies/shelly1-C45BBE77B9C9/relay/0`).
- **Input Mapping:**
    - If MQTT publishes `"off"`, the property value is set to `false`.
    - If MQTT publishes `"on"`, the property value is set to `true`.
- **Output Mapping:**
    - If the property is set to `true`, `"on"` is sent to MQTT.
    - If the property is set to `false`, `"off"` is sent to MQTT.

### Example: Mapping Button Press Actions

```yaml
id: button-test
name: Virtual Button
nodes:
    - id: button
      from_smarthome:
          type: button
          config:
              actions: ["press", "long-press"]
      properties:
          - id: action
            compound_spec:
                members:
                    - mqtt_input:
                          topic: zigbee2mqtt/button-1/action
                          mapping:
                              - from: "single"
                                to: { Enum: "press" }
                              - from: "hold"
                                to: { Enum: "long-press" }
```

**Explanation:**

- The `action` property listens to `zigbee2mqtt/button-1/action`.
- MQTT values `"single"` and `"hold"` are transformed into Homie-compatible `"press"` and `"long-press"`.

---

## Queries as Members

Instead of listing specific device IDs, you can use **queries** to match all nodes or properties of a certain type dynamically.

### Example: Aggregating All Motion Sensors

```yaml
id: motion-aggregator
name: Motion Detector Group
nodes:
    - id: motion
      from_smarthome:
          type: motion
      properties:
          - id: active
            datatype: boolean
            compound_spec:
                members:
                    - query:
                          node:
                              id: motion
                          property:
                              id: state
                aggregate_function: or
```

**Explanation:**

- Instead of listing motion sensors manually, the `query` automatically includes all `motion` nodes.
- The `"or"` function ensures that if _any_ motion sensor detects movement, the result is `true`.

---

## Using Mappings and Aggregation Together

You can use **both mappings and compound specifications** in the same property.

### Example: Motion-Based Light Control

```yaml
id: motion-light
name: Motion-Controlled Light
nodes:
    - id: light
      from_smarthome:
          type: switch
      properties:
          - id: state
            compound_spec:
                members:
                    - query:
                          node:
                              id: motion
                          property:
                              id: state-integer
                      mapping:
                          input:
                              - from: { Integer: 1 }
                                to: { Bool: true }
                              - from: { Integer: 0 }
                                to: { Bool: false }
                aggregate_function: or
```

**Explanation:**

- This property aggregates **all motion sensors with a integer field as state info**.
- Uses `"or"` to turn on the light if _any_ sensor detects movement.
- Applies a **mapping** to ensure motion `true/false` values map correctly.

# Advanced Configurations

This chapter covers additional configuration options that enhance the flexibility of virtual Homie devices. These include property options for MQTT state restoration, debounce timing for compound properties, and detailed duration formatting.

---

## Property Options

Each property can define additional behavior related to how values are retrieved and processed. The `property_opts` section allows for:

| Key                | Type    | Default | Description                                                               |
| ------------------ | ------- | ------- | ------------------------------------------------------------------------- |
| **read_from_mqtt** | boolean | `false` | If `true`, the property retrieves its initial state from MQTT on startup. |
| **read_timeout**   | string  | `none`  | Defines how long the device waits for an MQTT state before starting.      |

When `read_from_mqtt` is enabled, the device attempts to read the last known retained value of the property from MQTT when it starts. This ensures state persistence across restarts.
Please note that the device stays in init state until the value is read from mqtt or until the timeout.

### Example: Restoring Switch State on Startup

```yaml
id: virtual-switch
name: Restorable Switch
nodes:
    - id: switch
      from_smarthome:
          type: switch
          config:
              settable: true
      property_opts:
          read_from_mqtt: true
          read_timeout: 500ms
```

**Explanation:**

- `read_from_mqtt: true` ensures the switch retrieves its last known state from MQTT.
- `read_timeout: 500ms` sets a 500ms delay before considering the device ready, allowing time for the state to be fetched.

## Aggregation Debounce

When using **compound_spec**, the `aggregation_debounce` option ensures that rapid, consecutive changes in values do not immediately trigger updates. Instead, it waits for a period of time without any further changes before updating the final value.

| Key                      | Type   | Default | Description                                                                                 |
| ------------------------ | ------ | ------- | ------------------------------------------------------------------------------------------- |
| **aggregation_debounce** | string | `none`  | Ensures that a value is only updated after it remains unchanged for the specified duration. |

### How It Works

If multiple values arrive in quick succession, **aggregation_debounce waits** until no further changes occur for the specified time before applying the latest value. This prevents excessive updates when values fluctuate rapidly.

### Example: Stabilizing Average Brightness Calculation

```yaml
id: living-room-lights
name: Living Room Lights
nodes:
    - id: dimmer
      name: Dimmer Group
      from_smarthome:
          type: dimmer
      properties:
          - id: brightness
            datatype: integer
            compound_spec:
                members:
                    - floor-lamp-couch/dimmer/brightness
                    - ceiling-light/dimmer/brightness
                    - bookshelf-light/dimmer/brightness
                    - tv-backlight/dimmer/brightness
                    - wall-lamp/dimmer/brightness
                    - dining-table-lamp/dimmer/brightness
                aggregate_function: avg
                aggregation_debounce: 200ms
```

**Explanation:**

- The `brightness` property calculates the average brightness of multiple smart lights.
- Without `aggregation_debounce`, every small brightness change on any light would immediately trigger an update, creating **many unnecessary intermediate values**.
- With `aggregation_debounce: 200ms`, the system **waits 200 milliseconds** before applying the final brightness value.
- If another brightness update arrives within the 200ms window, the debounce timer **resets**, ensuring only the final settled brightness is applied.

This prevents excessive updates and ensures a **more stable brightness level**, reducing unnecessary MQTT messages.

---

## Duration Formatting

Several configuration options such as `read_timeout` and `aggregation_debounce` require **duration values** in a standardized format.

### Duration Format

| Format | Example | Description  |
| ------ | ------- | ------------ |
| `ms`   | `500ms` | Milliseconds |
| `s`    | `5s`    | Seconds      |
| `m`    | `2m`    | Minutes      |
| `d`    | `1d`    | Days         |

### Example: Multiple Timed Configurations

```yaml
id: timed-device
name: Timed Device Example
nodes:
    - id: sensor
      properties:
          - id: value
            datatype: boolean
            property_opts:
                read_from_mqtt: true
                read_timeout: 1s
            compound_spec:
                members:
                    - mqtt_input:
                          topic: test/sensor
                          mapping:
                              - from: "on"
                                to: { Bool: true }
                              - to: { Bool: false }
                aggregation_debounce: 500ms
```

**Explanation:**

- `read_timeout: 1s`: The device waits **1 second** for an initial MQTT value.
- `aggregation_debounce: 500ms`: Changes must **remain stable for 500ms** before being applied.

---

## Summary

- **Property Options (`property_opts`)** allow MQTT-based state restoration (`read_from_mqtt`).
- **Aggregation Debounce (`aggregation_debounce`)** ensures updates only occur after a stable period with no further changes.
- **Duration Values** (`ms`, `s`, `m`, `d`) standardize time-based configurations.

These options help fine-tune how virtual devices behave and interact with MQTT messages.

# Smart Home Integration

Virtual devices support **predefined node types** from smart home integrations, making it easier to define devices without manually specifying all nodes and properties. By using `from_smarthome`, you can generate nodes and properties automatically based on common device types such as switches, dimmers, thermostats, and sensors.

## Using `from_smarthome`

Instead of defining every node and property manually, you can specify a smart home device type, and the necessary structure will be created automatically.

### `from_smarthome` Fields

| Key        | Required | Type   | Description                                                                     |
| ---------- | -------- | ------ | ------------------------------------------------------------------------------- |
| **type**   | Yes      | string | The predefined smart home device type (e.g., `switch`, `dimmer`, `thermostat`). |
| **config** | No       | object | Configuration options specific to the chosen smart home type.                   |

The exact structure of each smart home node type, including its predefined properties and supported configuration options, can be found in the repository:  
[hc-homie5-smarthome](https://github.com/homie-homecontrol/hc-homie5-smarthome)

## Overriding Properties from `from_smarthome`

While `from_smarthome` generates a node with a default set of properties, you can **override** or **extend** those properties if needed.

### Why Override a Property?

- **Modify an existing property** (e.g., change `datatype`, `settable`, `retained` flags).
- **Add additional properties** that are not included by default.
- **Define compound properties** for virtual devices that aggregate multiple sources.

### How Overriding Works

- If a property is **not defined explicitly**, it will be generated from `from_smarthome`.
- If a property is **explicitly defined** under `properties`, it **replaces** the automatically generated one.

## The `datatype` Field

The `datatype` field defines the type of values a property can hold (e.g., `integer`, `boolean`, `string`). When using `from_smarthome`, the `datatype` is **predefined** for all standard properties.

However:

- If you define a **new property** under `properties`, **you must specify `datatype` explicitly**.
- If `from_smarthome` is **not used at all**, every property must include a `datatype`.

### Example: Adding a New Property to a Smart Home Node

```yaml
id: smart-plug
name: Smart Plug
nodes:
    - id: plug
      from_smarthome:
          type: switch
      properties:
          - id: power-usage
            datatype: float # Required, since this property is not predefined
            unit: "W"
```

**Explanation:**

- The `switch` node is generated automatically.
- A new property, `power_usage`, is added to report power consumption.
- Since `power_usage` is **not part of the predefined smart home switch definition**, `datatype: float` **must be set manually**.

## Supported Smart Home Device Types

The following device types can be used with `from_smarthome`:

| Type         | Description                                                              |
| ------------ | ------------------------------------------------------------------------ |
| `switch`     | A simple on/off switch.                                                  |
| `dimmer`     | A dimmable light that supports brightness control.                       |
| `colorlight` | A light that supports color control (RGB, HSV, or color temperature).    |
| `thermostat` | A heating or cooling device with temperature control and optional modes. |
| `motion`     | A motion sensor that detects movement.                                   |
| `contact`    | A sensor that detects whether a door or window is open or closed.        |
| `shutter`    | A motorized shutter or blind that can be opened, closed, and stopped.    |
| `weather`    | A weather sensor that reports temperature, humidity, and pressure.       |
| `button`     | A button device that supports different press actions.                   |
| `lightscene` | A predefined lighting scene that can be activated.                       |

Each type may support additional configuration options in the `config` field.

## Example: Simple Switch

This example defines a virtual switch that can be toggled on and off.

```yaml
id: living-room-switch
name: Living Room Switch
nodes:
    - id: switch
      from_smarthome:
          type: switch
          config:
              settable: true
      pass_through: true
```

**Explanation:**

- The `from_smarthome` section defines this node as a `switch`.
- The `config.settable: true` option allows remote control.
- `pass_through: true` ensures that when the switch is toggled, the new state is immediately published.

## Example: Overriding a Property from `from_smarthome`

By default, a `dimmer` device includes a `brightness` property. However, you can override it to define custom behavior.

```yaml
id: floor-lamp
name: Floor Lamp
nodes:
    - id: light
      from_smarthome:
          type: dimmer
      properties:
          - id: brightness
            datatype: integer
            format:
                IntegerRange:
                    min: 0
                    max: 100
            settable: true
            retained: true
            property_opts:
                read_from_mqtt: true
                read_timeout: 500ms
```

**Explanation:**

- Normally, `brightness` would be created automatically.
- By explicitly defining `brightness`, we customize:
    - **The format** (restricting the range from `0` to `100`).
    - **Settable & retained flags** (`true` in this case).
    - **State retrieval** (`read_from_mqtt` ensures the value is restored on restart).

## Example: Grouped Smart Home Devices with Property Override

You can create **virtual groups** that aggregate values from multiple real devices while overriding properties.

```yaml
id: kitchen-lights
name: Kitchen Lights
nodes:
    - id: dimmer
      from_smarthome:
          type: dimmer
      properties:
          - id: brightness
            datatype: integer
            compound_spec:
                members:
                    - ceiling-light/dimmer/brightness
                    - counter-light/dimmer/brightness
                    - table-light/dimmer/brightness
                aggregate_function: avg
                aggregation_debounce: 200ms
```

**Explanation:**

- This defines a **virtual dimmer** that represents multiple real light dimmers.
- The **brightness** property aggregates values from three light sources.
- The **average brightness** is calculated with a **200ms debounce** to avoid unnecessary updates.
