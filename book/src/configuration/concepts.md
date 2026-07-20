# Concepts

`oatbar` configuration is built around four main concepts that work together to display information.

- [**Bar**](./bar.md) (`[[bar]]`): The top-level window and layout container.
- [**Block**](./block.md) (`[[block]]`): Visual widgets (text, images, graphs) displayed on the bar.
- [**Command**](./command.md) (`[[command]]`): External programs that fetch data.
- [**Variable**](./variable.md) (`[[var]]`): Named data holders populated by commands and used by blocks.

---

## Data Flow

1.  **Command** runs (e.g., `date`) and outputs text.
2.  `oatbar` parses the output and updates a **Variable** (e.g., `${clock:value}`).
3.  **Block** references the variable in its `value` property (e.g., `value="Time: ${clock:value}"`).
4.  **Bar** re-renders the block with the new text.

## Variable Interpolation

String properties in blocks support variable interpolation using the `${...}` syntax.

### Syntax
-   **Basic**: `${command_name:variable_name}`
-   **With Property**: `${command_name:variable_name.property}` (for complex data like i3bar JSON).
-   **Filters**: `${variable|filter:arg}` (e.g., `${cpu|align:>3}`).

### Example
```toml
[[command]]
name="clock"
command="date +%H:%M"
interval=60

[[block]]
name="my_clock"
type="text"
value="Time: ${clock:value}"
```

### Contextual variables (`#name`)

Some variables are not stored in global state — they are injected per-bar at render time. Use `#name` syntax (with a `#` prefix) to reference them inside `${...}` expressions.

The built-in contextual variable is:

| Variable | Value |
|---|---|
| `#monitor_name` | The monitor/display name of the bar (e.g. `DP-1`, `HDMI-1`) |

`#name` can appear as the entire expression or embedded within a longer variable key:

```toml
# Shows the monitor name of the bar this block is rendered on
value = '${#monitor_name}'

# Looks up workspace data scoped to this bar's monitor
active = '${desktop:workspace.#monitor_name.active}'
```

This is particularly useful for **per-monitor workspace blocks** — a single shared block config that automatically tracks the correct workspace on each bar.

## Debugging

Use `oatctl` to inspect the current state of variables and blocks.

```bash
# List all active variables and their values
oatctl var ls
```
