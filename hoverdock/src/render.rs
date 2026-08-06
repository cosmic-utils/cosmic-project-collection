//! Drawing the dock.
//!
//! Everything is rasterised on the CPU into one ARGB buffer. That sounds
//! extravagant until you count: the dock is a few hundred pixels tall and only
//! redraws while something is moving, and the expensive effect — the blur
//! behind it — is done by the compositor on the GPU. What is left is a handful
//! of rounded rectangles and a dozen already-rasterised icons.

use tiny_skia::{
    BlendMode, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform,
};

use crate::config::{Indicator, Theme};
use crate::icons::IconCache;
use crate::layout::Placed;

/// One icon's state at the moment of drawing.
pub struct Item<'a> {
    pub icon: Option<&'a str>,
    pub placed: Placed,
    /// 0 when at rest, 1 at full magnification — drives the subtler effects.
    pub emphasis: f32,
    pub running: bool,
}

pub struct Scene<'a> {
    pub theme: &'a Theme,
    /// Where the background panel starts and ends horizontally.
    pub panel: Option<(f32, f32)>,
    /// The dock's baseline: the bottom edge icons sit on.
    pub baseline: f32,
    pub panel_height: f32,
    pub glass: bool,
    pub indicator: Indicator,
    /// True when the dock is at the top of the screen and everything hangs
    /// downwards from the baseline instead of standing on it.
    pub flipped: bool,
    /// Lock the icons' *centre* to this line across the bar instead of resting
    /// them on the baseline, so magnification grows symmetrically in both
    /// directions.
    ///
    /// Inside a panel this is what keeps the row aligned with the neighbouring
    /// applets, whose icons the panel centres in the bar. A dock that owns the
    /// whole screen edge wants the baseline instead: growing away from the edge
    /// is the classic behaviour, and there is nothing to stay level with.
    pub center_line: Option<f32>,
    /// The bar runs down the side of the screen rather than across the top.
    ///
    /// The layout is written once, along the bar, and only becomes x or y here.
    /// The icons themselves are never rotated — a sideways dock is a column of
    /// upright icons, not a row turned on its side.
    pub vertical: bool,
    /// The screen edge the bar sits against is at coordinate zero across the
    /// surface — true for a bar at the top or on the left. Running indicators
    /// go on that side, between the icon and the edge, whichever way round it
    /// is.
    pub edge_at_zero: bool,
}

/// Draw a frame. `pixmap` is the whole surface, which is wider than the dock.
pub fn draw(pixmap: &mut Pixmap, scene: &Scene, items: &mut [Item], icons: &mut IconCache) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);

    if let Some((left, right)) = scene.panel {
        let panel = if scene.flipped {
            Rect::from_ltrb(left, scene.baseline, right, scene.baseline + scene.panel_height)
        } else {
            Rect::from_ltrb(left, scene.baseline - scene.panel_height, right, scene.baseline)
        };
        if let Some(panel) = panel {
            draw_panel(pixmap, scene, panel);
        }
    }

    for item in items.iter() {
        draw_icon(pixmap, scene, item, icons);
    }
}

fn draw_panel(pixmap: &mut Pixmap, scene: &Scene, panel: Rect) {
    let theme = scene.theme;
    let radius = theme.corner_radius.min(panel.height() / 2.0);

    // A soft shadow, built from concentric rounded rectangles. Cheaper than a
    // real blur and, under a dock, indistinguishable from one.
    if theme.shadow > 0.0 {
        let steps = 10;
        for step in (1..=steps).rev() {
            let spread = theme.shadow * step as f32 / steps as f32;
            let alpha = 0.05 * (1.0 - step as f32 / steps as f32);
            let (up, down) = if scene.flipped {
                (spread * 0.3, spread * 0.6)
            } else {
                (spread * 0.6, spread * 0.3)
            };
            let Some(rect) = Rect::from_ltrb(
                panel.left() - spread,
                panel.top() - up,
                panel.right() + spread,
                panel.bottom() + down,
            ) else {
                continue;
            };
            fill_rounded(
                pixmap,
                rect,
                radius + spread,
                tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, alpha).unwrap_or(tiny_skia::Color::TRANSPARENT),
            );
        }
    }

    // The panel itself. Without glass it is the same colour, fully opaque —
    // the blur region is what makes the difference, not the paint.
    let background = if scene.glass {
        theme.background
    } else {
        theme.background.with_alpha(255)
    };
    fill_rounded(pixmap, panel, radius, background.to_skia());

    // A hairline along the top edge. This, more than the blur, is what reads as
    // glass: a lit edge where the surface catches the light.
    let edge_y = if scene.flipped {
        panel.bottom() - 1.0
    } else {
        panel.top()
    };
    if let Some(edge) = Rect::from_ltrb(
        panel.left() + radius * 0.5,
        edge_y,
        panel.right() - radius * 0.5,
        edge_y + 1.0,
    ) {
        fill_rounded(pixmap, edge, 0.5, theme.highlight.to_skia());
    }

    if let Some(outline) = theme.outline {
        // The glow is the outline drawn again, wider and fainter each time.
        if theme.glow > 0.0 {
            let steps = 6;
            for step in (1..=steps).rev() {
                let width = theme.glow * step as f32 / steps as f32;
                let alpha = (outline.3 as f32 / 255.0) * 0.09 * (1.0 - step as f32 / steps as f32);
                stroke_rounded(
                    pixmap,
                    panel,
                    radius,
                    width * 2.0,
                    tiny_skia::Color::from_rgba(
                        outline.0 as f32 / 255.0,
                        outline.1 as f32 / 255.0,
                        outline.2 as f32 / 255.0,
                        alpha,
                    )
                    .unwrap_or(tiny_skia::Color::TRANSPARENT),
                );
            }
        }
        stroke_rounded(pixmap, panel, radius, 1.0, outline.to_skia());
    }
}

fn draw_icon(pixmap: &mut Pixmap, scene: &Scene, item: &Item, icons: &mut IconCache) {
    let size = item.placed.size;
    if size <= 0.0 {
        return;
    }

    let room = indicator_room(scene.indicator, scene.theme);
    let (left, top) = icon_origin(scene, item.placed, size, room);

    if item.running {
        draw_indicator(pixmap, scene, item);
    }

    let rasterised = icons.size() as f32;
    let Some(icon) = icons.get(item.icon) else {
        return;
    };

    // Scaled down from the one rasterisation, never up past it.
    let scale = size / rasterised;
    pixmap.draw_pixmap(
        0,
        0,
        icon.as_ref(),
        &PixmapPaint {
            quality: tiny_skia::FilterQuality::Bicubic,
            opacity: 1.0,
            blend_mode: BlendMode::SourceOver,
        },
        Transform::from_translate(left, top).pre_scale(scale, scale),
        None,
    );
}

/// Where an icon's top-left corner goes.
///
/// Along the bar it is wherever the layout put it. Across the bar it normally
/// sits `room` away from the baseline, on whichever side of it the screen is
/// not — that is a dock growing away from the screen edge. Centre-locked, it
/// grows equally in both directions about a fixed line instead, which is what
/// keeps a row inside a panel level with the applets beside it.
fn icon_origin(scene: &Scene, placed: Placed, size: f32, room: f32) -> (f32, f32) {
    let across = cross_offset(scene, size, room);
    if scene.vertical {
        (across, placed.left())
    } else {
        (placed.left(), across)
    }
}

/// The icon's offset across the bar: its top edge on a horizontal bar, its left
/// edge on a vertical one.
fn cross_offset(scene: &Scene, size: f32, room: f32) -> f32 {
    match scene.center_line {
        Some(center) => center - size / 2.0,
        None if scene.flipped => scene.baseline + room,
        None => scene.baseline - room - size,
    }
}

/// How much room under the icons the running indicator needs.
fn indicator_room(indicator: Indicator, _theme: &Theme) -> f32 {
    match indicator {
        Indicator::None => 0.0,
        Indicator::Filled => 0.0,
        _ => 7.0,
    }
}

fn draw_indicator(pixmap: &mut Pixmap, scene: &Scene, item: &Item) {
    let colour = scene.theme.indicator.to_skia();
    // The indicator grows a little with its icon, so it does not look pinned in
    // place while everything around it moves.
    let grown = 1.0 + item.emphasis * 0.6;

    // Pinned to the bar's own edge rather than to the icon. At full
    // magnification an icon fills the whole thickness of the bar, so an
    // indicator placed just outside it would be off the surface and simply not
    // drawn — which is worse than one that does not move.
    let (thickness, along_size) = if scene.vertical {
        (pixmap.width() as f32, pixmap.height() as f32)
    } else {
        (pixmap.height() as f32, pixmap.width() as f32)
    };
    let _ = along_size;
    let along = item.placed.center;

    let (mut near, mut far) = match scene.indicator {
        Indicator::None => return,
        Indicator::Dot => (3.0, 3.0 + 5.2 * grown),
        Indicator::Line | Indicator::Underline => (3.0, 5.5),
        Indicator::Glow => (2.0, 2.0 + 9.0 * grown),
        Indicator::Filled => (0.0, thickness),
    };
    if !scene.edge_at_zero {
        // The screen edge is at the far side of the surface, so measure from
        // there instead.
        let (a, b) = (thickness - far, thickness - near);
        near = a;
        far = b;
    }

    let half = match scene.indicator {
        Indicator::Dot | Indicator::Glow => (2.6 * grown).max(1.0),
        Indicator::Line => 8.0 * grown,
        Indicator::Underline | Indicator::Filled => item.placed.size / 2.0 * 0.8,
        Indicator::None => return,
    };

    let rect = if scene.vertical {
        Rect::from_ltrb(near, along - half, far, along + half)
    } else {
        Rect::from_ltrb(along - half, near, along + half, far)
    };
    let Some(rect) = rect else {
        return;
    };

    let radius = (far - near).min(half * 2.0) / 2.0;
    if scene.indicator == Indicator::Glow {
        let steps = 4;
        for step in (1..=steps).rev() {
            let spread = 2.0 * step as f32;
            let alpha = 0.14 * (1.0 - step as f32 / steps as f32);
            let Some(wide) = Rect::from_ltrb(
                rect.left() - spread,
                rect.top() - spread,
                rect.right() + spread,
                rect.bottom() + spread,
            ) else {
                continue;
            };
            fill_rounded(
                pixmap,
                wide,
                radius + spread,
                tiny_skia::Color::from_rgba(colour.red(), colour.green(), colour.blue(), alpha)
                    .unwrap_or(tiny_skia::Color::TRANSPARENT),
            );
        }
    }
    fill_rounded(pixmap, rect, radius, colour);
}

fn rounded_rect(rect: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    if radius <= 0.01 {
        return PathBuilder::from_rect(rect).into();
    }

    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    // Circular arcs approximated with cubics; 0.5523 is the usual constant.
    let k = radius * 0.5523;

    let mut path = PathBuilder::new();
    path.move_to(l + radius, t);
    path.line_to(r - radius, t);
    path.cubic_to(r - radius + k, t, r, t + radius - k, r, t + radius);
    path.line_to(r, b - radius);
    path.cubic_to(r, b - radius + k, r - radius + k, b, r - radius, b);
    path.line_to(l + radius, b);
    path.cubic_to(l + radius - k, b, l, b - radius + k, l, b - radius);
    path.line_to(l, t + radius);
    path.cubic_to(l, t + radius - k, l + radius - k, t, l + radius, t);
    path.close();
    path.finish()
}

pub fn fill_rounded(pixmap: &mut Pixmap, rect: Rect, radius: f32, colour: tiny_skia::Color) {
    let Some(path) = rounded_rect(rect, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(colour);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

pub fn stroke_rounded(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: f32,
    width: f32,
    colour: tiny_skia::Color,
) {
    let Some(path) = rounded_rect(rect, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(colour);
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Colour;

    fn theme() -> Theme {
        Theme {
            name: "Test".into(),
            background: Colour(0, 0, 0, 200),
            highlight: Colour(255, 255, 255, 30),
            outline: Some(Colour(255, 0, 0, 255)),
            glow: 6.0,
            indicator: Colour(255, 255, 255, 255),
            corner_radius: 12.0,
            shadow: 10.0,
        }
    }

    fn scene<'a>(theme: &'a Theme) -> Scene<'a> {
        Scene {
            theme,
            panel: Some((20.0, 180.0)),
            baseline: 90.0,
            panel_height: 70.0,
            glass: true,
            indicator: Indicator::Dot,
            flipped: false,
            center_line: None,
            vertical: false,
            edge_at_zero: false,
        }
    }

    #[test]
    fn centre_locking_grows_an_icon_in_both_directions() {
        // The applet needs symmetric growth to stay level with the applets
        // beside it. Baseline-anchored growth only ever moves one edge.
        let theme = theme();
        let mut scene = scene(&theme);
        scene.center_line = Some(50.0);

        let small = cross_offset(&scene, 20.0, 7.0);
        let big = cross_offset(&scene, 60.0, 7.0);
        assert!(big < small, "the icon did not grow upwards");
        assert!(big + 60.0 > small + 20.0, "the icon did not grow downwards");
        // Its centre did not move at all.
        assert!((small + 10.0 - 50.0).abs() < f32::EPSILON);
        assert!((big + 30.0 - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_vertical_bar_puts_the_row_down_the_screen_and_the_icon_across_it() {
        let theme = theme();
        let mut scene = scene(&theme);
        scene.center_line = Some(32.0);
        let placed = Placed {
            center: 200.0,
            size: 48.0,
        };

        scene.vertical = false;
        let (left, top) = icon_origin(&scene, placed, 48.0, 7.0);
        assert_eq!(left, placed.left(), "horizontal: the row runs along x");
        assert_eq!(top, 32.0 - 24.0);

        scene.vertical = true;
        let (left, top) = icon_origin(&scene, placed, 48.0, 7.0);
        assert_eq!(top, placed.left(), "vertical: the row runs along y");
        assert_eq!(left, 32.0 - 24.0);
    }

    #[test]
    fn without_centre_locking_the_icon_still_stands_on_the_baseline() {
        // The layer-shell behaviour has to survive: a dock at the bottom of the
        // screen grows upwards only, away from the edge.
        let theme = theme();
        let mut scene = scene(&theme);
        scene.center_line = None;
        scene.flipped = false;
        assert!((cross_offset(&scene, 20.0, 7.0) + 20.0 - (90.0 - 7.0)).abs() < f32::EPSILON);
        assert!((cross_offset(&scene, 60.0, 7.0) + 60.0 - (90.0 - 7.0)).abs() < f32::EPSILON);

        // Flipped, it hangs from the baseline and grows downwards.
        scene.flipped = true;
        assert!((cross_offset(&scene, 20.0, 7.0) - (90.0 + 7.0)).abs() < f32::EPSILON);
        assert!((cross_offset(&scene, 60.0, 7.0) - (90.0 + 7.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn a_frame_actually_puts_pixels_on_the_surface() {
        let theme = theme();
        let scene = scene(&theme);
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let mut icons = IconCache::new(48);
        let mut items = [Item {
            icon: None,
            placed: Placed {
                center: 100.0,
                size: 48.0,
            },
            emphasis: 0.0,
            running: true,
        }];

        draw(&mut pixmap, &scene, &mut items, &mut icons);

        // Inside the panel: something was drawn.
        let inside = pixmap.pixel(100, 60).unwrap();
        assert!(inside.alpha() > 0, "the panel should be visible");
        // Outside it: still transparent, so the rest of the surface stays
        // click-through and invisible.
        let outside = pixmap.pixel(2, 10).unwrap();
        assert_eq!(outside.alpha(), 0, "the surface outside the dock must stay clear");
    }

    #[test]
    fn the_minimal_theme_draws_no_panel_at_all() {
        let mut theme = theme();
        theme.background = Colour(0, 0, 0, 0);
        theme.outline = None;
        theme.glow = 0.0;
        theme.shadow = 0.0;
        theme.highlight = Colour(0, 0, 0, 0);

        let scene = scene(&theme);
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let mut icons = IconCache::new(48);

        draw(&mut pixmap, &scene, &mut [], &mut icons);

        assert!(
            (0..200).all(|x| pixmap.pixel(x, 60).unwrap().alpha() == 0),
            "a transparent theme must leave the surface empty"
        );
    }

    #[test]
    fn a_rounded_rect_never_takes_a_radius_bigger_than_itself() {
        let rect = Rect::from_ltrb(0.0, 0.0, 10.0, 4.0).unwrap();
        // Would produce a self-intersecting path if the radius were obeyed.
        assert!(rounded_rect(rect, 50.0).is_some());
    }

    #[test]
    fn drawing_an_empty_dock_is_a_no_op_not_a_panic() {
        let theme = theme();
        let mut scene = scene(&theme);
        scene.panel = None;
        let mut pixmap = Pixmap::new(200, 100).unwrap();
        let mut icons = IconCache::new(48);
        draw(&mut pixmap, &scene, &mut [], &mut icons);
        assert_eq!(pixmap.pixel(100, 60).unwrap().alpha(), 0);
    }
}
