//! Finding an application's icon and turning it into pixels.
//!
//! Icons are rasterised once, at the largest size the dock will ever draw them,
//! and scaled down from there. Re-rasterising an SVG on every frame of a
//! magnification would be the easiest way to miss the frame budget.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

pub struct IconCache {
    /// The size icons are rasterised at.
    size: u32,
    /// The desktop's configured icon theme (for example `Papirus-Dark`).
    theme: String,
    /// `None` means "we looked and there is nothing", which is worth caching
    /// too — an icon theme miss is not cheap.
    loaded: HashMap<String, Option<Pixmap>>,
    fallback: Option<Pixmap>,
}

impl IconCache {
    pub fn new(size: u32) -> Self {
        Self {
            size: size.max(16),
            theme: system_icon_theme(),
            loaded: HashMap::new(),
            fallback: None,
        }
    }

    /// The size icons are rasterised at.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Throw everything away — used when the scale factor changes and every
    /// icon has to be rasterised again to stay sharp.
    pub fn resize(&mut self, size: u32) {
        let size = size.max(16);
        if size != self.size {
            self.size = size;
            self.loaded.clear();
            self.fallback = None;
        }
    }

    /// The pixels for an icon name, or the generic fallback.
    pub fn get(&mut self, name: Option<&str>) -> Option<&Pixmap> {
        let key = name.unwrap_or_default().to_string();
        if !self.loaded.contains_key(&key) {
            let pixmap = name.and_then(|name| load(name, self.size, &self.theme));
            self.loaded.insert(key.clone(), pixmap);
        }

        // Load the fallback before borrowing the map, so the two borrows
        // never overlap.
        if self.loaded.get(&key).is_none_or(Option::is_none) {
            self.ensure_fallback();
            return self.fallback.as_ref();
        }
        self.loaded.get(&key).and_then(Option::as_ref)
    }

    fn ensure_fallback(&mut self) {
        if self.fallback.is_none() {
            self.fallback = ["application-x-executable", "application-default-icon"]
                .into_iter()
                .find_map(|name| load(name, self.size, &self.theme));
        }
    }
}

/// Look an icon up by name — or take it as a path, which is what Steam games
/// and a lot of third-party installers put in their entries.
fn load(name: &str, size: u32, theme: &str) -> Option<Pixmap> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let path = if name.starts_with('/') {
        let path = PathBuf::from(name);
        path.is_file().then_some(path)
    } else {
        lookup(name, size, theme)
    }?;

    let pixmap = rasterise(&path, size);
    if pixmap.is_none() {
        log::debug!("could not read the icon at {}", path.display());
    }
    pixmap
}

fn lookup(name: &str, size: u32, theme: &str) -> Option<PathBuf> {
    freedesktop_icons::lookup(name)
        .with_theme(theme)
        .with_size(size as u16)
        .with_scale(1)
        .with_cache()
        .find()
        .or_else(|| {
            freedesktop_icons::lookup(name)
                .with_theme(theme)
                .with_cache()
                .find()
        })
}

/// Read COSMIC's icon-theme setting first, then GTK's equivalent. The
/// freedesktop-icons crate otherwise defaults to hicolor even when the desktop
/// has another theme selected.
fn system_icon_theme() -> String {
    let cosmic_theme = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .and_then(|config| {
            std::fs::read_to_string(config.join("cosmic/com.system76.CosmicTk/v1/icon_theme")).ok()
        })
        .and_then(|value| parse_icon_theme(&value));

    cosmic_theme
        .or_else(freedesktop_icons::default_theme_gtk)
        .unwrap_or_else(|| "hicolor".to_string())
}

fn parse_icon_theme(value: &str) -> Option<String> {
    let theme = value.trim().trim_matches('"').trim();
    (!theme.is_empty()).then(|| theme.to_string())
}

fn rasterise(path: &Path, size: u32) -> Option<Pixmap> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("svgz") => {
            rasterise_svg(path, size)
        }
        _ => rasterise_bitmap(path, size),
    }
}

fn rasterise_svg(path: &Path, size: u32) -> Option<Pixmap> {
    let data = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;

    let source = tree.size();
    if source.width() <= 0.0 || source.height() <= 0.0 {
        return None;
    }

    // Fit inside the square without distorting a non-square icon.
    let scale = (size as f32 / source.width()).min(size as f32 / source.height());
    let mut pixmap = Pixmap::new(size, size)?;
    let offset_x = (size as f32 - source.width() * scale) / 2.0;
    let offset_y = (size as f32 - source.height() * scale) / 2.0;

    resvg::render(
        &tree,
        tiny_skia::Transform::from_translate(offset_x, offset_y).pre_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

fn rasterise_bitmap(path: &Path, size: u32) -> Option<Pixmap> {
    let image = image::open(path).ok()?.into_rgba8();
    let source = Pixmap::from_vec(
        premultiply(image.as_raw()),
        tiny_skia::IntSize::from_wh(image.width(), image.height())?,
    )?;

    if image.width() == size && image.height() == size {
        return Some(source);
    }

    // Scale into the square, keeping the aspect ratio.
    let scale = (size as f32 / image.width() as f32).min(size as f32 / image.height() as f32);
    let mut pixmap = Pixmap::new(size, size)?;
    let offset_x = (size as f32 - image.width() as f32 * scale) / 2.0;
    let offset_y = (size as f32 - image.height() as f32 * scale) / 2.0;

    pixmap.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &tiny_skia::PixmapPaint {
            quality: tiny_skia::FilterQuality::Bicubic,
            ..Default::default()
        },
        tiny_skia::Transform::from_translate(offset_x, offset_y).pre_scale(scale, scale),
        None,
    );
    Some(pixmap)
}

/// Straight RGBA in, premultiplied RGBA out — what tiny-skia stores.
fn premultiply(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for pixel in rgba.chunks_exact(4) {
        let alpha = pixel[3] as u32;
        for channel in &pixel[..3] {
            out.push(((*channel as u32 * alpha + 127) / 255) as u8);
        }
        out.push(alpha as u8);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplication_matches_what_tiny_skia_expects() {
        // Opaque pixels are untouched.
        assert_eq!(premultiply(&[10, 20, 30, 255]), vec![10, 20, 30, 255]);
        // Fully transparent pixels lose their colour.
        assert_eq!(premultiply(&[10, 20, 30, 0]), vec![0, 0, 0, 0]);
        // Half transparent white is half white.
        let half = premultiply(&[255, 255, 255, 128]);
        assert_eq!(half, vec![128, 128, 128, 128]);
        // And the result is always a valid pixmap: no channel above alpha.
        for pixel in premultiply(&[255, 128, 0, 64]).chunks_exact(4) {
            assert!(pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3]);
        }
    }

    #[test]
    fn a_missing_icon_does_not_panic() {
        let mut cache = IconCache::new(48);
        // Both of these may legitimately be None on a machine with no themes;
        // what matters is that neither blows up.
        let _ = cache.get(Some("definitely-not-an-icon-name-4471"));
        let _ = cache.get(None);
    }

    #[test]
    fn resizing_clears_what_was_rasterised_for_the_old_size() {
        let mut cache = IconCache::new(48);
        let _ = cache.get(Some("application-x-executable"));
        cache.resize(96);
        assert_eq!(cache.size(), 96);
        assert!(cache.loaded.is_empty(), "stale sizes would render blurry");
    }

    #[test]
    fn an_icon_size_is_never_absurd() {
        assert_eq!(IconCache::new(0).size(), 16);
        let mut cache = IconCache::new(64);
        cache.resize(1);
        assert_eq!(cache.size(), 16);
    }

    #[test]
    fn cosmic_icon_theme_value_is_parsed() {
        assert_eq!(
            parse_icon_theme("\"Papirus-Dark\"\n").as_deref(),
            Some("Papirus-Dark")
        );
        assert_eq!(parse_icon_theme("  hicolor  ").as_deref(), Some("hicolor"));
        assert_eq!(parse_icon_theme("\"\"\n"), None);
    }
}
