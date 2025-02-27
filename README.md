# Homecontrol homie5 automation controller

## Introduction

`hc-homie5-automation` is a automation solution for homie v5 based (smarthome) automation.
In order to work you will need a mqtt broker - see [homie convention](https://homieiot.github.io/) for more details.

It consists of basically 2 main parts:

- rule engine
    - rules can be defined in yaml to handle things like "when switch is pushed, turn the light on)
    - also includes a lua scripting runtime to handle more complex rules
- virtual device manager
    - publish virtual homie devices that can be used for things like holding configurations for automations
    - create composite properties that aggregate values of other device properties using different functions (like max or a boolean AND or OR)

In addition it is also able to use normal (non homie convention) mqtt topics as input or output for rules and virtual devices.

## Documentation

The documentation is split up into several parts (see the `docs` folder):

1.  [Setup and configuration](./docs/setup_config.md)
2.  [General Concepts](./docs/general_concepts.md)
3.  [Rules](./docs/rules.md)
4.  [Lua Runtime](./docs/lua_runtime.md)
5.  [Virtual Devices](./docs/virtual_devices.md)

## Contributing

Contributions are welcome! Please submit a pull request or open an issue for discussion.

## License

This project was released under the MIT License ([LICENSE](./LICENSE))
