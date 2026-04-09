# Lua Runtime

<small> back to [readme](../README.md)</small>

## Overview

The Lua runtime is the escape hatch for automation logic that is hard to express with plain `set`, `map_set`, or `toggle` actions.
Use it when you need branching logic, external API calls, temporary state, or custom orchestration across multiple devices.

A `run` action executes your script inside an embedded Lua 5.4 VM.
Each execution gets a fresh VM instance, which keeps runs isolated and avoids accidental cross-run side effects.
If you want state to survive between executions, use `value_store`.

## Execution Model

Think of a script execution as a short-lived function call with injected helpers.

1. A rule trigger fires.
2. The runtime creates a new Lua VM.
3. Globals (`homie`, `virtual_device`, `timers`, `value_store`, `utils`, `event`) are injected.
4. Your script runs.
5. VM is dropped.

This model makes script behavior deterministic and easier to reason about, especially under frequent events.

## Lua Modules (`require`)

You can split shared logic into reusable modules and import them with `require(...)`.
Modules are loaded from the backend configured via `HCACTL_LUA_MODULE_CONFIG`.

- Module name = file name without `.lua`.
- Example: `/service/lua/mymod.lua` -> `require("mymod")`.
- Module namespace is flat. Subfolders can be watched by the backend, but only the basename is used for lookup.

Example:

```lua
local m = require("mymod")
m.hello()
```

## Runtime Globals

The runtime injects six globals. Together, they form a small API surface:

- `homie` for real-device interactions
- `virtual_device` for virtual-device interactions
- `timers` for delayed/repeating behavior
- `value_store` for persistence
- `utils` for helper functions (HTTP, JSON, MQTT publish, sleep)
- `event` for trigger context

### `homie`

Use `homie` when you want to interact with discovered real devices (or read their metadata).

Methods:

- `set_command(property_ref, value)`
- `get_value(property_ref) -> value | nil`
- `get_property_description(property_ref) -> table | nil`
- `get_device_description(device_ref_or_property_ref) -> table | nil`
- `get_device_alerts(device_ref) -> table | nil`

`set_command` publishes a standard Homie `/set` command; it does not directly mutate values in memory.

### `virtual_device`

Use `virtual_device` to control or inspect virtual-device properties and alerts.

Methods:

- `set_str_value(property_ref, raw_string)`
- `set_value(property_ref, value)`
- `set_command(property_ref, value)`
- `get_value(property_ref) -> value | nil`
- `get_property_description(property_ref) -> table | nil`
- `get_device_description(device_ref_or_property_ref) -> table | nil`
- `set_device_alert(device_ref_or_property_ref, alert_id, alert_text) -> bool`
- `clear_device_alert(device_ref_or_property_ref, alert_id) -> bool`
- `get_device_alerts(device_ref) -> table | nil`

Important behavior differences:

- `set_value(...)` writes a virtual property value directly.
- `set_command(...)` simulates the virtual property's set-command path.
- `set_str_value(...)` parses a raw string against the property description before setting.

### `timers`

Use timers for delayed actions (for example, auto-off) and for recurring script workflows.

Methods:

- `create(id, duration_seconds, repeat_seconds_optional)`
- `cancel(id)`

Examples:

```lua
-- one-shot timer: fire once after 10 minutes
timers:create("window-reminder", 600)

-- repeating timer: first fire after 1 second, then every 1 second
timers:create("blink", 1, 1)

timers:cancel("blink")
```

### `value_store`

`value_store` is the persistent memory of your scripts.
Use it for counters, last-seen timestamps, temporary locks, or small state machines.

Methods:

- `set(key, value)`
- `get(key) -> value | nil`
- `delete(key)`

Notes:

- Keys are normalized internally for backend compatibility.
- Values are stored as JSON-compatible data.

### `utils`

`utils` groups side-effect and conversion helpers so your script code stays compact.

Methods:

- `sleep(milliseconds)`
- `from_json(json_string) -> lua_value`
- `to_json(table) -> json_string`
- `http_get(url) -> response`
- `http_post(url, body_string) -> response`
- `http_post_json(url, table) -> response`
- `http_post_form(url, table) -> response`
- `mqtt_publish(topic, payload, qos_optional, retained_optional)`

`response` (HTTP body wrapper) methods:

- `response:text() -> string`
- `response:json() -> table`

`mqtt_publish` QoS values:

- `0` = AtMostOnce (default)
- `1` = AtLeastOnce
- `2` = ExactlyOnce

### `event`

`event` is the trigger context for the current script run.
It is read-only and describes why the script is currently executing.

Field overview:

| Field | Type | Description |
| --- | --- | --- |
| `type` | `string` | Trigger type identifier (see values below). |
| `prop` | `LuaPropertyRef \| nil` | Trigger property for property/on-set triggers. |
| `on_set_value` | `string \| nil` | Raw set payload for on-set triggers. |
| `value` | `varies` | Trigger payload value (depends on trigger type). |
| `from_value` | `HomieValue-mapped \| nil` | Previous value for property-changed triggers. |
| `timer_id` | `string \| nil` | Timer ID for timer triggers. |
| `mqtt_topic` | `string \| nil` | MQTT topic for MQTT triggers. |
| `mqtt_retain` | `boolean \| nil` | MQTT retain flag for MQTT triggers. |

Current `event.type` values (as emitted by implementation):

- `changed`
- `trigered` (spelling in current implementation)
- `timer`
- `cron`
- `mqtt`
- `onset`
- `solar`

`event.value` by trigger type:

- Property changed/triggered: mapped Homie value
- MQTT: payload string
- On-set: payload string
- Timer/Cron/Solar: `nil`

## Property References

A property reference uses slash-separated notation:

```lua
-- without domain (configured default domain is used)
"device_id/node_id/property_id"

-- with explicit domain
"homie_domain/device_id/node_id/property_id"
```

Most methods also accept typed refs from `event.prop` (`LuaPropertyRef`) where applicable.

## HomieValue <-> Lua Mapping

The runtime transparently converts between Homie values and Lua values.
Understanding this mapping helps avoid subtle bugs when comparing values or publishing data.

### HomieValue -> Lua

| HomieValue | Lua value |
| --- | --- |
| `Empty` | `nil` |
| `String` | `string` |
| `Integer` | `integer` |
| `Float` | `number` |
| `Bool` | `boolean` |
| `Enum` | `string` |
| `Color` | `string` |
| `DateTime` | `string` (RFC3339) |
| `Duration` | `string` (`PT...S`) |
| `JSON` | `string` (JSON encoded) |

### Lua -> HomieValue

| Lua value | HomieValue |
| --- | --- |
| `nil` | `Empty` |
| `string` | Parsed as `Color`, `DateTime`, or `Duration` if format matches; otherwise `String` |
| `integer` | `Integer` |
| `number` | `Float` |
| `boolean` | `Bool` |
| `table` | `JSON` |

Notes:

- String values are not auto-converted to `Enum`.
- If you need descriptor-based parsing from a raw string for virtual properties, use `virtual_device:set_str_value(...)`.
- If you receive JSON as string (for example from `event.value` on MQTT triggers), use `utils:from_json(...)`.

## Practical Patterns

- Use `event.type` as your top-level branch condition when one script handles multiple trigger kinds.
- Use `timers:create(...)` for delayed behavior instead of blocking with long sleeps.
- Keep long-lived state in `value_store`, not in Lua globals.
- Prefer `utils:http_post_json(...)` and `utils:from_json(...)` over manual JSON string handling.

## Example Rule

```yaml
---
name: test-device-toggle
triggers:
  - properties:
      - virtual-test-device/switch/action
    set_value: "toggle"
actions:
  - type: run
    script: |-
      local prop = "virtual-test-device/switch/state"
      local current = virtual_device:get_value(prop)
      virtual_device:set_value(prop, not current)
      print("set payload:", event.on_set_value)
```

This rule toggles the virtual switch state whenever `toggle` is received on the corresponding `/set` topic.
