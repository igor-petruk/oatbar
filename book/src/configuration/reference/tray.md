# Tray Reference

This page documents how `oatbar` interacts with the [Status Notifier Item (SNI)](https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/) protocol.

The `oatbar-sni` backend implements a cross-platform System Tray endpoint that works on both Wayland and X11.

## Variables

When an application registers a tray item on DBus, `oatbar-sni` exports the following variables. All variables are prefixed with the app's namespace ID (`AppID`):

| Variable | Description |
|---|---|
| `sni.AppID.visible` | `1` if the tray item is active, empty string if disconnected. Use with `show_if_matches`. |
| `sni.AppID.dbus` | The DBus name required by `oatbar-sni activate` to route mouse interactions. |
| `sni.AppID.pixmap` | JSON array of pixel data: `[width, height, byte0, byte1, ...]` in ARGB format. Bind to the block's `pixmap` field. |
| `sni.AppID.icon_name` | Named icon from the system icon theme (alternative to pixmap). *Requires `gtk4_icons` feature (disabled by default).* |
| `sni.AppID.icon_theme_path` | Additional icon theme search path for resolving `icon_name`. *Requires `gtk4_icons` feature (disabled by default).* |

## Mouse Event Handling

Forward mouse clicks to the tray application using `oatbar-sni activate`:

```bash
oatbar-sni activate <dbus_name> <button> <x> <y>
```

| Argument | Description |
|---|---|
| `dbus_name` | The DBus name from `sni.AppID.dbus`. |
| `button` | One of `left`, `middle`, `right`. |
| `x`, `y` | Screen coordinates of the click (use `$ABS_X`, `$ABS_Y`). |

**Usage pattern:**
```toml
on_mouse_left="oatbar-sni activate ${sni:sni.App.dbus} left $ABS_X $ABS_Y"
```

## Context Menus (DBusMenu Protocol)

The SNI specification supports two approaches for context menus:

1. **Application-managed:** The application draws its own context menu window. `oatbar` simply forwards the activation request.
2. **Bar-managed (DBusMenu):** The application exports menu items over DBus using the DBusMenu protocol. The bar is responsible for displaying them.

For bar-managed menus, `oatbar-sni` pipes the menu entries into an external launcher process via stdin and reads the user's selection from stdout.

**Default command:**
```bash
rofi -dmenu -i -no-sort -hover-select -me-select-entry '' -me-accept-entry MousePrimary
```

### Customizing the Menu Renderer

Override the menu display command by creating `~/.config/oatbar/sni.toml`:

```toml
dbusmenu_display_cmd = "wofi --dmenu"
```

The configured command receives menu entries on stdin (one per line) and must print the selected entry to stdout. See the [Cookbook](../cookbook/tray.md#customizing-context-menus) for examples.

## DBusMenu CLI Commands

### `dbusmenu print`

Dumps the full menu tree of a tray application to the console:

```bash
oatbar-sni dbusmenu print <dbus_address>
```

Each line shows the menu item ID, indentation for hierarchy, and the label. Useful for discovering item IDs and labels for use with `item-click`. Example:

```
20    Hide VLC media player in taskbar
19    ---
18    Play
13    ---
12    Speed:
3       Faster (fine)
2       Normal Speed
1       Slower (fine)
11    ---
9     Increase Volume
8     Decrease Volume
7     Mute
6     ---
5     Open Media
4     Quit
```

### `dbusmenu item-click`

Programmatically clicks a menu item without user interaction:

```bash
oatbar-sni dbusmenu item-click <dbus_address> --id <item_id>
oatbar-sni dbusmenu item-click <dbus_address> --regex <pattern>
```

| Option | Description |
|---|---|
| `--id <item_id>` | Click the menu item with this exact numeric ID (from `dbusmenu print`). |
| `--regex <pattern>` | Click the menu item whose label matches this regex. Fails if zero or more than one item matches. |

Exactly one of `--id` or `--regex` must be provided.
