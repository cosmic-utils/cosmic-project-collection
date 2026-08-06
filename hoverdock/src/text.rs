//! Text, for the parts of the dock that have any.
//!
//! COSMIC's own text stack, so the menu is shaped and hinted the same way the
//! rest of the desktop is. The font system is expensive to build — it scans
//! every font on the machine — so it is created the first time something needs
//! text and never at startup. A dock that has not been right-clicked yet
//! should not pay for a font database.

use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache};
use tiny_skia::Pixmap;

pub struct Text {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Text {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    /// How wide a string will be at a given size.
    pub fn width(&mut self, text: &str, size: f32) -> f32 {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * 1.4));
        let mut buffer = buffer.borrow_with(&mut self.fonts);
        buffer.set_size(None, None);
        buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
        buffer.shape_until_scroll(false);
        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0f32, f32::max)
    }

    /// Draw a single line, its left edge at `x` and its baseline area starting
    /// at `y`.
    pub fn draw(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        colour: crate::config::Colour,
    ) {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * 1.4));
        {
            let mut shaping = buffer.borrow_with(&mut self.fonts);
            shaping.set_size(None, None);
            shaping.set_text(text, &Attrs::new(), Shaping::Advanced, None);
            shaping.shape_until_scroll(false);
        }

        let width = pixmap.width() as i32;
        let height = pixmap.height() as i32;
        let base = cosmic_text::Color::rgba(colour.0, colour.1, colour.2, colour.3);

        let mut buffer = buffer.borrow_with(&mut self.fonts);
        buffer.draw(
            &mut self.cache,
            base,
            |glyph_x, glyph_y, w, h, glyph_colour| {
                let alpha = glyph_colour.a();
                if alpha == 0 {
                    return;
                }
                for dy in 0..h as i32 {
                    for dx in 0..w as i32 {
                        let px = x as i32 + glyph_x + dx;
                        let py = y as i32 + glyph_y + dy;
                        if px < 0 || py < 0 || px >= width || py >= height {
                            continue;
                        }
                        blend(
                            pixmap,
                            px as u32,
                            py as u32,
                            glyph_colour.r(),
                            glyph_colour.g(),
                            glyph_colour.b(),
                            alpha,
                        );
                    }
                }
            },
        );
    }
}

/// Source-over one pixel. tiny-skia has no "set pixel", and a whole path per
/// glyph would cost far more than this.
fn blend(pixmap: &mut Pixmap, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    let index = (y * pixmap.width() + x) as usize;
    let Some(pixel) = pixmap.pixels_mut().get_mut(index) else {
        return;
    };

    let src_a = a as u32;
    let inv = 255 - src_a;
    // Everything here is premultiplied, which is what tiny-skia stores.
    let mix = |src: u8, dst: u8| -> u8 {
        (((src as u32 * src_a + 127) / 255) + ((dst as u32 * inv + 127) / 255)).min(255) as u8
    };

    let out = tiny_skia::PremultipliedColorU8::from_rgba(
        mix(r, pixel.red()),
        mix(g, pixel.green()),
        mix(b, pixel.blue()),
        (src_a + (pixel.alpha() as u32 * inv + 127) / 255).min(255) as u8,
    );
    if let Some(out) = out {
        *pixel = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Colour;

    #[test]
    fn text_actually_lands_on_the_pixmap() {
        let mut text = Text::new();
        let mut pixmap = Pixmap::new(200, 40).unwrap();
        text.draw(&mut pixmap, "Förstoring", 4.0, 4.0, 16.0, Colour(255, 255, 255, 255));

        let lit = pixmap.pixels().iter().filter(|p| p.alpha() > 0).count();
        assert!(lit > 20, "only {lit} pixels were drawn");
    }

    #[test]
    fn a_wider_string_measures_wider() {
        let mut text = Text::new();
        let short = text.width("Tema", 14.0);
        let long = text.width("Tema: Neon Purple", 14.0);
        assert!(short > 0.0);
        assert!(long > short);
    }

    #[test]
    fn drawing_outside_the_pixmap_is_clipped_not_a_panic() {
        let mut text = Text::new();
        let mut pixmap = Pixmap::new(20, 10).unwrap();
        text.draw(&mut pixmap, "för långt för att få plats", -50.0, -20.0, 20.0, Colour(255, 255, 255, 255));
        text.draw(&mut pixmap, "utanför", 500.0, 500.0, 20.0, Colour(255, 255, 255, 255));
    }
}
