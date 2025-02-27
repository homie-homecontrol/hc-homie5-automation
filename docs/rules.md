# Rules

<small> back to [readme](../README.md)</small>

`hc-homie5-automation`'s primary function is automated actions for homie devices. Automations are defined in `rules` which are specified using `yaml` syntax.
A `rule` has the following structure:

```yaml
name: <rule-name>
triggers:
  - <trigger-config>
    while: # (Optional)
      - <condition-config>
      - ...
  - ...
actions:
  - <action-config>
  - ...
```

This document will describe all the 3 parts of a rule (name, trigger, action) in the following chapters.

### Example Rule

```yaml
name: evening-light
triggers:
    - schedule: "0 0 19 * * *"
      while:
          - subject: homie/living-room/motion-sensor/state
            condition:
                Bool: true
actions:
    - type: set
      target: homie/living-room/light/brightness
      value:
          Integer: 100
```

This rule turns on the living room light at full brightness at 7:00 PM if the motion sensor reports a motion at the trigger time.

## Name

The name attribute gives the rule a human-readable name, this is useful for debugging purposes.

## Triggers

The following trigger types are supported:

- `Subject triggered`: a property value is emitted (this is available for retained and non-retained properties)
- `Subject changed`: a property value changed (this is available only for retaied properties)
- `On set trigger`: for a virtual device property (when a message to ../set is published for a virtual property)
- `MQTT trigger`: when a value is published on any generic mqtt topic
- `Cron trigger`": define time intervals when a rule should be triggered
- `Timer trigger`: a defined timer fires
- `Solar event trigger`: specify a solar event e.g. sunset, when a rule should be triggered

### 1. Subject triggered

Activates when one or multiple specified property publishes a value matches a `trigger_value`.
This will work for `retained` and `non retained` properties.

Available config attributes:

| Attribute       | Type                                      | Description                                                                                                   |
| --------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `subjects`      | list of `subject` definitions             | defines all the properties subject to this this trigger                                                       |
| `queries`       | list of `query` definitions               | defines queries which will match all properties that are subject to this trigger                              |
| `trigger_value` | a `value-condition` of type `homie-value` | defines the value condition that needs to match the property value in order for the rule to trigger           |
| `while`         | list of `while-conditions`                | defines additional conditions that need to be true while the trigger is evaluated in order for it to triggger |

#### Example

```yaml
triggers:
    - subjects:
          - living-room-light-switch/button/action
      trigger_value:
          Enum: "press"
```

Triggeres when the living room light switch button is pressed.

### 2. Subject changed

Activates when one or multiple property values change. You can optional also restrict further to changes `from` a certain value or `to` a certain value.
This works only for `retained` properties.

Available config attributes:

| Attribute      | Type                                           | Description                                                                                                    |
| -------------- | ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `subjects`     | list of `subject` definitions                  | defines all the properties subject to this this trigger                                                        |
| `queries`      | list of `query` definitions                    | defines queries which will match all properties that are subject to this trigger                               |
| `changed`      | `changed` defintion (contains `from` and `to`) | defines the change condition (`from` and `to`) for the trigger to fire                                         |
| `changed.from` | a `value-condition` of type `homie-value`      | defines the value condition that needs to match the property's previous value in order for the rule to trigger |
| `changed.to`   | a `value-condition` of type `homie-value`      | defines the value condition that needs to match the property's current value in order for the rule to trigger  |
| `while`        | list of `while-conditions`                     | defines additional conditions that need to be true while the trigger is evaluated in order for it to triggger  |

Both `from` and `to` are optional.
|from|to|explanation|
|---|---|---|
| x | x | will trigger when a change from a certain value to a certain value occurs|
| | x | will trigger when a change from any value to a certain value occurs|
| x | | will trigger when a change from a certain value to any value occurs|
| | | will trigger whenever the value changes|

#### Example `from` -> `to`

```yaml
triggers:
    - subjects:
          - homie5-home/kitchen/light/state
      changed:
          from:
              Bool: false
          to:
              Bool: true
```

#### Example any -> `to`

```yaml
triggers:
    - subjects:
          - air-condition/mode/state
      changed:
          to: { Enum: "auto" }
```

This triggers when the aircondition mode changes to auto (e.g. either from `heating` or `cooling` mode before that)

#### Trigger on Any Change

```yaml
triggers:
    - subjects:
          - homie5-home/living-room/thermostat/temperature
      changed: {}
```

This triggers on any change in the temperature.

### 3. On Set Trigger

Activates when a value is published under the `../set` topic of a virtual device's property. This can be used to handle custom business logic for virtual devices.

Available config attributes:

| Attribute   | Type                                 | Description                                                                                                                                                                                                                                                                                                                                                  |
| ----------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `subjects`  | list of `subject` definitions        | defines all the properties subject to this this trigger                                                                                                                                                                                                                                                                                                      |
| `queries`   | list of `query` definitions          | defines queries which will match all properties that are subject to this trigger                                                                                                                                                                                                                                                                             |
| `set_value` | a `value-condition` of type `string` | defines the value condition that needs to match the property value in order for the rule to trigger. This will be the raw string value published for the `../set` topic. Beware that you will need to process and validate the input value by yourself in order to adhere to homie convention specs (there are functions in the lua runtime to handle this). |
| `while`     | list of `while-conditions`           | defines additional conditions that need to be true while the trigger is evaluated in order for it to triggger                                                                                                                                                                                                                                                |

#### Example

```yaml
triggers:
    - subjects:
          - virtual-light-device/switch/action
      set_value: "toggle"
```

### 4. MQTT Trigger

Triggered by specific MQTT messages.

Available config attributes:

| Attribute         | Type                                           | Description                                                                                                   |
| ----------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `topic`           | mqtt topic string                              | the mqtt topic to subscribe to                                                                                |
| `qos`             | qos `AtMostOnce`, `AtLeastOnce`, `ExactlyOnce` | the QoS level used to subscribe to the topic                                                                  |
| `trigger_value`   | a `value-condition` of type `string`           | defines the value condition that needs to match the received value in order for the rule to trigger           |
| `skip_retained`   | `boolean`                                      | do not trigger if the value was the retained value sent after initial subscription                            |
| `skip_duplicated` | `boolean`                                      | do not trigger if packet is marked as a duplicate                                                             |
| `check_qos`       | `boolean`                                      | check the qos of the published packet agains the specified qos for the subscription                           |
| `while`           | list of `while-conditions`                     | defines additional conditions that need to be true while the trigger is evaluated in order for it to triggger |

#### Example

```yaml
triggers:
    - mqtt: home/garage/door/status
      trigger_value: "open"
```

This triggers when the MQTT topic receives the payload `"open"`.

### 5. Schedule Trigger

Triggered at specified times using CRON syntax.

Available config attributes:

| Attribute  | Type                       | Description                                                                                                   |
| ---------- | -------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `schedule` | cron expression            | a cron expression that defines when the trigger fires.                                                        |
| `while`    | list of `while-conditions` | defines additional conditions that need to be true while the trigger is evaluated in order for it to triggger |

#### Cron expession example:

```
* * * * * * *
| | | | | | |
| | | | | | +-- Year
| | | | | +---- Day of week
| | | | +------ Month
| | | +-------- Day of month
| | +---------- Hour
| +------------ Minute
+-------------- Second
```

You can specify exact number matches or also fractions like:

```
# every minute at 10 o clock
0 * 10 * * * *

# every 5 minutes at 10 o clock
0 */5 10 * * * *
```

**Complex example**

```
# sec  min   hour   day of month   month   day of week   year
  "0   30   9,12,15     1,15       May-Aug  Mon,Wed,Fri  2018/2"
```

Breaking It Down:

| Field         | Value         | Meaning                               |
| ------------- | ------------- | ------------------------------------- |
| **Second**    | `0`           | Execute at second 0                   |
| **Minute**    | `30`          | At the 30th minute                    |
| **Hour**      | `9,12,15`     | At 09:00, 12:00, and 15:00            |
| **Day (DOM)** | `1,15`        | On the 1st and 15th of the month      |
| **Month**     | `May-Aug`     | From May to August                    |
| **Day (DOW)** | `Mon,Wed,Fri` | Only on Monday, Wednesday, and Friday |
| **Year**      | `2018/2`      | Every 2 years starting from 2018      |

When Does It Run?

- **At second 0 and minute 30** (e.g., `09:30`, `12:30`, `15:30`)
- **Only on the 1st and 15th of the month**
- **Only in the months May, June, July, and August**
- **Only on Monday, Wednesday, and Friday**
- **Starting in 2018, then every 2 years (2018, 2020, 2022, etc.)**

#### Example

```yaml
triggers:
    - schedule: "0 0 19 * * *"
```

This triggers every day at 7:00 PM.

### 6. Timer Trigger

Triggered when a predefined timer expires.

#### Example

```yaml
triggers:
    - timer_id: light-timer
```

### While conditions

Every trigger also has a while condition. A `while` is a list of expressions that need to evaluate to true in order for the trigger to actually fire.

#### Example

An example could be a trigger that acts on a motion detector and switches a light on but only while the value of a light sensor is low enough to signal darkness.

```yaml
triggers:
    - subjects:
          - motion-restroom/motion/state
      changed:
          from: { Bool: false }
          to: { Bool: true }
      while:
          - subject: lightsensor-restroom/lux/value
            condition:
                operator: "<"
                value: { Integer: 10 }
```

There are 2 different kinds of while conditions:

- Property while condition
- Time while condition

#### Property while condition

A propery while condition tests against the current value of a (retained) homie property.

Fields are:
| Attribute | Type | Description |
| ----------- | ----------------------------------------- | ------------------------------------------------------------------------------- |
| `subject` | a `subject` definition of the property | defines the property subject to this this condition |
| `condition` | a `value-condition` of type `homie-value` | defines the value condition that needs to match to true for the trigger to fire |

#### Time while condition

A time while condition tests against the time at the time of the event.

Fields are:
| Attribute | Type | Description |
| ----------- | ----------------------------------------- | --------------------------- |
| `after` | ISO 8601 time without timezone | Event trigger must happen after this time |
| `before` | ISO 8601 time without timezone | Event trigger must happen before this time |
|`weekdays` | a list of weekdays (3 character lowercase) | Event trigger must happen on one of these days (e.g. mon, tue, fri) |

## Actions

Actions defines tasks to perform when a rule triggers.

## Action Types

`hc-homie5-automation` supports a wide range of actions to be triggered for a rule. Each rule can define multiple actions to be executed.
The following actions types are supported:

| **Type**             | **Key**        | **Description**                                                               |
| -------------------- | -------------- | ----------------------------------------------------------------------------- |
| **Set Property**     | `set`          | Set a property to a certain value.                                            |
| **Map Set Property** | `map_set`      | Set a property to a value mapped from the triggering value                    |
| **Toggle Property**  | `toggle`       | Toggle a boolean property value.                                              |
| **Run Script**       | `run`          | Execute a Lua script.                                                         |
| **Send MQTT**        | `mqtt`         | Publish a message to an MQTT topic.                                           |
| **Start Timer**      | `timer`        | create a timer with a specified id, duration and optional repetition interval |
| **Cancel Timer**     | `cancel_timer` | Cancel a specific timer.                                                      |

### Action Examples

#### Set Property

```yaml
actions:
    - type: set
      target: homie5-home/living-room/light/brightness
      value:
          Integer: 50
```

#### Publish MQTT Message

```yaml
actions:
    - type: mqtt
      topic: home/alerts/security
      value:
          String: "Intruder detected!"
      qos: AtLeastOnce
      retained: true
```

#### Run Script

```yaml
actions:
    - type: run
      script: |-
          print("Starting custom script...")
          homie:set("ome/security-system/armed", true)
```

### Set Action

The Set action sets a specific value for a target subject.

Available fields are:

| Attribute | Type              | Description                                                                                   |
| --------- | ----------------- | --------------------------------------------------------------------------------------------- |
| `type`    | "set"             | defines the action type                                                                       |
| `target`  | `subject`         | the subject to set the value for (see chapter Subject in 'General Concepts')                  |
| `value`   | `HomieValue`      | The value to set as HomieValue(e.g., `{ Bool: true }`, `{ Integer: 123 }`, etc.)              |
| `timer`   | `TimerDefinition` | Every action can be delayed or repeated with a timer. See 'Timer Definition` for more details |

#### Example:

```yml
actions:
    - type: set
      target: device_id/node_id/property_id
      value:
          Bool: true
```

### MapSet Action

Maps the triggering subject's value using a predefined mapping.
The mapped value is then set for the target subject.

Available fields are:

| Attribute | Type              | Description                                                                                              |
| --------- | ----------------- | -------------------------------------------------------------------------------------------------------- |
| `type`    | "map_set"         | defines the action type                                                                                  |
| `target`  | `subject`         | the subject to set the value for (see chapter Subject in 'General Concepts')                             |
| `mapping` | `HomieValue`      | A list of mappings. There are 3 different `from` types to map from: `HomieValue`, `String`, `SolarPhase` |
| `timer`   | `TimerDefinition` | Every action can be delayed or repeated with a timer. See 'Timer Definition` for more details            |

#### Mapping field description:

The mapping field contains a list of mappings which are used to map the triggering value of the rule to a new value used to be set for the target subject. The mapping in the list that first matches in its `from` condition will be used to determine the output value.
A mapping is simply a combination of a (optional) `from` value condition and a `to` output value. If `from` is omitted the mapping is a so called "catch all" mapping, this is useful to map all not further defined input values to a defined output.

##### Example

```yaml
from:
    HomieValue:
        Bool: true
to:
    Bool: true
```

**`from` explanation**

Since a rule can be triggered by multiple different kind of triggers the mapping also needs to specify the source value type explicitly it is mapping`from`.
The following kind of triggers output a value:

- Subject triggered
- Subject changed
- On Set Trigger
- Timer Trigger
- Mqtt Trigger
- Solar Event trigger

In the table below you can find the type of the trigger output value and a example:

| Trigger             | Type         | Example                      |
| ------------------- | ------------ | ---------------------------- |
| subject triggered   | `HomieValue` | `HomieValue: { Integer: 5 }` |
| subject changed     | `HomieValue` | `HomieValue: { Bool: true }` |
| on set trigger      | `String`     | `String: "toggle"`           |
| timer trigger       | `String`     | `String: "my_timer_id"`      |
| mqtt trigger        | `String`     | `String: "off"`              |
| solar event trigger | `SolarPhase` | `SolarPhase: sunrise`        |

These types have to be named explicitly in the mapping, e.g.:

```yaml
from:
    HomieValue:
        Bool: true
```

or:

```yaml
from:
    String: "off"
```

Please also note that as mentioned earlier the `from` field is a value condition which means all matching options like described in the "Value Condition" chapter in "General Concepts" are available.
**Example for value condition**:

```yaml
from:
    operator: "<"
    value:
        HomieValue:
            Integer: 5
to:
    Bool: true
```

This will map all input values less than `5` of type integer to `true`.

**`to` explanation**

The `to` field is a plain `HomieValue` type and defines the mapping output.
**Example**:

```yaml
to:
    Float: 0.55
```

Please note that since `to` is always only a `HomieValue` the "HomieValue" type does not need to be explictly named like in the from field.

#### Full MapSet rule example:

```yml
name: light-switch
triggers:
    # trigger on a published value from the living room light switch.
    # It will send a 2001 Integer for the On button and a 2002 for the Off button
    - subjects:
          - livingroom/light-switch/button
      trigger_value:
          operator: matchAlways
    # in addition subscribe to a mqtt topic
    - topic: some_mqtt/topic
      trigger_value:
          operator: matchAlways
actions:
    - type: map_set
      target: group-4/switch/state
      mapping:
          # Map from button property (HomieValue) to Boolean
          - from:
                HomieValue:
                    Integer: 2002
            to:
                Bool: false
          # Map from mqtt topic (String) to Boolean ("off" -> false)
          - from:
                String: "off"
            to:
                Bool: false
          # Map anything else to true
          - to:
                Bool: true
```

This rule will map values from a light switch (2001 or 2002) or values published under "some_mqq/topic" ("off" or "on") to toggle a light gorup (group-4) on or off.
Mappings are defined only for 2002 and off input value to false (lights off) states. All other input values are mapped to true (lights on) using "catch all" mapping definitions.

### Toggle Action

The Toggle action toggles the state of a target subject (true -> false / false -> true)

- **Type**: `toggle`
- **Target**: The `boolean` property subject to toggle (e.g., `device_id/node_id/property_id`)

Example:

```yml
actions:
    - type: toggle
      target: device_id/node_id/property_id
```

### Run Action

The Run action runs a script with optional timer settings.

Available fields are:

| Attribute | Type              | Description                                                                                     |
| --------- | ----------------- | ----------------------------------------------------------------------------------------------- |
| `type`    | "run"             | Defines the action type                                                                         |
| `script`  | `lua source code` | The lua code to be executed when the rule is triggered. See "Lua Runtime" for more information. |
| `timer`   | `TimerDefinition` | Every action can be delayed or repeated with a timer. See 'Timer Definition` for more details   |

### Example 1:

```yml
actions:
    - type: run
      script: |
          local prop = "device_id/node_id/property_id"
          homie:set_command(prop, true)
```

This example simply sends a `../set` command to another property with HomieValue true.

### Example 2:

```yaml
name: window_open_notify
triggers:
    - queries:
          - node:
                id: contact
            property:
                id: state
                datatype: boolean
      changed: {}
actions:
    - type: run
      script: |-
          if event.value == true then
            -- implement actual code to e.g. send a push notification to the user
            print("==> Window open. Please close:", desc.name)
          end
      timer:
          id: window-timer
          duration: 600
          triggerbound: true
          cancelcondition:
              Bool: false
```

This example triggers for a value change of all properties with `id` "contact" of `type` "boolean" which are blow a node with `id` "contact" for all devices. (This is a query example for all window contact sensors)
When triggered a timer will be started with a 10 minute duration after which the lua script will be executed. (if the same property changes to HomieValue boolean false within the 10minutes duration the timer will be cancelled and no script will be executed)
The lua script will print a notification to remind the user to close the window again. (Intention here is of course not to just print out a log message but e.g. send a push notification to the users mobile phone in a real world implementation)

### Timer Action

The Timer action creates a new timer with specified settings.

Available fields are:

| Attribute | Type              | Description                                                                          | Required |
| --------- | ----------------- | ------------------------------------------------------------------------------------ | -------- |
| `type`    | "timer"           | Defines the action type                                                              | Yes      |
| `timer`   | `TimerDefinition` | Timer settings (e.g., `id`, `duration`, `repeat`, `cancelcondition`, `triggerbound`) | Yes      |

#### Timer Definition

The Timer Definition is used to specify the settings for a timer. The available fields are:

| Attribute         | Type        | Description                                                                                                                                      | Required |
| ----------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | -------- |
| `id`              | `string`    | Unique identifier for the timer. This is used to reference the timer in other actions.                                                           | Yes      |
| `duration`        | `duration`  | The length of time the timer will run for, e.g. (10s, 5m, 1d)                                                                                    | Yes      |
| `repeat`          | `duration`  | The interval at which the timer will repeat, e.g. (10s, 5m, 1d) - if omitted the timer will only run once                                        | No       |
| `triggerbound`    | `boolean`   | If `true`, the timer id will be generated by appending the property id of the trigger event to the specified timer id.                           | No       |
| `cancelcondition` | `condition` | A condition that, if met, will cancel the timer. The condition can be a boolean value, a comparison of two values, or a more complex expression. | No       |

#### Triggerbound

The `triggerbound` field is used to determine how the timer id is generated when a timer is triggered by a rule. If `triggerbound` is `true`, the timer id will be generated by appending the property subject path (domain/device/node/property) of the trigger event to the specified timer id, separated by a hyphen. This allows you to create multiple timers with the same id, but triggered by different properties.

One of the key benefits of the `triggerbound` field is that it enables you to create a single rule that can handle multiple properties, and each property will have its own separate timer context. This means you can create a rule that matches multiple properties via a query, and the timer will be created separately for each property that triggers the rule. For example, if you have a rule that matches all window contact sensors in the house, you can use the `triggerbound` field to create a separate timer for each window contact sensor. This way, if one window is opened, a timer will be started for that specific window, and if another window is opened, a separate timer will be started for that window.

This feature is particularly useful when you have a large number of devices or properties that need to be handled in a similar way, and you don't want to create a separate rule for each one. By using the `triggerbound` field, you can simplify your configuration and make it more efficient.

#### Example 1:

```yml
actions:
    - type: timer
      timer:
          id: my_timer
          duration: 10s
          repeat: 5s
```

This example creates a new timer with the id "my_timer", a duration of 10 seconds, and a repeat interval of 5 seconds.

#### Example 2:

```yml
name: window_open_notify
triggers:
    - queries:
          - node:
                id: contact
            property:
                id: state
                datatype: boolean
      changed: {}
actions:
    - type: run
      script: |-
          if event.value == true then
            -- implement actual code to e.g. send a push notification to the user
            print("==> Window open. Please close:", desc.name)
          end
      timer:
          id: window-timer
          duration: 10m
          triggerbound: true
          cancelcondition:
              Bool: false
```

This example creates a rule that triggers when any window contact sensor in the house changes to `true`. The `triggerbound` field is set to `true`, which means that the timer id will be generated by appending the property id of the trigger event to the specified timer id. The `cancelcondition` field is used to cancel the timer if the window contact sensor changes to `false` within the 10-minute duration. This way, each window contact sensor will have its own separate timer, and the rule can handle multiple windows without requiring a separate rule for each one.

### CancelTimer Action

The CancelTimer action cancels a running timer by its ID.

Available fields are:

| Attribute  | Type           | Description                                | Required |
| ---------- | -------------- | ------------------------------------------ | -------- |
| `type`     | "cancel_timer" | Defines the action type                    | Yes      |
| `timer_id` | `TimerID`      | The timer id of the timer to be cancelled. | Yes      |

Example:

```yml
actions:
    - type: cancel_timer
      timer_id: my_timer
```

### Mqtt Action

The Mqtt action publishes a message to an MQTT topic. This action is specifically designed for non-Homie convention purposes, allowing you to publish custom messages to MQTT topics that do not adhere to the standard Homie protocol.

Available fields are:

| Attribute | Type      | Description                                                                         | Required |
| --------- | --------- | ----------------------------------------------------------------------------------- | -------- |
| `type`    | "mqtt"    | Defines the action type                                                             | Yes      |
| `topic`   | `string`  | The MQTT topic to publish to (e.g., `homie/device_id/node_id/property_id`)          | Yes      |
| `value`   | `string`  | The value to publish (e.g., `true`, `false`, `123`, etc.)                           | Yes      |
| `qos`     | `QoS`     | The quality of service (e.g., `AtMostOnce`, `AtLeastOnce`, `ExactlyOnce` (default)) | No       |
| `retain`  | `boolean` | Whether to retain the message (e.g., `true`, `false` (default))                     | No       |

The `qos` field specifies the quality of service for the MQTT message. The possible values are:

- `AtMostOnce`: Fire and forget
- `AtLeastOnce`: Guaranteed delivery
- `ExactlyOnce`: Guaranteed delivery with acknowledgement

The `retain` field specifies whether the message should be retained on the MQTT broker. If `true`, the message will be stored on the broker and delivered to new subscribers.

This action enables integration with other systems or devices that do not follow the Homie protocol, or publishing custom messages to MQTT topics for specific use cases.

#### Example 1:

```yml
actions:
    - type: mqtt
      topic: home-assistant/notifications
      value: "The door is open"
      qos: AtMostOnce
```

This example publishes a notification message to an MQTT topic, which can be used to trigger custom actions or notifications in other systems.

#### Example 2:

```yml
actions:
    - type: mqtt
      topic: custom-scenes/scene-1
      value: "on"
      qos: ExactlyOnce
      retain: true
```

This example publishes a custom scene control message to an MQTT topic, which can be used to control custom scenes or devices that do not follow the standard Homie protocol.
