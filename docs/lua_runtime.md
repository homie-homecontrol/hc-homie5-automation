# Lua Runtime Documentation

<small> back to [readme](../README.md)</small>

## Introduction

The Lua Runtime is a powerful tool for automating and controlling Homie devices. It provides a range of commands and functions that can be used to interact with devices, set values, and trigger actions. This document provides an overview of the available commands and how to use them.

## Commands

The following commands are available in the Lua Runtime:

- `homie`: Provides functions for interacting with Homie devices.
- `virtual_device`: Provides functions for interacting with virtual devices.
- `timers`: Provides functions for working with timers.
- `value_store`: Provides functions for storing and retrieving values.
- `utils`: Provides utility functions for tasks such as sleeping and making HTTP requests.
- `event`: Provides information about the current event that triggered the script.

### Homie Commands

The `homie` command provides functions for interacting with Homie devices. The available functions are:

- `set_command(subject, value)`: Sets the value of a Homie property.
- `get_value(subject)`: Gets the current value of a Homie property.
- `get_property_description(subject)`: Gets the description of a Homie property.
- `get_device_description(device_id)`: Gets the description of a Homie device.

#### Example

```lua
-- Set the value of a Homie property
homie:set_command("device_id/node_id/property_id", true)

-- Get the current value of a Homie property
local value = homie:get_value("device_id/node_id/property_id")

-- Get the description of a Homie property
local description = homie:get_property_description("device_id/node_id/property_id")

-- Get the description of a Homie device
local device_description = homie:get_device_description("device_id")
```

### Virtual Device Commands

The `virtual_device` command provides functions interacting for with virtual devices. The available functions are:

- `set_str_value(subject, value)`: Sets the string value of a virtual device property.
- `set_value(subject, value)`: Sets the value of a virtual device property.
- `set_command(subject, value)`: Sets the value of a virtual device property.
- `get_value(subject)`: Gets the current value of a virtual device property.
- `get_property_description(subject)`: Gets the description of a virtual device property.
- `get_device_description(device_id)`: Gets the description of a virtual device.
- `set_device_alert(device_id, alert_id, alert)`: Sets an alert on a virtual device.
- `clear_device_alert(device_id, alert_id)`: Clears an alert on a virtual device.
- `get_device_alerts(device_id)`: Gets the alerts of a virtual device.

#### Example

```lua
-- Set the string value of a virtual device property
virtual_device:set_str_value("device_id/node_id/property_id", "value")

-- Set the value of a virtual device property
virtual_device:set_value("device_id/node_id/property_id", true)

-- Set the value of a virtual device property
virtual_device:set_command("device_id/node_id/property_id", true)

-- Get the current value of a virtual device property
local value = virtual_device:get_value("device_id/node_id/property_id")

-- Get the description of a virtual device property
local description = virtual_device:get_property_description("device_id/node_id/property_id")

-- Get the description of a virtual device
local device_description = virtual_device:get_device_description("device_id")

-- Set an alert on a virtual device
virtual_device:set_device_alert("device_id", "alert_id", "alert")

-- Clear an alert on a virtual device
virtual_device:clear_device_alert("device_id", "alert_id")

-- Get the alerts of a virtual device
local alerts = virtual_device:get_device_alerts("device_id")
```

### Timer Commands

The `timers` command provides functions for working with timers. The available functions are:

- `create(id, duration, repeat)`: Creates a new timer.
- `cancel(id)`: Cancels a timer.

#### Example

```lua
-- Create a new timer
timers:create("timer_id", 10, 5)

-- Cancel a timer
timers:cancel("timer_id")
```

### Value Store Commands

The `value_store` command provides functions for storing and retrieving values. The available functions are:

- `set(key, value)`: Sets a value in the store.
- `get(key)`: Gets a value from the store.
- `delete(key)`: Deletes a value from the store.

#### Example

```lua
-- Set a value in the store
value_store:set("key", "value")

-- Get a value from the store
local value = value_store:get("key")

-- Delete a value from the store
value_store:delete("key")
```

### Utils Commands

The `utils` command provides utility functions for tasks such as sleeping and making HTTP requests. The available functions are:

- `sleep(time)`: Sleeps for a specified amount of time.
- `http_get(uri)`: Makes an HTTP GET request to a specified URI.
- `http_post(uri, data)`: Makes an HTTP POST request to a specified URI with specified data.
- `http_post_json(uri, data)`: Makes an HTTP POST request to a specified URI with specified JSON data.
- `http_post_form(uri, data)`: Makes an HTTP POST request to a specified URI with specified form data.

#### Example

```lua
-- Sleep for 5 seconds
utils:sleep(5000)

-- Make an HTTP GET request
local response = utils:http_get("https://example.com")

-- Make an HTTP POST request
local response = utils:http_post("https://example.com", "data")

-- Make an HTTP POST request with JSON data
local response = utils:http_post_json("https://example.com", { key = "value" })

-- Make an HTTP POST request with form data
local response = utils:http_post_form("https://example.com", { key = "value" })
```

### Event Commands

The `event` command provides information about the current event that triggered the script. The available fields are:

- `type`: Gets the type of the event.
- `prop`: Gets the property that triggered the event.
- `on_set_value`: Gets the value that was set for a virtual device's property
- `value`: Gets the current value of the property.
- `from_value`: Gets the previous value of the property.
- `timer_id`: Gets the ID of the timer that triggered the event.

#### Example

```lua
-- Get the type of the event
local event_type = event.type

-- Get the property that triggered the event
local prop = event.prop

-- Get the value that was set
local on_set_value = event.on_set_value

-- Get the current value of the property
local value = event.value

-- Get the previous value of the property
local from_value = event.from_value

-- Get the ID of the timer that triggered the event
local timer_id = event.timer_id
```

## Subjects

A subject is a reference to a Homie property. It can be specified in two ways:

- Using a topic-like notation: `device_id/node_id/property_id`
- Using an object notation:

```lua
{
  homie_domain = "homie_domain",
  device_id = "device_id",
  node_id = "node_id",
  property_id = "property_id"
}
```

## HomieValue to Native Lua Type Mapping

The HomieValue type is used to represent values in the Homie protocol. When working with the Lua Runtime, it's essential to understand how HomieValue maps to native Lua types and back.

### HomieValue to Lua Type Mapping

The following table shows how HomieValue types map to native Lua types:

| HomieValue Type | Lua Type                   | Example                                                            |
| --------------- | -------------------------- | ------------------------------------------------------------------ |
| Empty           | `nil`                      | `{ Nil: true }` -> `nil`                                           |
| String          | `string`                   | `{ String: "hello" }` -> `"hello"`                                 |
| Integer         | `number` (integer)         | `{ Integer: 42 }` -> `42`                                          |
| Float           | `number` (float)           | `{ Float: 3.14 }` -> `3.14`                                        |
| Boolean         | `boolean`                  | `{ Bool: true }` -> `true`                                         |
| Enum            | `string`                   | `{ Enum: "toggle" }` -> `"toggle"`                                 |
| Color           | `string`                   | `{ Color: "rgb,100,120,110" }` -> `"rgb,100,120,110"`              |
| DateTime        | `string` (ISO 8601 format) | `{ DateTime: "2024-10-08T10:15:30Z" }` -> `"2024-10-08T10:15:30Z"` |
| Duration        | `string` (ISO 8601 format) | `{ Duration: "PT12H5M46S" }` -> `"PT12H5M46S"`                     |
| JSON            | `table`                    | `{ JSON: "{\"hello\": \"world\"}" }` -> `{ hello = "world" }`      |

### Lua Type to HomieValue Mapping

The following table shows how native Lua types map to HomieValue types:

| Lua Type           | HomieValue Type | Example                                                            |
| ------------------ | --------------- | ------------------------------------------------------------------ |
| `nil`              | Empty           | `nil` -> `{ Nil: true }`                                           |
| `string`           | String          | `"hello"` -> `{ String: "hello" }`                                 |
| `string`           | Enum            | `"toggle"` -> `{ Enum: "toggle" }`                                 |
| `string`           | Color           | `"rgb,100,120,110"` -> `{ Color: "rgb,100,120,110" }`              |
| `string`           | DateTime        | `"2024-10-08T10:15:30Z"` -> `{ DateTime: "2024-10-08T10:15:30Z" }` |
| `string`           | Duration        | `"PT12H5M46S"` -> `{ Duration: "PT12H5M46S" }`                     |
| `number` (integer) | Integer         | `42` -> `{ Integer: 42 }`                                          |
| `number` (float)   | Float           | `3.14` -> `{ Float: 3.14 }`                                        |
| `boolean`          | Boolean         | `true` -> `{ Bool: true }`                                         |
| `table`            | JSON            | `{ hello = "world" }` -> `{ JSON: "{\"hello\": \"world\"}" }`      |

Note that when converting a Lua `string` to a HomieValue, it will be parsed as a HomieValue type based on its format. For example, if the string is in the format "rgb,100,120,110", it will be converted to a `Color` HomieValue. If the string is in the format "2024-10-08T10:15:30Z", it will be converted to a `DateTime` HomieValue.

## Current Rule Example

Here's an example of a current rule file that uses the Lua Runtime:

```yml
---
name: test-device-toggle
triggers:
    - subjects:
          - virtual-test-device/switch/action
      set_value: "toggle"
actions:
    - type: run
      script: |-
          local prop = "virtual-test-device/switch/state"
          virtual_device:set_value(prop, not virtual_device:get_value(prop))
          print("VALUE: ", event.on_set_value)
```

This rule uses the Lua Runtime to implement the toggle action for a virtual device. It reads and the virtual devices switch state value and inverts it before applying it again.

Please note that this is just a basic example and you can use the Lua Runtime to perform more complex tasks.
