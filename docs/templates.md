# Templates

Rules and virtual devices can be written once as a template and instantiated with a few
parameters, or generated per discovered device with `for_each`. The constructs are the same as in
the dashboard pages; the engine is `hc_homie5::template`.

Existing rule and virtual-device files are unaffected: a document without any of the constructs
below loads exactly as before.

## Template files

Templates live in files ending in `.tpl.yaml` in the rules folder (rule templates) or the
virtual-devices folder (virtual-device templates). A template file holds only template
documents; other documents in it are skipped with a warning, and a template document in any other
file is skipped as well.

```yaml
template: switch-off-rule        # name, used by `use:`
params: [device, name]           # parameters
defaults:                        # optional defaults; may refer to earlier and supplied params
  name: "{{ device }} off at night"
body:                            # any YAML: a whole document, or a fragment for a list
  name: "{{ name }}"
  triggers:
    - schedule: "0 0 23 * * * *"
  actions:
    - type: set
      target: "{{ device }}/switch/state"
      value: { Bool: false }
```

A template file can hold several templates (`---` separated). Templates are shared by all files of
the same folder: rule templates by rules, virtual-device templates by virtual devices.

## Constructs

| Construct | Where | Effect |
|---|---|---|
| `"{{ expr }}"` | any string value | the value of the expression; a string with exactly one placeholder yields the typed value (number, bool, list), other text renders a string |
| `use: name` + `with: {..}` | whole document, any mapping, any list item | replaced by the template body; a list body is spliced into the surrounding list |
| `for_each: { query, sort_by }` + `use:`/`with:` or `item:` | whole document, any list item | one expansion per matching device, `item` is the device |
| `when: expr` | whole document, any list item | the item is kept only if the expression is true |

Lists include node and property lists of virtual devices, compound `members`, trigger lists,
trigger `properties`, action lists and action `targets`. Keys are never templated.

Expressions are [minijinja](https://docs.rs/minijinja) expressions with strict undefined
variables: an unknown variable is an error.

### Instances

```yaml
# rules/lights.yaml
use: switch-off-rule
with:
  device: garden-light
---
use: switch-off-rule
with:
  device: terrace-light
  name: Terrace light off
```

### for_each

`query` is a [query](general_concepts.md) (device, node, property and `$meta` conditions); every
discovered device with at least one matching property yields one element. `sort_by` is a field
path of the element (for example `label` or `meta.room`); elements are then ordered by `id`.

What `item` holds (descriptions and `$meta`, never property values):

```yaml
id: dev-1
domain: homie
ref: homie/dev-1
exists: true
name: Thermostat 1          # Homie $name or null
label: Büro                 # meta friendly_name, else $name, else id
meta: { room: Büro }        # first non-empty value per device annotation
annotations: { room: [Büro] }
nodes:
  thermostat:
    name: Thermostat
    type: hc-smarthome/v2/cap/thermostat
    meta: {}                # node annotations, first values
    properties:
      set-temperature:
        name: Target temperature
        datatype: float     # text form
        format: "7.5:28.5:0.5"   # text form, null when the property has none
        unit: "°C"
        settable: true
        retained: true
matched: [homie/dev-1/thermostat/set-temperature]   # properties matching the query
```

This is the device snapshot the dashboard's expressions see, plus `matched`.

A virtual device with one node per annotated window contact:

```yaml
id: windows
name: Windows
nodes:
  - for_each:
      query: { meta: { annotations: { tags: { any: window } } } }
      sort_by: label
    item:
      id: "{{ item.id }}"
      name: "{{ item.label }}"
      properties:
        - id: state
          datatype: boolean
          compound_spec:
            members: ["{{ item.matched[0] }}"]
```

A rule triggered by all of them:

```yaml
name: window-opened
triggers:
  - properties:
      - for_each:
          query: { meta: { annotations: { tags: { any: window } } } }
        item: "{{ item.ref }}/contact/state"
    changed: { to: { Bool: true } }
actions:
  - type: run
    script: |
      require("contacts").on_window(event)
```

Where a live query suffices (trigger `queries:`, compound `query:` members), prefer it: it follows
devices without re-creating the rule or virtual device.

## Lua scripts are never templated

The values of `script:` keys (Lua in `run` actions) are taken verbatim: `{{ }}` in a script is
not evaluated, so Lua table literals such as `{{a=1},{b=2}}` work unchanged, in documents and in
template bodies. A template cannot insert parameters into a script; scripts read what they need
from `event` (e.g. `event.prop`) or from a Lua module.

Elsewhere a literal `{{` is written as `{{ '{' ~ '{' }}`.

## When documents are expanded

Documents using templates are expanded 500 ms after the last change of:

- a template (`*.tpl.yaml` added, changed or removed),
- a document using templates,
- and, when a `for_each` exists, a discovered device (appeared, description changed, removed) or
  its `$meta` annotations. Property values never cause an expansion.

Each expansion result is compared with the previous one by content:

- unchanged rules and virtual devices are left alone; rules keep their running timers;
- a changed result replaces the old one (like editing a rule: its timers are cancelled, a virtual
  device is removed and re-published);
- a result that is no longer produced is removed.

A `for_each` should not match the virtual devices it generates.

## Errors

When a document is loaded or a template changes, every document is checked: unknown templates,
missing or unknown parameters, expression syntax, malformed `use`/`for_each`/`when` and template
recursion. Errors are logged with the file and the YAML path, for example

```
ERROR rule template expansion failed; document skipped error=lights.yaml: at with.device (param `device`): ...
```

A document with errors yields nothing until it is fixed; all other documents keep working. An
invalid template file is logged and skipped. A generated rule or virtual device that does not
deserialize is logged and skipped like an invalid document.

## Example: smart thermostat

`tests/fixtures/smart-thermostat/` holds the office smart thermostat as a template
(`smart-thermostat.tpl.yaml`) and its instance (`smart-heating.v5device.yaml`):

```yaml
use: smart-thermostat
with:
  id: bue-thermostat
  name: Buero Thermostat
  trv: matter-5489-227-5fdb7d425dda566c
  sensor: leq1258226
```

The instance expands to exactly the hand-written virtual device (checked by
`tests/smart_thermostat_template.rs`); the smart-heating rules and `smart_heating.lua` are
unchanged.
