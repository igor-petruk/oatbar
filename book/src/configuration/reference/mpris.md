# MPRIS Reference

This page documents the variables and CLI commands exposed by `oatbar-mpris`.

## Variables

The `oatbar-mpris` daemon continuously outputs the state of the active MPRIS media player. All variables are available under the `mpris.mpris.*` namespace (assuming your command is named `mpris`).

| Variable | Description |
|---|---|
| `mpris.mpris.full_text` | A formatted string combining artist and title (e.g. `Artist - Title`). |
| `mpris.mpris.track` | Identical to `full_text` but exposed as a standalone variable for convenience. |
| `mpris.mpris.title` | The track title. |
| `mpris.mpris.artist` | The track artist. |
| `mpris.mpris.album` | The album name. |
| `mpris.mpris.playback_status` | Current playback state: `Playing`, `Paused`, or `Stopped`. |
| `mpris.mpris.player` | Short name of the active player (e.g. `spotify`, `vlc`). |
| `mpris.mpris.volume` | Player volume level (0-100). |
| `mpris.mpris.length` | Track duration in seconds. |
| `mpris.mpris.position` | Current playback position in seconds. |
| `mpris.mpris.position_ts` | The Unix timestamp (in seconds) of the last position sample. Useful for manual interpolation. |
| `mpris.mpris.rate` | Current playback rate (multiplier, typically 1.0). |

## CLI Commands

The `oatbar-mpris` binary also acts as a CLI client to control the active media player.

```bash
oatbar-mpris [COMMAND]
```

### Output Mode
Running without any command acts as a long-running daemon that streams i3bar JSON to standard output. It is intended to be run by `oatbar` as an external block configuration command.

### Media Controls
These commands interact with the currently active media player:

- `play` — Start or resume playback.
- `pause` — Pause playback.
- `play-pause` — Toggle between play and pause.
- `next` — Skip to the next track.
- `previous` — Go to the previous track.
- `stop` — Stop playback entirely.
- `seek <PCT>` — Seek to a specific percentage of the track (0-100).

**Example:**
```bash
oatbar-mpris seek 50
```
*(Seeks strictly to the midpoint of the current track)*
