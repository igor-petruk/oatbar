# Varable

<!-- toc -->

Variables are at length described in the [Command](./command.md) section
where they are produced and in the [Block](./block.md) section where they
are consumed.

If `oatbar` is running and you have added a few commands, you can see all available variables
using `oatctl` command:

```
$ oatctl var ls
clock:value=Mon Jul  8 00:40:19
desktop:window_title.full_text=window: Alacritty
desktop:window_title.value=Alacritty
desktop:workspace.active=0
desktop:workspace.full_text=workspace: 1
desktop:workspace.value=1
desktop:workspace.variants=1,2
...
```

More info on how to get and set variables programmatically: 

```
$ oatctl var help
````

# Standalone variables

You can declare your additional variables that do not come from commands. This is useful to
pre-process data with [`replace` and `replace_first_match`](./block.md#common-properties) to be used in multiple blocks.

```toml
[[var]]
name="clock_color_attr"
value = '${clock:color}'
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
* `align` aligns the text to occupy fixed width if it is shorter than a certain length
  * First character is the filler
  * Second character is an alignment: `<`, `^` (center) or `>`
  * Min width
  * Example:
    * `hello` passed via `align:_>10` will be `_____hello`
