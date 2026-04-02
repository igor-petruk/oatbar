# MCP Server Configuration

`oatbar-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io/) server that exposes
oatbar's internal variable system to AI assistants. It lets MCP-compatible clients read bar state,
inject content, trigger data refreshes, and restart oatbar — all in real time.

See [MCP Cookbook](../cookbook/mcp.md) for practical usage ideas.

<!-- toc -->

## Building

`oatbar-mcp` requires the `mcp` feature flag:

```sh
cargo build --release --features mcp --bin oatbar-mcp
```

## CLI Options

| Flag | Default | Description |
|---|---|---|
| `--instance-name <NAME>` | `oatbar` | Unique name of the running oatbar instance to connect to. Must match the instance oatbar was started with. |

## Configuration File

`oatbar-mcp` reads an optional config from `~/.config/oatbar/mcp.toml`. All fields have defaults
and the file may be omitted entirely.

```toml
# ~/.config/oatbar/mcp.toml

# Regex patterns for variables that MCP clients cannot read or write.
# Useful for hiding variables you prefer the assistant not to see,
# such as window titles that reveal what you are working on.
hidden_variables = ["desktop:window\\..*"]

# Seconds that mcp:recent (or mcp:<name>.recent) stays "1" after a
# status report before automatically resetting to "". Default: 5.
recent_timeout_seconds = 5

# Optional name suffix for mcp variables. When unset, variables are
# named `mcp:value` and `mcp:recent`. When set to e.g. "assistant",
# they become `mcp:assistant.value` and `mcp:assistant.recent`.
# Useful when running multiple MCP clients simultaneously.
mcp_name = "assistant"
```

### Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `hidden_variables` | list of regex strings | `[]` | Variable name patterns to hide from MCP clients. Matched variables cannot be read or written. |
| `recent_timeout_seconds` | integer | `5` | How long (in seconds) `mcp:recent` stays `"1"` after a status update before auto-resetting to `""`. |
| `mcp_name` | string | `None` | Optional name suffix. Changes MCP variable names from `mcp:*` to `mcp:<name>.*`. |

## MCP Variables

When `report_status` is called, `oatbar-mcp` writes two variables into oatbar's variable system:

| Variable | Value | Description |
|---|---|---|
| `mcp:value` | status string | The last status message reported by the MCP client. |
| `mcp:recent` | `"1"` or `""` | Set to `"1"` on status update, cleared after `recent_timeout_seconds`. Use with `show_if_matches` to show the block only while active. |

When `mcp_name` is set to e.g. `"assistant"`, these become `mcp:assistant.value` and
`mcp:assistant.recent`.

### Displaying MCP Status on the Bar

Use `show_if_matches` to show the block only while the assistant is actively reporting:

```toml
[[block]]
name = "mcp_status"
type = "text"
value = "${mcp:value}"
show_if_matches = [["${mcp:recent}", "1"]]
```

## Tools

### `list_vars`

Lists all oatbar variables and their current values. Useful for exploring available data and
diagnosing block issues.

| Parameter | Type | Description |
|---|---|---|
| `filter` | string (regex, optional) | Only return variables whose names match this regex. |
| `names_only` | bool (optional) | If `true`, return only variable names without values. Saves tokens when exploring. |
| `human_description` | string (optional) | Shown on the bar as a status update (updates `mcp:value` and `mcp:recent`). |

Variables matching `hidden_variables` patterns are excluded from the output.

### `get_var`

Gets the current value of a single variable.

| Parameter | Type | Description |
|---|---|---|
| `name` | string | Variable name (e.g., `clock:time`). |
| `human_description` | string (optional) | Shown on the bar as a status update (updates `mcp:value` and `mcp:recent`). |

Returns an error if the variable matches `hidden_variables`.

### `set_var`

Sets a variable to a new value. Variables do not need to exist beforehand — they are created
on first write. The bar redraws immediately.

| Parameter | Type | Description |
|---|---|---|
| `name` | string | Variable name. |
| `value` | string | New value. Plain text; [Pango markup](https://docs.gtk.org/Pango/pango_markup.html) rendering is controlled by the block that displays the variable, not the variable itself. |
| `human_description` | string (optional) | Shown on the bar as a status update (updates `mcp:value` and `mcp:recent`). |

Returns an error if the variable matches `hidden_variables`.

### `poke`

Forces an immediate refresh of one or all commands by interrupting their interval timer.

| Parameter | Type | Description |
|---|---|---|
| `command` | string (optional) | Name of the command to poke. If omitted, pokes all commands. |
| `human_description` | string (optional) | Shown on the bar as a status update (updates `mcp:value` and `mcp:recent`). |

### `report_status`

Writes a short status string to `mcp:value` and sets `mcp:recent` to `"1"` for
`recent_timeout_seconds`. Intended for MCP clients to show live activity on the bar.

| Parameter | Type | Description |
|---|---|---|
| `status` | string | Short status message (e.g., `"Building..."`). Sets `mcp:value` and triggers `mcp:recent`. |

### `restart_oatbar`

Restarts the connected oatbar instance cleanly via IPC, preserving the original command line.
Use this after editing `config.toml` to apply changes without terminal disruption.

| Parameter | Type | Description |
|---|---|---|
| `human_description` | string (optional) | Shown on the bar as a status update (updates `mcp:value` and `mcp:recent`). |

## Connecting an MCP Client

Add the server to your MCP client configuration. The exact format depends on your client;
generally it looks like:

```json
{
  "mcpServers": {
    "oatbar": {
      "command": "/path/to/oatbar-mcp",
      "args": ["--instance-name", "oatbar"]
    }
  }
}
```

The server communicates over stdio and connects to oatbar via a Unix domain socket. The socket
path is resolved in order: `$XDG_RUNTIME_DIR`, then `$XDG_STATE_HOME`, then the system temp
directory — whichever exists first. The socket file is named `oatbar/<instance_name>.sock`
within that directory.
