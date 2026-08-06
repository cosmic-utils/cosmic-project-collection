//! What the host panel tells an applet about itself.
//!
//! cosmic-panel launches each applet with a handful of environment variables
//! describing the bar it is about to live in. Reading them is not optional
//! politeness: the icon size and the maximum thickness the panel will accept
//! are both derived from `COSMIC_PANEL_SIZE`, and an applet that picks its own
//! numbers is either a few pixels out of line with its neighbours or silently
//! clamped.
//!
//! The tables below are transcribed from `cosmic-panel-config`'s `PanelSize`.
//! They are duplicated rather than depended on because the panel's config crate
//! is not published — and because being wrong here is visible immediately, so a
//! copy that drifts is a copy that gets noticed.

/// The panel's size class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSize {
    Xs,
    S,
    M,
    L,
    Xl,
    Custom(u32),
}

impl PanelSize {
    /// Icon size in logical pixels for a full-colour (non-symbolic) applet icon.
    pub fn icon_size(self) -> f32 {
        match self {
            Self::Xs => 24.0,
            Self::S => 32.0,
            Self::M => 40.0,
            Self::L => 48.0,
            Self::Xl => 56.0,
            Self::Custom(s) => (Self::quantise(s) * 7 / 10) as f32,
        }
    }

    /// Padding the panel leaves around each applet icon.
    pub fn padding(self) -> f32 {
        match self {
            Self::Xs | Self::S => 4.0,
            Self::M | Self::L => 8.0,
            Self::Xl => 12.0,
            Self::Custom(s) => (Self::quantise(s) * 3 / 20) as f32,
        }
    }

    /// The thickest the panel will let its bar become, gap excluded.
    ///
    /// This is the ceiling on how far a magnified icon can grow: ask for more
    /// and `constrain_dim` quietly clamps the bar, cutting the icon off.
    pub fn max_thickness(self) -> f32 {
        match self {
            Self::Xs => 61.0,
            Self::S => 81.0,
            Self::M => 101.0,
            Self::L => 121.0,
            Self::Xl => 141.0,
            Self::Custom(s) => (Self::quantise(s) * 2 + 1) as f32,
        }
    }

    fn quantise(s: u32) -> u32 {
        s.max(16) / 4 * 4
    }

    /// Parse the RON the panel puts in `COSMIC_PANEL_SIZE` — `L`, `Custom(40)`.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        match raw {
            "XS" => Some(Self::Xs),
            "S" => Some(Self::S),
            "M" => Some(Self::M),
            "L" => Some(Self::L),
            "XL" => Some(Self::Xl),
            _ => {
                let inner = raw.strip_prefix("Custom(")?.strip_suffix(')')?;
                inner.trim().parse().ok().map(Self::Custom)
            }
        }
    }
}

/// Which edge the panel is on. A horizontal bar lays its applets out in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Bottom,
    Left,
    Right,
}

impl Anchor {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "Top" => Some(Self::Top),
            "Bottom" => Some(Self::Bottom),
            "Left" => Some(Self::Left),
            "Right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// Everything the panel said, with defaults for anything it did not.
#[derive(Debug, Clone, Copy)]
pub struct Host {
    pub size: PanelSize,
    pub anchor: Anchor,
    pub spacing: f32,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            size: PanelSize::M,
            anchor: Anchor::Bottom,
            spacing: 4.0,
        }
    }
}

impl Host {
    /// Read the environment cosmic-panel set up for us.
    ///
    /// Missing variables are not an error. Running the binary by hand, outside
    /// a panel, should still produce something sane rather than a crash.
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            size: std::env::var("COSMIC_PANEL_SIZE")
                .ok()
                .and_then(|raw| PanelSize::parse(&raw))
                .unwrap_or(default.size),
            anchor: std::env::var("COSMIC_PANEL_ANCHOR")
                .ok()
                .and_then(|raw| Anchor::parse(&raw))
                .unwrap_or(default.anchor),
            spacing: std::env::var("COSMIC_PANEL_SPACING")
                .ok()
                .and_then(|raw| raw.trim().parse::<f32>().ok())
                .unwrap_or(default.spacing),
        }
    }

    /// True when we were actually launched by cosmic-panel.
    pub fn inside_panel() -> bool {
        std::env::var_os("COSMIC_PANEL_NAME").is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_named_sizes_parse() {
        assert_eq!(PanelSize::parse("L"), Some(PanelSize::L));
        assert_eq!(PanelSize::parse(" XL "), Some(PanelSize::Xl));
        assert_eq!(PanelSize::parse("Custom(40)"), Some(PanelSize::Custom(40)));
        assert_eq!(PanelSize::parse("nonsense"), None);
    }

    #[test]
    fn a_custom_size_matches_the_panels_own_arithmetic() {
        // cosmic-panel quantises to a multiple of four before doing anything.
        assert_eq!(PanelSize::Custom(42).icon_size(), (40 * 7 / 10) as f32);
        assert_eq!(PanelSize::Custom(42).padding(), (40 * 3 / 20) as f32);
        assert_eq!(PanelSize::Custom(42).max_thickness(), 81.0);
        // Below the floor everything pins to 16.
        assert_eq!(PanelSize::Custom(1).icon_size(), (16 * 7 / 10) as f32);
    }

    #[test]
    fn there_is_room_to_magnify_at_every_size() {
        // If this ever fails the applet cannot grow an icon at all without the
        // panel clamping the bar, and the whole feature is off the table.
        for size in [
            PanelSize::Xs,
            PanelSize::S,
            PanelSize::M,
            PanelSize::L,
            PanelSize::Xl,
            PanelSize::Custom(40),
        ] {
            let resting = size.icon_size() + size.padding() * 2.0;
            assert!(
                size.max_thickness() > resting * 1.4,
                "{size:?}: {resting} at rest, ceiling {}",
                size.max_thickness()
            );
        }
    }

    #[test]
    fn anchors_parse_and_know_their_orientation() {
        assert!(Anchor::parse("Top").unwrap().is_horizontal());
        assert!(!Anchor::parse("Left").unwrap().is_horizontal());
        assert_eq!(Anchor::parse("Sideways"), None);
    }
}
