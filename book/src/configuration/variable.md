# Varable

<!-- toc -->

Variables are at length described in the [Command](./command.md) section
where they are produced and in the [Block](./block.md) section where they
are consumed.


# Standalone variables

You can declare your additional variables that do not come from commands. This is useful to
pre-process data with [`replace` and `replace_first_match`](./block.md#common-properties) to be used in multiple blocks.

```toml
[[var]]
name="clock_color_attr"
input = '${clock:color}'
replace = [["(.+)","foreground='$1'"]]

[[block]]
value = "<span ${clock_color_attr}>${clock:value}</span>"
```

Standalone variables can use each other only in the order they are declared in the file,
otherwise the result is undefined.

## Filters

Filters are additional functions you can apply to values inside of the `${...}` expressions. Example:

```toml
value = '${desktop:window_title.value|def:(no selected window)|max:100}'
```

Supported filters:

* `def` sets the default value if the input variable is empty
* `max` limits the length of the input. If it is larger, it is shortened with ellipsis (`...`)
* `align` aligns the text to take fixed width if it is smaller
  * First character is the filler
  * Second character is an alignment: `<`, `^` (center) or `>`
  * Min width
  * Example:
    * `hello` passed via `align: >10` will be `     hello`
