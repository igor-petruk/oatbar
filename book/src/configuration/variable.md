# Varable

Variables are at length described in the [Command](./command.md) section
where they are produced and in the [Block](./block.md) section where they
are consumed.

It is also possible to declare standalone variables and additionally
pre-process data with [`replace` and `replace_first_match`](./block.md#common-properties) to be used in blocks.

```toml
[[var]]
name="clock_color_attr"
input = '${clock:color}'
replace = [["(.+)","foreground='$1'"]]

[[block]]
value = "<span ${clock_color_attr}>${clock:value}</span>"
```

Standalon Variables can use each other only in the order they are declared in the file,
otherwise the result is undefined.
