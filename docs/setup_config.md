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
| `HCACTL_VALUE_STORE_CONFIG`     | Defines how values are stored                     | `inmemory`,<br />`sqlite:/path/to/database.db`,<br />`kubernetes:secret`                           | inmemory                      | `"sqlite:/sevice/values.db"`                    |
| `HCACTL_LOCATION`               | Defines the geographical location                 | `<latitude>,<longitude>,<elevation>`                                                               | `0,0,0`                       | `"48.1351,11.5820,519"`                         |

### Rules and Virtual Devices config backends

For rules and virtual devices `hc-homie5-automation` supports multiple backends as input.

- **File:**

    Will read rules and virtual devices specifications from yaml files inside the specified folder.

    - Example: `file:/path/to/config`

- **MQTT:**

    Will read rules and virtual devices specifications from all subtopics with the specified topic. Each topic is treated like a yaml file in the file system. Ensure valid yaml data is published under these topics.

    - Example: `mqtt:some/topic`

- **Kubernetes:**

    Will read rules and virtual devices specifications from a kubernetes `ConfigMap`. Each data field in the `ConfigMap` is treated like a yaml file in the file system. Ensure valid yaml data is published under these fields.

    - Example: `kubernetes:config-name[,namespace]`
    - If no namespace is provided, the `default` namespace is used.

All three backends support hot reload, this means your changes are immediately applied without the need of a restart.

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
            HCACTL_VALUE_STORE_CONFIG: sqlite:./data/store/data.db
            HCACTL_LOGLEVEL: debug,info,warn,error
            TZ: "Europe/Berlin"
        volumes:
            - ./rules:/service/data/rules
            - ./virtual_devices:/service/data/virtual_devices
            - ./store:/service/data/store
            - /usr/share/zoneinfo/Europe/Berlin:/etc/localtime:ro
```
