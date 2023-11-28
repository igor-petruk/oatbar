# Command

Command is an external program that provides data to `oatbar`.

```toml
# Runs periodically
[[command]]
name="disk_free"
command="df -h / | tail -1 | awk '{print $5}'"
interval=60

# Streams continiously
[[command]]
name="desktop"
command="oatbar-desktop"
# format="i3bar"
```

`oatbar` will run each command as `sh -c "command"` to support basic shell
substitutions.

### Formats

Formats are usually auto-detected and there is no need to set `format` explicitly.

#### `plain`

Plain text format is just text printed to `stdout`.

```toml
name="hello"
command="echo Hello world"
```

This will set `${hello:value}` [variable](./variable.md) to be used 
by [blocks](./block.md). If the command outputs multiple lines, each print
will set this variable to a new value. If the command runs indefinitely, the 
pauses between prints can be used to only update the variable when necessary.
When the command is complete, it will be restarted after `interval`
seconds (the default is `10`).

If `line_names` are set, then the output is expected in groups of
multiple lines, each will set it's own variable, like `${hello:first_name}` and
`${hello:last_name}` in the following example:

```toml
name="hello"
line_names=["first_name", "last_name"]
```

Many [`polybar` scripts](https://github.com/polybar/polybar-scripts) can be
used via the `plain` format, as soon as they don't use polybar specific
formatting.

[`i3blocks` raw format](https://vivien.github.io/i3blocks/#_format) plugins
can be consumed too by means of the `line_names` set to standard names for
`i3blocks`.

#### `i3bar`

[`i3bar` format](https://oatbar.app/index.html) is the richest supported format.
It supports multiple streams of data across multiple "instances" of these streams.
In [`i3wm`](i3wm.org) this format fully controls the display of `i3bar`, where
for `oatbar` it is a yet another data source that needs to be explicitly
connected to properties of the blocks. For example instead of coloring 
the block, you can choose to color the entire bar. Or you can use color `red` coming 
from an `i3bar` plugin as a signal to show a hidden block.

Plugins that `oatbar` ships with use this format.

```toml
[[command]]
name="desktop"
command="oatbar-desktop"
```

The command `output-desktop` outputs:

```json
{"version":1}
[
[{"full_text":"workspace: 1","name":"workspace","active":0,"value":"1","variants":"1,2,3"},
{"full_text":"window: Alacritty","name":"window_title","value":"Alacritty"}],
...
```

This command is named `desktop` in the config. 

Each entry is groups variables under a different `name` that
represents a purpose of the data stream, in this case: `workspace` 
and `window_title`. Multiple entries with the same `name`, but different 
`instance` field to represent further breakdown (e.g. names of 
network interfaces from a network plugin).

All other values are individual data streams. The output from above will set
the following variables.

```ini
desktop:workspace.active=0
desktop:workspace.value=1
desktop:workspace.variants=1,2,3
desktop:workspace.full_text=workspace: 1
desktop:window_title.value=Alacritty
desktop:window_title.full_text=window: Alacritty
```

If `instance` is present in the entry, then the name of the variable is 
`command_name:name.instance.variable`.

`i3status` from `i3wm.org` is a great `i3bar` format command, that contains
a lot of useful data for your bar.

