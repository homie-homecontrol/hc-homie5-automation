# Setup and configuration

<small> back to [readme](../README.md)</small>

Recommended method of installation and usage is `Docker`.

```bash
 docker pull ghcr.io/homie-homecontrol/hc-homie5-automation:latest

```

## Container setup

Settings are passed via environment variables. The environment variables are prefixed with `HCACTL_`. Below is a list of all supported environment variables, their purpose, and valid example values.

### Homie MQTT Configuration

| Variable                 | Purpose                                                           | Default                             | Example                      |
| ------------------------ | ----------------------------------------------------------------- | ----------------------------------- | ---------------------------- |
| `HCACTL_HOMIE_HOST`      | MQTT broker hostname                                              | `localhost`                         | `"mqtt.example.com"`         |
| `HCACTL_HOMIE_PORT`      | MQTT broker port                                                  | `1883`                              | `"1883"`                     |
| `HCACTL_HOMIE_USERNAME`  | Username for MQTT authentication                                  | (empty)                             | `"mqttuser"`                 |
| `HCACTL_HOMIE_PASSWORD`  | Password for MQTT authentication                                  | (empty)                             | `"securepassword"`           |
| `HCACTL_HOMIE_CLIENT_ID` | MQTT client ID used for identification (keep this <16 characters) | Randomly generated                  | `"hcactl-mqtt-client"`       |
| `HCACTL_HOMIE_DOMAIN`    | Homie MQTT domain under which the automation controller operates. | `homie`                             | `"homie"`                    |
| `HCACTL_HOMIE_CTRL_ID`   | Sets the homie id for the controller device                       | `hc-homie5-automation-ctrl`         | `"my-custom-controller"`     |
| `HCACTL_HOMIE_CTRL_NAME` | Defines the controller device's human-readable name               | `Homecontrol Automation Controller` | `"My Smart Home Controller"` |

### Configuration Variables

| Variable                        | Purpose                                           | Valid Values                                                                                       | Default                       | Example                                         |
| ------------------------------- | ------------------------------------------------- | -------------------------------------------------------------------------------------------------- | ----------------------------- | ----------------------------------------------- |
| `HCACTL_RULES_CONFIG`           | Specifies the backend for rule storage            | `file:/path/to/rules`,<br/>`mqtt:some/topic`,<br /> `kubernetes:config-name[,namespace]`           | file:/service/rules           | `"mqtt:hcactl/rules"`                           |
| `HCACTL_VIRTUAL_DEVICES_CONFIG` | Specifies the backend for virtual devices storage | `file:/path/to/virtual_devices`,<br />`mqtt:some/topic`,<br />`kubernetes:config-name[,namespace]` | file:/service/virtual_devices | `"kubernetes:hcactl-virtual-devices,smarthome"` |
| `HCACTL_META_CONFIG`            | Specifies the backend for manual metadata overlays | `file:/path/to/meta`,<br/>`mqtt:some/topic`,<br /> `kubernetes:config-name[,namespace]`            | file:/service/meta            | `"file:/data/meta"`                             |
| `HCACTL_LUA_MODULE_CONFIG`      | Specifies the backend for lua module storage      | `file:/path/to/lua`,<br/>`mqtt:some/topic`,<br /> `kubernetes:config-name[,namespace]`             | file:/service/lua             | `"file:/data/lua_scripts"`                      |
| `HCACTL_VALUE_STORE_CONFIG`     | Defines how values are stored                     | `inmemory`,<br />`sqlite:/path/to/database.db`,<br />`kubernetes:secret\|configmap,name[,namespace]` | inmemory                      | `"sqlite:/service/values.db"`                   |
| `HCACTL_LOCATION`               | Defines the geographical location                 | `<latitude>,<longitude>,<elevation>`                                                               | `0,0,0`                       | `"48.1351,11.5820,519"`                         |
| `HCACTL_LUA_SCRIPT_TIMEOUT`     | Global execution budget for Lua `run` script actions (a `script_timeout` on the `run` action overrides it) | duration string: `<integer>` + `ms`/`s`/`m`/`h`/`d`                                                | `5s`                          | `"10s"`                                         |
| `HCACTL_LUA_MEMORY_LIMIT_MB`   | Memory limit for each Lua script VM; a script that allocates more fails with a `not enough memory` error. `0` disables the limit | whole mebibytes | `64` | `"128"` |
| `HCACTL_RULE_MAX_FIRINGS_PER_SEC` | Maximum firings per second of a single rule. Further firings within the same second are skipped and logged with one warning per second naming the rule; this stops rules that trigger themselves (see [rules.md](rules.md#self-triggering-rules)). `0` disables the limit | non-negative integer | `20` | `"50"` |
| `HCACTL_CONTROLLER_CHILDREN_DEBOUNCE` | Debounce window for controller children updates. Bursts of virtual device creations/removals republish the controller description only once per quiet period. `0` disables debouncing (publish immediately on every change) | duration string: `<integer>` + `ms`/`s`/`m`/`h`/`d`                                                | `200ms`                       | `"500ms"`                                       |

> Note:
> - For direct binary runs, application defaults are relative paths like `file:./rules`, `file:./virtual_devices`, `file:./meta`, and `file:./lua`.
> - Container deployments usually set explicit absolute paths via environment variables.

### Logging

| Variable            | Purpose                                                                                         | Valid Values                                       | Default | Example                                          |
| ------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------- | ------- | ------------------------------------------------ |
| `HCACTL_LOG_LEVEL`  | Log level of the automation controller; all other crates log at `warn`                          | `off`, `error`, `warn`, `info`, `debug`, `trace`   | `info`  | `"debug"`                                        |
| `RUST_LOG`          | Full log filter; overrides `HCACTL_LOG_LEVEL`                                                   | `tracing` EnvFilter directives                     | (unset) | `"warn,hc_homie5_automation=debug,hc_homie5=debug"` |
| `HCACTL_LOG_FORMAT` | Output format                                                                                   | `text`, `json` (one object per line)               | `text`  | `"json"`                                         |
| `NO_COLOR`          | Disables colored output (color is only used when stdout is a terminal)                          | any value                                          | (unset) | `"1"`                                            |

At `info` the log shows what the controller did: rules loaded and fired, commands sent, messages
published, HTTP requests of Lua scripts, devices added or removed, alerts, and connection changes.
`debug` adds the reasons (timers, trigger decisions, set commands received, Lua `print` output).

### Rules, Virtual Devices, Meta and Lua Modules config backends

For rules, virtual devices, manual metadata overlays and lua modules `hc-homie5-automation` supports multiple backends as input.

- **File:**

    Will read rule specifications, virtual device specifications, manual metadata overlays or lua modules from files inside the specified folder.

    - Example: `file:/path/to/config`

- **MQTT:**

    Will read rule specifications, virtual device specifications, manual metadata overlays or lua modules from all subtopics with the specified topic. Each topic is treated like a file in the file system. Ensure valid yaml data or lua code is published under these topics.

    - Example: `mqtt:some/topic`

- **Kubernetes:**

    Will read rule specifications, virtual device specifications, manual metadata overlays or lua modules from a kubernetes `ConfigMap`. Each data field in the `ConfigMap` is treated like a file in the file system. Ensure valid yaml data or lua code is stored under these fields.

    - Example: `kubernetes:config-name[,namespace]`
    - If no namespace is provided, the `default` namespace is used.

All three backend types support hot reload, this means your changes are immediately applied without the need of a restart.

### Manual metadata overlay file format

Manual metadata overlay documents (loaded through `HCACTL_META_CONFIG`) are defined in YAML:

```yaml
provider:
  id: hcactl-manual-meta
  schema: 1
  title: Manual metadata provider
  description: Optional description
devices:
  my-device-id:
    annotations:
      room: kitchen
      tags:
        - light
        - zigbee
    nodes:
      switch:
        properties:
          state:
            annotations:
              category: main-light
```

- One YAML document equals one meta provider.
- A file can contain multiple providers using YAML document separators (`---`).
- Duplicate active `provider.id` entries are rejected and logged.

### Value Store Config Details

For persistent storage (to be used in rules) `hc-homie5-automation` supports a simple key value store. This store supports multiple backends:

- **In-Memory:**

    Stores values in memory (default). Warning: All your data will be lost after a restart!

    - Example: `inmemory`

- **SQLite:**

    Stores values in a SQLite database file.

    - Example: `sqlite:/path/to/database.db`

- **Kubernetes:**

    Stores values in a Kubernetes resource.

    - Example: `kubernetes:secret|configmap,name[,namespace]`
    - If no namespace is provided, the `default` namespace is used.

## Docker compose example

For a quick starting example see the `deploy/docker` folder in this repo. It gives a simple example on how to run `hc-homie5-automation`.

```yaml
version: "3"
services:
    mqtt:
        image: eclipse-mosquitto:latest
        restart: "always"
        deploy:
            resources:
                limits:
                    memory: 25M
        hostname: mqtt
        dns:
            - 8.8.8.8
        ports:
            - "1883:1883"
            - "19001:19001"
        volumes:
            - ./mqtt/config/mosquitto.conf:/mosquitto/config/mosquitto.conf
            - ./mqtt/data:/mosquitto/data
            - ./mqtt/log:/mosquitto/log

    hc-homie5-automation:
        image: ghcr.io/homie-homecontrol/hc-homie5-automation:latest
        restart: "always"
        depends_on:
            - mqtt
        dns:
            - 192.168.1.101
        deploy:
            resources:
                limits:
                    memory: 20M
        environment:
            HCACTL_HOMIE_HOST: "mqtt"
            HCACTL_HOMIE_DOMAIN: homie
            HCACTL_HOMIE_CLIENT_ID: hactl
            HCACTL_HOMIE_CTRL_ID: homecontrol-automation
            HCACTL_VIRTUAL_DEVICES_CONFIG: file:./data/virtual_devices
            HCACTL_RULES_CONFIG: file:./data/rules
            HCACTL_META_CONFIG: file:./data/meta
            HCACTL_LUA_MODULE_CONFIG: file:./data/lua
            HCACTL_VALUE_STORE_CONFIG: sqlite:./data/store/data.db
            HCACTL_LOG_LEVEL: info
            TZ: "Europe/Berlin"
        volumes:
            - ./rules:/service/data/rules
            - ./virtual_devices:/service/data/virtual_devices
            - ./meta:/service/data/meta
            - ./lua:/service/data/lua
            - ./store:/service/data/store
            - /usr/share/zoneinfo/Europe/Berlin:/etc/localtime:ro
```
