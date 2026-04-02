# Media (MPRIS)

`oatbar` supports controlling and displaying track information from any media player that implements the MPRIS D-Bus interface (e.g., Spotify, VLC, mpv, Firefox).

The `oatbar-mpris` command provides both a continuous stream of media variables and subcommands for controlling playback.

### Enabling the Block

Add the `oatbar-mpris` command to your `~/.config/oatbar/config.toml`:

```toml
[[command]]
name="mpris"
command="oatbar-mpris"
```

### Media Player Block Example

Here is a block that displays the title and artist, and uses click handlers to control playback:

```toml
[[block]]
name="media_player"
type="text"
value="${mpris:mpris.track}"
show_if_matches=[['${mpris:mpris.playback_status}', 'Playing']]

on_mouse_left="oatbar-mpris play-pause"
on_mouse_right="oatbar-mpris next"
on_mouse_middle="oatbar-mpris previous"
```

### Displaying Progress

The `oatbar-mpris` command polls the current position periodically and handles track seeks automatically. You can use this to display a progress bar.

```toml
[[block]]
name="media_progress"
type="number"
value="${mpris:mpris.position}"
min_value="0"
max_value="${mpris:mpris.length}"
number_display="progress_bar"
progress_bar_size=20
show_if_matches=[['${mpris:mpris.playback_status}', 'Playing']]
```
