# MCP

`oatbar-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io/) server that gives
AI assistants live access to oatbar's variable system. They can read bar state, write new content,
trigger data refreshes, edit your config, and restart oatbar — all in real time.

See [MCP Reference](../reference/mcp.md) for setup instructions and the full configuration
reference.

<!-- toc -->

## Bar Configurator

The most powerful use of `oatbar-mcp` is letting an AI assistant act as a configuration
assistant for your bar.

The assistant can:
- Read your `~/.config/oatbar/config.toml` and explain what each block does
- Add, remove, or rearrange blocks
- Tune colors, fonts, padding, and separators
- Diagnose why a block isn't showing up by inspecting live variables via `list_vars`
- Apply changes immediately by calling `restart_oatbar` — no terminal needed

**Getting started:** Connect `oatbar-mcp` to your MCP client, then ask:

- *"Show me what variables are available on the bar."*
- *"Add a block showing disk usage on the right side."*
- *"Why is my clock block not visible?"*
- *"Change the background of the workspace block to #2d2d2d."*

The assistant inspects current state, edits the config file, and restarts oatbar — all in one
conversation.

See also: [Configuration Overview](../README.md), [Block Reference](../block.md)

---

## Live Activity Indicator

Show what the MCP client is doing right now as it happens. The assistant reports short status
messages (`"Building..."`, `"Searching..."`, `"Editing config.toml"`) via `report_status` as it
works. The status clears automatically when activity stops.

Add this block to your config to display it:

```toml
[[block]]
name = "mcp_status"
type = "text"
value = "${mcp:value}"
show_if_matches = [["${mcp:recent}", "1"]]
```

---

## Inject Custom Content

Ask the assistant to put anything on your bar — a reminder, a count of open issues, a quote:

- *"Put 'Focus: finish the parser' on my bar."*

The assistant calls `set_var` with a name like `mcp:message` and you display it:

```toml
[[block]]
name = "ai_message"
type = "text"
value = "${mcp:message}"
show_if_matches = [["${mcp:message}", ".+"]]
```

Blocks support [Pango markup](https://docs.gtk.org/Pango/pango_markup.html) in their `value`
field, so if the assistant writes a plain text value like `"Focus: finish the parser"`,
the block can still render it with styling defined in the block config itself.

---

## Read Bar State for Context

The assistant can call `list_vars` or `get_var` to read what oatbar currently knows — focused
workspace, media track, system stats — and use that as context.

**Example:** Ask the assistant to summarize system status. It reads variables like `stats:cpu`
and `stats:memory` and returns a human-readable summary — no shell commands needed.

**Example:** Ask which workspace is focused. It reads `desktop:workspace.value` and can
suggest or take action based on the current context.

---

## Force-Refresh Stale Data

If bar data looks outdated, ask the assistant to call `poke` to trigger an immediate re-run:

- *"My clock block looks stuck — refresh it."*

The assistant calls `poke` with `command = "clock"` to restart that command's interval
immediately, or with no command to refresh everything at once.

---

## Privacy: Hiding Variables

Some variables you may not want an MCP client to read — for example, the currently focused
window title may reveal what you are working on. Use `hidden_variables` in
`~/.config/oatbar/mcp.toml` to block access:

```toml
# ~/.config/oatbar/mcp.toml
hidden_variables = ["desktop:window\\..*"]
```

The assistant will receive an access-denied error if it tries to read or write matching
variable names.

---

## Multiple Concurrent Clients

If you run more than one MCP client simultaneously, set `mcp_name` per client so their
status variables don't collide:

```toml
# ~/.config/oatbar/mcp.toml
mcp_name = "assistant"
```

Then display `${mcp:assistant.value}` on the bar, and use `${mcp:assistant.recent}` in
`show_if_matches`. Each client gets its own namespace.
