# Bar

Bar is a single panel that is positioned at the top or the bottom of a screen.

Here are all the properties it can have:

```toml
[[bar]]
# Blocks to show at different parts of the bar, can be empty.
# Currently it is user's responsibility is to make sure they don't overlap.
blocks_left=["block1", "block2"]
blocks_center=["block3"]
blocks_right=["block4"]

# Monitor to use as listed by `xrandr` command.
# If unspecified, the primary is used.
monitor="DP-6.8"

# Height of the bar.
height=32

# "bottom" is a default value.
position="top"

# Empty space between the blocks and the bar edges.
margin=5

# A bar is normally hidden, unless pops up.
popup=true  

# Make a popup bar pop up when the mouse is near the edge of the screen.
popup_at_edge=true  

# List of pairs [expression, regex].
# Show the block only if all expressions match respective regexes.
show_if_matches=[["${clock:value}",'.+']]
```
