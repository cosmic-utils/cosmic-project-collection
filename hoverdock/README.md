# HoverDock

A cosmic-panel applet that draws the dock's pinned applications with macOS-style
hover magnification, and with no coloured plate behind them.

## Why an applet, and why it replaces the app list

The icons in COSMIC's dock belong to `cosmic-app-list`, which is a separate
process. Nothing outside that process can change how it draws — an applet cannot
reach into another applet's surface. So HoverDock takes that slot instead: same
place in the bar, same pinned applications (it reads COSMIC's own favourites),
different rendering.

`--install` swaps `com.system76.CosmicAppList` for `com.techy.HoverDock` in the
dock's `plugins_center`, leaving the launcher, workspaces, app-library and
minimise applets where they are. `--uninstall` puts it back.

## Use

```sh
cargo build --release
./enable.sh      # install, swap in, restart the panel
./disable.sh     # put COSMIC's app list back
```

`hoverdock --status` prints what it would draw without drawing anything: the
icons it resolved, the surface it would ask for, and the panel's ceiling.

## How it fits inside the panel

cosmic-panel launches applets with a Wayland socket to its own embedded
compositor, which speaks `wl_compositor`, `xdg_shell`, `wl_shm`, `wl_seat` and
`wl_output`. An applet is therefore an ordinary xdg-shell client — no libcosmic,
no iced — so the row layout, the spring and the CPU rasteriser are shared with
TechyDock unchanged.

Three facts from the panel's source decide the design:

- **Applets pick their own size.** In the normal path the panel sends
  `xdg_toplevel` a configure with `size = None`, and grows its bar to the
  thickest client in it.
- **There is a ceiling.** `constrain_dim` clamps the bar to
  `8 + gap ..= max_thickness + gap`, where `max_thickness` is 121 px at panel
  size L. Ask for more and the bar is cut back, taking the icon with it.
- **Nothing is clipped below that ceiling.** The panel only wraps a client in a
  `CropRenderElement` when it has configured a size for it, which it has not.

So the surface is allocated **once, at its largest**: tall enough for a fully
magnified icon, wide enough for the widest the row can ever become. Magnifying
happens entirely inside it, and the bar never resizes. The unused area is
transparent and costs nothing.

The icons are centre-locked rather than resting on a baseline: the panel centres
its other applets in the bar, and a row that grew in one direction only would
drift out of line with them the moment anything was hovered.

## The plate behind the icons

That is the panel's, not this applet's. Three values in
`~/.config/cosmic/com.system76.CosmicPanel.Dock/v1/` remove it:

| file | value |
|---|---|
| `opacity` | `0.0` |
| `keep_style_on_maximize` | `true` |
| `background` | `Color((0.0, 0.0, 0.0))` |

`keep_style_on_maximize` matters because `CosmicPanelConfig::maximize()`
otherwise overwrites `opacity` with `1.0` as soon as a window is maximised. The
`Color` background matters because `PanelColors::bg_color()` returns a colour
override verbatim, skipping the `if opaque { alpha = 1.0 }` line that can
otherwise bring the plate back.

## Seeing your open windows

An applet talks to cosmic-panel's own embedded compositor, which knows about the
panel and nothing else. So HoverDock opens a **second** connection, to the real
compositor, over the fd the panel puts in `X_PRIVILEGED_WAYLAND_SOCKET` — which
it only hands out because the desktop entry sets `X-HostWaylandDisplay=true`.

Over it: `ext_foreign_toplevel_list_v1` enumerates the windows,
`zcosmic_toplevel_info_v1` says which is focused, and
`zcosmic_toplevel_manager_v1` is the only one of the three that can raise one.
Each is optional; a missing global costs a feature, never the dock.

Two connections mean two event queues, so the main loop is calloop watching both
file descriptors rather than a blocking dispatch on either.

The COSMIC protocol bindings come from `pop-os/cosmic-protocols`, pinned to the
same revision `cosmic-applets` itself uses. A protocol description that shifts
under you is a runtime protocol error rather than a compile error, so the
revision is fixed rather than tracked.

## Vertical docks

Supported on all four edges. The layout, the magnification curve, the hit test
and the drag are written once, along the bar; the orientation only becomes x or
y in three places — the surface size, the pointer position, and the icon's
origin. The icons are never rotated: a side dock is a column of upright icons.

## Not there yet

- Tooltips.
- Hover previews of the windows themselves; the menu lists them by title.
