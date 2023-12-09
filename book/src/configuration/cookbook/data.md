# Data

This chapter contains examples of common sources and methods of ingesting
data for your blocks.

<!-- toc -->

## Common Blocks

### App Launcher

```toml
[[block]]
name='browser'
type = 'text'
value = "<span font='Font Awesome 6 Free 22'></span> "
on_click_command = 'chrome'
```

### Clock

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

### Keyboard

`oatbar` ships with keyboard status utility that streams keyboard layouts
and indicator values in `i3bar` format.

If you run it, you will see

```json
❯ oatbar-keyboard
{"version":1}
[
[{"full_text":"layout: us","name":"layout","active":0,"value":"us","variants":"us,ua"},
{"full_text":"caps_lock:off","name":"indicator","instance":"caps_lock","value":"off"},
 ...],
```

Enable it with

```toml
[[command]]
name="keyboard"
command="oatbar-keyboard"
```

#### Layout

`oatbar-keyboard` is designed to work with `setxkbmap`. 
For example you set up your layouts on each WM start like this:

```shell
setxkbmap -layout us,ua -option grp:alt_space_toggle
```

Enable `oatbar-keyboard` and use an `enum` block:

```toml
[[block]]
name = 'layout'
type = 'enum'
active = '${keyboard:layout.active}'
variants = '${keyboard:layout.variants}'
on_click_command = "oatbar-keyboard layout set $BLOCK_INDEX"
```

#### Indicators

Show indicators, such as `caps_lock`, `scroll_lock` and `num_lock`
as follows:

```toml
[[block]]
name = 'caps_lock'
type = 'text'
value = '${keyboard:indicator.caps_lock.full_text}'
```

### Active workspaces and windows

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
on_click_command = "oatbar-desktop $BLOCK_INDEX"

[[block]]
name='window'
type = 'text'
value = '${desktop:window_title.value}'
pango_markup = false  # Window title can happen to have HTML.
max_length = 100
```

### System stats

`oatbar` ships with a `oatbar-stats` utility that streams system stats in the `i3bar`
format:

* CPU
* Memory
* Network
  * Interface names
  * Running status
  * Address
  * Download and upload rates

There is a lot of data you can display on your `blocks`. Enable `oatbar-stats`
like this:

```toml
[[command]]
name="stats"
command="oatbar-stats"
```

Restart `oatbar` and examine the new variables.

```shell
oatctl var ls | grep '^stats:'
```

The example output below. 

```ini
stats:cpu.full_text=cpu:  2%
stats:cpu.percent=2
stats:memory.free=8744980480
stats:memory.full_text=mem: 73% 32.9 GB
stats:memory.percent=73
stats:memory.total=32915705856
stats:memory.used=24170725376
stats:net.igc0.full_text=igc0: 192.168.0.160
stats:net.igc0.ipv4_0_addr=192.168.0.160
stats:net.igc0.ipv4_0_broadcast=192.168.0.255
stats:net.igc0.ipv4_0_run=true
stats:net.igc0.ipv4_0_up=true
stats:net.igc0.mac_0_addr=48:21:0b:35:ca:08
stats:net.igc0.mac_0_run=true
stats:net.igc0.mac_0_up=true
stats:net.igc0.rx_per_sec=1704
stats:net.igc0.tx_per_sec=1110
...
```

Entries with `full_text` are a good start to display directly,
if you do not need more fine grained customizations.

```toml
[[block]]
name="ethernet"
type="text"
value="${stats:net.igc0.full_text}"
```

### Disk space

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

## Third-party sources

Existing bar ecosystems already can provide large mount of useful information.
`oatbar` by-design focuses on making it possible to adapt third-party data sources.

### `i3status`

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
output_format="<b>CPU:</b>{}"
number_display="text"
```

If you prefer a progress bar:

```toml
number_display="progress_bar"
```

### `conky`

As `i3status`, `conky` can also be a great source of system data.

`conky` can print it's [variables](https://conky.sourceforge.net/variables.html) as plain text
and `oatbar` can consume it as multi-line plain text. Example `~/.oatconkyrc`:

```lua
conky.config = {
    out_to_console = true,
    out_to_x = false,
    update_interval = 1.0,
}

conky.text = [[
$memperc%
$cpu%
]]
```

If you run `conky -c ~/.oatconkyrc` you will see repeating groups of numbers:

```
2%
10%
5%
10%
```

In `oatbar` config:

```toml
[[command]]
name="conky"
command="conky -c ~/.oatconkyrc"
line_names=["mem","cpu"]

[[block]]
name='cpu'
type = 'number'
value = "${conky:cpu}"
number_type = "percent"

[block.number_display]
type="text"
output_format="<b>CPU:</b>{}"

[[block]]
name='mem'
type = 'number'
value = "${conky:mem}"
number_type = "percent"

[block.number_display]
type="text"
output_format="<b>MEM:</b>{}"
```

### `i3blocks`

`i3blocks` in a drop-in replacement for `i3status` to be used in
`i3bar`. If you have existingi `i3blocks` configs, feel free to plug it
directly into `oatbar`:

```toml
[[command]]
name="i3blocks"
command="i3blocks"
```

You can check which `oatbar` variables it makes available by running
`i3blocks` in your console.

The indirection between the script, `i3blocks` and `oatbar` is not required.
You can connect any plugin from the [`i3block-contrib`](https://github.com/vivien/i3blocks-contrib)
excellent collection directly into `oatbar`.

For example:

```console
$ git clone https://github.com/vivien/i3blocks-contrib
$ cd ./i3blocks-contrib/cpu_usage2
$ make
$ ./cpu_usage2 -l "cpu: "
cpu: <span> 39.79%</span>
cpu: <span> 47.06%</span>
```

As you can see, it inputs only one line of data each interval,
so setting `line_names` is not necessary, however always check for it.

```toml
[[command]]
name="cpu_usage2"
command="/path/to/cpu_usage2 -l 'cpu: '"

[[block]]
name="cpu_usage2"
type="text"
value="${cpu_usage2:value}"
```

### HTTP APIs

HTTP JSON APIs that do not require complicated login are extremely
easy to integrate using `curl` and `jq`.

Explore your JSON first

```console
$ curl 'https://api.ipify.org?format=json'
{"ip":"1.2.3.4"}
```

`jq -r` let's you extract a value from a JSON object. Add the command
to `oatbar` config, but make sure to set a sufficient `interval` not to get
banned.

```toml
[[command]]
name="ip"
command="curl 'https://api.ipify.org?format=json | jq -r .ip"
interval=1800

[[block]]
name="ip"
type="text"
value="my ip: ${ip:value}"
```

### File

You can use file watching utils to output file contents on any file change. 
For example for Linux you can use `fswatch`.

```toml
[[command]]
command="cat ./file; fswatch --event Updated ./file | xargs -I {} cat {}"
```

### Socket

Use `socat` to read from sockets. TCP socket:

```toml
[[command]]
command="socat TCP:localhost:7777 -"
```

SSL socket:

```toml
[[command]]
command="socat OPENSSL:localhost:7777 -"
```

For Unix socket:

```toml
[[command]]
command="socat UNIX-CONNECT:/path/to/socket -"
```
