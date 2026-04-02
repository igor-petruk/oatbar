# Oatbar MCP Server

Oatbar is a standalone, lightweight, and highly customizable DE/WM status bar written in Rust.
It relies on a block-based module configuration system with dynamic system variables driving its
UI rendering.

## Documentation

**Before making any configuration changes, you MUST consult the official oatbar documentation
at https://oatbar.app/ using your web browsing tools.** Oatbar has its own unique TOML-based
configuration format that differs from other status bars. Do not guess — read the docs first
and offer the user to give you permission to access the documentation.

Reference links:

- Documentation: https://oatbar.app/
- Official Repository: https://github.com/igor-petruk/oatbar
- Configuration is typically located at: `~/.config/oatbar/config.toml`

## Status Reporting

On any interaction with this MCP server, always populate the `human_description`
field to explain what you are about to do (e.g., "Fetching uptime data...").
Be creative and quirky in these descriptions.

Report what you are about to do more specifically, do NOT report something useless
like "Reading all variable names". The server will automatically use this description
to update some `{mcp_vars}` variables that the user may choose to display on their status bar.

## Working with Variables

As an MCP agent, when a user asks to "update something on the status bar", you can utilize these
exposed tools to manipulate or investigate variables in real-time. Oatbar instantly reacts
to variable changes by redrawing associated blocks!

Variables are usually named with a colon separating the command/source namespace and the sub-variable,
for example: `clock:time`, `mpris:mpris.position_str`, or `i3:workspace.focused`.

- Discover available variables dynamically using `list_vars`.
- Create new variables by setting them using `set_var` — they don't need to exist beforehand!
- If the data looks stale, poke the source command using the `poke` tool to force a refresh.
  The command name is the first part of the variable name before the colon (e.g. `clock` for `clock:time`).
- Pixmap variables are often in a format: `[width, height, ARGB_byte0, ARGB_byte1, ...]`.

## Text Formatting

Text variables and UI components can be formatted using Pango XML markup tags,
for example: `<span color='red' size='12pt'>styled text</span>`.

Make sure to use absolute units such as `pt` when specifying font sizes.

## Modifying Configuration

If you implement edits directly inside the TOML configuration files,
always immediately execute `restart_oatbar` to apply changes.

Always run `fc-list` using your terminal tools before configuring or updating a font family
to verify the system has the exact font you want to use installed.
