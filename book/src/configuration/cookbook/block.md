# Basic Blocks

## Separator

Separator is just a text block.

```toml
[[bar]]
blocks_left=["foo", "S", "bar", "S", "baz"]

[[block]]
name="S"
type = 'text'
separator_type = 'gap'
value = '|'
foreground = "#53e2ae"
```

This approach offers maximum flexibility: 
* Multiple separator types and styles
* Dynamically separators based on conditions
* Disappearing separators via `show_if_set`

Yet, specifying `separator_type` gives `oatbar` a hint that the block is
a separator. For example multiple separators in a row do not make sense and
they will collapse if real blocks between them become hidden.

## App Launcher

```toml
[[block]]
name='browser'
type = 'text'
value = "<span font='Font Awesome 6 Free 22'></span> "
on_click_command = 'chrome &'
```

## Clock

```toml
[[command]]
name="clock"
command="date '+%a %b %e %H:%M:%S'"
interval=1

[[block]]
name = 'clock'
type = 'text'
value = '${clock:value}'
```

If you do not need to show seconds, you can make `interval` smaller.

## Keyboard layout

`oatbar-keyboard` outputs `setxkbmap`-compatible
layout stream in `i3bar` format. For example you set up
your layouts on each WM start like this:

```shell
setxkbmap -layout us,ua -option grp:alt_space_toggle
```

If you run it, you will see

```json
❯ oatbar-keyboard
{"version":1}
[
[{"full_text":"layout: us","name":"layout","active":0,"value":"us","variants":"us,ua"}],
```

Use it as a command in your config and attack it to the `enum` block:

```toml
[[command]]
name="keyboard"
command="oatbar-keyboard"

[[block]]
name = 'layout'
type = 'enum'
active = '${keyboard:layout.active}'
variants = '${keyboard:layout.variants}'
on_click_command = "oatbar-keyboard $BLOCK_VALUE &"
```

## Active workspaces and windows

```oatbar-desktop``` talks to your WM via EWMH protocol to obtain
the information about active workspaces and windows.

```json
❯ oatbar-desktop
{"version":1}
[
[{"full_text":"workspace: 1","name":"workspace","active":0,"value":"1","variants":"1,2,3"},
{"full_text":"window: Alacritty","name":"window_title","value":"Alacritty"}],
```

```toml
[[command]]
name="desktop"
command="oatbar-desktop"

[[block]]
name = 'workspace'
type = 'enum'
active = '${desktop:workspace.active}'
variants = '${desktop:workspace.variants}'
# Optional replacement with icons
replace = [
   ["1",""],
   ["2",""],
   ["3",""]
]
font="Font Awesome 6 Free 13"
on_click_command = "oatbar-desktop $BLOCK_VALUE &"

[[block]]
name='window'
type = 'text'
value = '${desktop:window_title.value}'
pango_markup = false  # Window title can happen to have HTML.
max_length = 100
```

## Disk space

Example for `/home` directory partition:

```toml
[[command]]
name="home_free"
command="df -h /home | tail -1 | awk '{print $5}'"
interval=60

[[block]]
name='home_free'
type = 'text'
value = '<b>/home</b> ${home_free:value}'
```

## `i3status`

`i3status` is a great cross-platform source of information about the system. It supports:

* CPU
* Memory
* Network
* Battery
* Volume

`i3status` is designed to be used by `i3bar`, but `oatbar` supports this format natively.
Enable it in `~/.i3status.conf` or in `~/.config/i3status/config`:

```conf
general {
        output_format = "i3bar"
}
```

Add you plugin as described in the [`i3status` documentation](https://i3wm.org/docs/i3status.html).
Prefer simple output format, as you can format it on the `oatbar` side. Example:

```
order += cpu_usage

cpu_usage {
   format = "%usage"
}
```

If you run `i3status` you will now see

```json
❯ i3status
{"version":1}
[
[{"name":"cpu_usage","markup":"none","full_text":"00%"}]
,[{"name":"cpu_usage","markup":"none","full_text":"02%"}]
```

In `oatbar` config:

```toml
[[block]]
name='cpu'
type = 'number'
value = "${i3status:cpu_usage.full_text}"
number_type = "percent"

[block.number_display]
type="text"
output_format="<b>CPU:</b>{}"
```

If you prefer a progress bar:

```toml
[block.number_display]
type="progress_bar"
bar_format="<b>CPU:</b> {}"
```
