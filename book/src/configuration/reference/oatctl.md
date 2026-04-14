# `oatctl` Reference

`oatctl` is a command-line tool used to interact with a running `oatbar` instance. It communicates
over a Unix domain socket to inspect variables, trigger refreshes, and manage the background process.

<!-- toc -->

## Global Options

| Option | Default | Description |
|---|---|---|
| `--instance-name <NAME>` | `oatbar` | Unique name of the oatbar instance to connect to. |

## Commands

### `var ls`

Lists all active variables and their current values. Useful for discovering what data is
available from commands or other sources.

```bash
oatctl var ls
```

### `var get`

Prints the current value of a specific variable.

```bash
oatctl var get clock:time
```

### `var set`

Updates a variable to a new value. If the variable doesn't exist, it is created. The bar redraws
immediately if any block depends on this variable.

```bash
oatctl var set my_var "Hello World"
```

### `var rotate`

Rotates a variable's value through a provided list. If the current value is not in the list,
it resets to the first (right) or last (left) item.

```bash
# Toggle between "" and "show"
oatctl var rotate show_menu right "" "show"
```

### `poke`

Forces one or all commands to refresh immediately, ignoring their `interval` timers.

```bash
# Poke a specific command
oatctl poke --command my_script
# Poke everything
oatctl poke
```

### `dump-svg`

Exports the current visual state of the status bar to an SVG file.

```bash
oatctl dump-svg /tmp/bar.svg
```

| Argument | Description |
|---|---|
| `path` | Absolute path to save the SVG. |
| `--index` | Index of the bar to dump (default: `0`). |

### `restart`

Restarts oatbar cleanly. It fetches the original launch command line, terminates the current
process via IPC, and starts a new detached instance. Useful for applying configuration changes.

```bash
oatctl restart
```

### `terminate`

Securely terminates the background oatbar process via IPC.

```bash
oatctl terminate
```
