//! Configuration and themes.
//!
//! One TOML file, every value optional. A missing file is not an error — it is
//! a first run, and the defaults are chosen to look right without anyone
//! touching them.
//!
//! Everything the *panel* already decides — which edge it is on, which output,
//! whether it hides — is deliberately absent. An applet that disagreed with its
//! host about any of those would just look broken.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Indicator {
    None,
    Dot,
    Line,
    Glow,
    Underline,
    Filled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// How much the icon under the pointer grows.
    pub magnification: f32,
    /// Draw a rounded plate behind the icons.
    ///
    /// Off by default: the whole point of this applet is icons standing free on
    /// the desktop, and the panel already has a background of its own if one is
    /// wanted.
    pub plate: bool,
    /// Translucent plate rather than a solid one. Only read when `plate` is on.
    pub glass: bool,
    pub theme: String,
    /// Icon size at rest. Leave at 0 to follow the panel's own applet size, so
    /// the row lines up with the launcher and workspace buttons beside it.
    pub icon_size: f32,
    /// Extra pixels of surface, purely to give magnification somewhere to go.
    ///
    /// This is the one setting that costs something real. The panel sizes its
    /// bar to the thickest applet in it and reserves that much screen, so every
    /// pixel here is a pixel taken off every window on the display.
    ///
    /// At 0 the row occupies exactly what COSMIC's own app list did and the
    /// icons can only grow to about 1.3x, which is too timid to read as a dock
    /// effect at all. 16 buys roughly 1.6x — about what Plank does — for 16 px.
    /// 30 or so is enough for the full configured `magnification`.
    pub extra_height: f32,
    pub spacing: f32,
    /// How far the pointer's influence reaches, in icon widths.
    pub reach: f32,
    pub indicator: Indicator,
    /// Desktop entry ids, in the order they should appear. Empty means "use
    /// whatever COSMIC's own dock is pinned to", so it looks like the desktop
    /// it was dropped into.
    pub pinned: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            magnification: 1.8,
            plate: false,
            glass: true,
            theme: "Default Dark".to_string(),
            // 0 means "ask the panel". COSMIC_PANEL_SIZE is authoritative here:
            // guessing a size is how an applet ends up a few pixels taller than
            // its neighbours.
            icon_size: 0.0,
            extra_height: 16.0,
            spacing: 10.0,
            // 2.5 icon widths meant an icon started growing while the pointer
            // was still three icons away, which reads as the dock twitching at
            // everything. Just over one icon width is what feels deliberate.
            reach: 1.3,
            indicator: Indicator::Dot,
            pinned: Vec::new(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        config_home().join("hoverdock").join("config.toml")
    }

    /// Load the config, or the defaults if there is not one yet.
    ///
    /// A *broken* config is different from a missing one: it is reported, and
    /// the dock still starts, because a dock that refuses to appear leaves the
    /// user with no way to fix the file.
    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(raw) => match toml::from_str(&raw) {
                Ok(config) => config,
                Err(err) => {
                    log::error!("{}: {err}", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Write the current settings out, so there is something to edit.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok(())
    }

    /// `from_panel` is the icon size the panel says its applets use; it is what
    /// the row falls back to when the config does not override it.
    pub fn metrics(&self, from_panel: f32) -> crate::layout::Metrics {
        let icon_size = if self.icon_size > 0.0 {
            self.icon_size
        } else {
            from_panel
        };
        crate::layout::Metrics {
            icon_size: icon_size.max(16.0),
            spacing: self.spacing.max(0.0),
            magnification: self.magnification.clamp(1.0, 4.0),
            reach: self.reach.max(0.5),
            padding: (icon_size * 0.14).max(6.0),
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::by_name(&self.theme)
    }
}

/// Straight RGBA, 0–255, because that is what everything downstream wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colour(pub u8, pub u8, pub u8, pub u8);

impl Colour {
    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.0, self.1, self.2, self.3)
    }

    pub fn with_alpha(self, alpha: u8) -> Self {
        Self(self.0, self.1, self.2, alpha)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    /// The dock's own background.
    pub background: Colour,
    /// A hairline along the top edge — what sells "glass" more than blur does.
    pub highlight: Colour,
    /// Optional outline around the whole dock.
    pub outline: Option<Colour>,
    /// Glow around the outline, in pixels. 0 turns it off.
    pub glow: f32,
    /// Running-application indicators.
    pub indicator: Colour,
    pub corner_radius: f32,
    /// How far the shadow reaches under the dock.
    pub shadow: f32,
}

impl Theme {
    pub fn by_name(name: &str) -> Self {
        let wanted = name.trim().to_lowercase().replace([' ', '-', '_'], "");
        Self::builtin()
            .into_iter()
            .find(|theme| theme.name.to_lowercase().replace([' ', '-', '_'], "") == wanted)
            .unwrap_or_else(|| {
                if !name.trim().is_empty() {
                    log::warn!("unknown theme {name:?}, falling back to Default Dark");
                }
                Self::default_dark()
            })
    }

    pub fn builtin() -> Vec<Self> {
        vec![
            Self::default_dark(),
            Self::neon_purple(),
            Self::cyberpunk(),
            Self::glass(),
            Self::minimal(),
            Self::nord(),
            Self::catppuccin(),
        ]
    }

    fn default_dark() -> Self {
        Self {
            name: "Default Dark".into(),
            background: Colour(18, 18, 22, 170),
            highlight: Colour(255, 255, 255, 26),
            outline: Some(Colour(255, 255, 255, 20)),
            glow: 0.0,
            indicator: Colour(255, 255, 255, 220),
            corner_radius: 18.0,
            shadow: 24.0,
        }
    }

    fn neon_purple() -> Self {
        Self {
            name: "Neon Purple".into(),
            background: Colour(20, 10, 32, 175),
            highlight: Colour(214, 160, 255, 38),
            outline: Some(Colour(178, 92, 255, 190)),
            glow: 14.0,
            indicator: Colour(206, 140, 255, 235),
            corner_radius: 20.0,
            shadow: 28.0,
        }
    }

    fn cyberpunk() -> Self {
        Self {
            name: "Cyberpunk".into(),
            background: Colour(8, 12, 24, 180),
            highlight: Colour(0, 255, 214, 40),
            outline: Some(Colour(0, 240, 200, 200)),
            glow: 18.0,
            indicator: Colour(255, 60, 160, 240),
            corner_radius: 14.0,
            shadow: 30.0,
        }
    }

    fn glass() -> Self {
        Self {
            name: "Glass".into(),
            background: Colour(255, 255, 255, 40),
            highlight: Colour(255, 255, 255, 60),
            outline: Some(Colour(255, 255, 255, 46)),
            glow: 0.0,
            indicator: Colour(255, 255, 255, 230),
            corner_radius: 22.0,
            shadow: 20.0,
        }
    }

    fn minimal() -> Self {
        Self {
            name: "Minimal".into(),
            background: Colour(0, 0, 0, 0),
            highlight: Colour(0, 0, 0, 0),
            outline: None,
            glow: 0.0,
            indicator: Colour(255, 255, 255, 200),
            corner_radius: 0.0,
            shadow: 0.0,
        }
    }

    fn nord() -> Self {
        Self {
            name: "Nord".into(),
            background: Colour(46, 52, 64, 190),
            highlight: Colour(216, 222, 233, 30),
            outline: Some(Colour(136, 192, 208, 120)),
            glow: 0.0,
            indicator: Colour(163, 190, 140, 235),
            corner_radius: 16.0,
            shadow: 22.0,
        }
    }

    fn catppuccin() -> Self {
        Self {
            name: "Catppuccin".into(),
            background: Colour(30, 30, 46, 190),
            highlight: Colour(205, 214, 244, 28),
            outline: Some(Colour(203, 166, 247, 140)),
            glow: 8.0,
            indicator: Colour(203, 166, 247, 235),
            corner_radius: 18.0,
            shadow: 24.0,
        }
    }
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_example_parses() {
        let raw = r#"
magnification = 1.8
plate = false
glass = true
theme = "Cyberpunk"
icon_size = 48
spacing = 10
reach = 1.3
indicator = "none"
"#;
        let config: Config = toml::from_str(raw).expect("the documented config must parse");
        assert_eq!(config.magnification, 1.8);
        assert_eq!(config.theme, "Cyberpunk");
        assert!(!config.plate);
        assert_eq!(config.icon_size, 48.0);
    }

    #[test]
    fn a_zero_icon_size_defers_to_the_panel() {
        let config = Config {
            icon_size: 0.0,
            ..Config::default()
        };
        // Whatever the panel says, not some size of our own invention.
        assert_eq!(config.metrics(48.0).icon_size, 48.0);
        assert_eq!(config.metrics(32.0).icon_size, 32.0);

        // An explicit size still wins, so it can be overridden.
        let config = Config {
            icon_size: 40.0,
            ..Config::default()
        };
        assert_eq!(config.metrics(48.0).icon_size, 40.0);
    }

    #[test]
    fn an_empty_file_is_a_complete_config() {
        let config: Config = toml::from_str("").expect("empty is valid");
        assert_eq!(config.magnification, Config::default().magnification);
    }

    #[test]
    fn every_builtin_theme_can_be_asked_for_by_name() {
        for theme in Theme::builtin() {
            assert_eq!(Theme::by_name(&theme.name).name, theme.name);
        }
        // Spelling should not have to be exact.
        assert_eq!(Theme::by_name("neon purple").name, "Neon Purple");
        assert_eq!(Theme::by_name("neon-purple").name, "Neon Purple");
        assert_eq!(Theme::by_name("NEONPURPLE").name, "Neon Purple");
    }

    #[test]
    fn an_unknown_theme_falls_back_instead_of_failing() {
        assert_eq!(Theme::by_name("nonesuch").name, "Default Dark");
    }

    #[test]
    fn absurd_values_are_clamped_rather_than_obeyed() {
        let config = Config {
            magnification: 40.0,
            icon_size: 2.0,
            spacing: -10.0,
            ..Config::default()
        };
        let metrics = config.metrics(48.0);
        assert_eq!(metrics.magnification, 4.0);
        assert_eq!(metrics.icon_size, 16.0);
        assert_eq!(metrics.spacing, 0.0);
    }

    #[test]
    fn config_round_trips() {
        let config = Config::default();
        let raw = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&raw).unwrap();
        assert_eq!(back.theme, config.theme);
        assert_eq!(back.plate, config.plate);
        assert_eq!(back.indicator, config.indicator);
    }
}
