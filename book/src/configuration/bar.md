# Bar

Bar is a single panel that is positioned at the top or the bottom of a screen.

Here are all the properties it can have:

| Property | Type | Default | Description |
|---|---|---|---|
| `height` | int | `32` | Height of the bar in pixels. |
| `position` | string | `bottom` | Position on screen: `top`, `bottom` or `center`. |
| `monitor` | string | `primary` | Monitor name (from `xrandr`) to display the bar on. |
| `blocks_left` | list | `[]` | List of block names to align to the left. |
| `blocks_center` | list | `[]` | List of block names to align to the center. |
| `blocks_right` | list | `[]` | List of block names to align to the right. |
| `margin` | int/object | `0` | Margin around the bar. Can be a single number or `{top=0, bottom=0, left=0, right=0}`. |
| `background` | color | `transparent` | Background color of the entire bar. |
| `popup` | bool | `false` | If `true`, the bar is hidden until triggered by a block (see [Block](./block.md#popups-and-visibility)) or mouse at edge. |
| `popup_at_edge` | bool | `false` | If `true`, showing the mouse at the screen edge triggers the popup. |
| `show_if_matches` | list | `[]` | List of `[expression, regex]` pairs. Bar is visible only if **all** regexes match. |

### Example

```toml
[[bar]]
height=32
position="top"
monitor="eDP-1"
background="#1e1e2e"
margin={left=10, right=10, top=5, bottom=0}
blocks_left=["workspace"]
blocks_center=["window_title"]
blocks_right=["clock", "sys_info"]
popup=false
popup_at_edge=true
show_if_matches=[["${desktop:workspace.active}", "1"]]
```
