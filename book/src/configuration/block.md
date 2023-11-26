# Block

Blocks are the widgets displaying pieces of information on the bar. Supported blocks types are:

* **Text** focuses on displaying text information.
  * Fake text blocks can be used as separators, blank spaces or edges of the small partial bars within a bar.
* **Number** focuses on presenting numerical information.
  * Support numerical units, such as bytes or percentages.
  * Displayed as text or as progress bars.
* **Enum** focuses on presenting a selection among multiple items.
  * Active item and other items can be presented differently.
* **Image** focuses on rendering an image.

`oatbar` provides a lot of hidden power via these widgets, as they can provide
more than they initially seem. The reference below explains properties of
these blocks, but the [Cookbock](./cookbook.md) shows how to use them in a very
clever way.

## Common properties

```toml
[[block]]
# Name used as a reference 
name="block-name"

# Main input for the block.
value="<b>Clock:</b> ${clock:value}"

# A series of regex replacements that
# are applied to `value`.
# See https://docs.rs/regex/latest/regex/
replace=[
  ["^1$","www"],
  ["^2$","term"]
]
# If true, stop applying replaces after one row matches.
# If false, keep applying replaces to the end.
replace_first_match=false

# If set, max length of the block.
max_length=40
# If the displayed value is greater than max_length,
# truncate it and add elliptis at the end.
ellipsis="..."

# If true (default), full Pango markup is supported.
# https://docs.gtk.org/Pango/pango_markup.html
# It may be desirable to turn it off if input has
# HTML-like text to be displayed.
pango_markup=true

# If set, only show the block if this value is not empty.
show_if_set="${clock:value}"

# If set, and the bar has hidden=true, then this block
# can pop up.
#   - block - the block itself pops up
#   - partial_bar - the partial bar pops up
#   - bar - the entire bar pops up.
popup="partial_bar"
# If unset, the popup is triggered by any property change.
# If set, the popup is triggered by change of this property.
popup_value="${clock:value}"

# Font and size of the text in the block.
font="Iosevka 14"

# Base RGBA colors of the blocks.
background="#101010bb"
foreground="#ffff00"

# Properties of lines around the block, if set.
overline_color="#..."
underline_color="#..."
edgeline_color="#..."
line_width=0.4

# Margin and padding of the block within a bar.
margin=3.0
padding=5.0
```

To avoid repetition, consider using `default_block`, that
supports all common properties.

```toml
[[default_block]]
background="#202020"
```

Multiple named `default_block` sections can be used.

```toml
[[default_block]]
name="ws1_widgets"

[[block]]
inherit="ws1_widgets"
```

## Text block

Text blocks include all common properties, which should be enough to show
basic text or icons using [Pango markup](https://docs.gtk.org/Pango/pango_markup.html),
 icon fonts such as [Font Awesome](https://fontawesome.com/),
[Nerd Fonts](https://www.nerdfonts.com/), [IcoMoon](https://icomoon.io/) or emojis.

In addition, text blocks are used as separators to create **partial bars**.
They are smaller bars within a bar that groups multiple blocks together.

![Separators](img/separators.png)

```toml
[[bar]]
blocks_right=["L", "music", "R", "E", "L", "layout", "S", "clock", "R"]

[[block]]
name='music'
...
show_if_set = '${player:now_playing.full_text}'
popup = "partial_bar"

[[block]]
name="S"
type = 'text'
separator_type = 'gap'
value = '|'

[[block]]
name="E"
type = 'text'
separator_type = 'gap'
value = ' '
background = "#00000000"

[[block]]
name='L'
type = 'text'
separator_type = 'left'
separator_radius = 8.0

[[block]]
name='R'
type = 'text'
separator_type = 'right'
separator_radius = 8.0
```

`separator_type` gives a hint on where partial bars are located.
This helps when `popup="partial_bar"`. It also helps to collapse
unnecessary separators when normal blocks around them are hidden.
