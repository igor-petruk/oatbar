# Concepts

[**Command**](./command.md) is a source of data that streams [**variables**](variable.md). Variables are
used in properties of [**blocks**](block.md). [**Bars**](bar.md) are made of blocks. All you have to learn.

```toml
[[bar]]
blocks_right=["clock"]

[[block]]
name = 'clock'
type = 'text'
value = '${clock:value}'

[[command]]
name="clock"
command="date '+%a %b %e %H:%M:%S'"
```
