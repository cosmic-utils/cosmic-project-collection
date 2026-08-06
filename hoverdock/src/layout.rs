//! Where every icon sits, and how big it is right now.
//!
//! This is the signature feature and it is deliberately pure: no Wayland, no
//! drawing, no clock. The magnification curve and the spring are the two things
//! that decide whether the dock feels premium or cheap, so they are the two
//! things that have to be testable.
//!
//! The cursor never "snaps" to an icon. Both the scale of every icon and the
//! position of the whole strip are continuous functions of the pointer's x
//! position, which is what stops the row from twitching as the pointer crosses
//! the boundary between two icons.

/// A critically-ish damped spring, stepped by real elapsed time.
///
/// Time is passed in rather than read, so a frame that took 20 ms and a frame
/// that took 7 ms settle at the same rate — the animation is tied to the clock,
/// not to the frame rate.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    velocity: f32,
    stiffness: f32,
    damping: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        // Slightly under critical damping (2·√k ≈ 34.6): enough overshoot to
        // read as springy, not enough to wobble.
        Self {
            value,
            velocity: 0.0,
            stiffness: 300.0,
            damping: 26.0,
        }
    }

    /// Move towards `target`. Returns true while it is still moving, so the
    /// caller knows whether to ask for another frame.
    pub fn step(&mut self, target: f32, dt: f32) -> bool {
        // A frame we missed entirely must not launch the icon into orbit.
        let dt = dt.clamp(0.0, 1.0 / 30.0);

        let displacement = self.value - target;
        let acceleration = -self.stiffness * displacement - self.damping * self.velocity;
        self.velocity += acceleration * dt;
        self.value += self.velocity * dt;

        let settled = displacement.abs() < 0.001 && self.velocity.abs() < 0.001;
        if settled {
            self.value = target;
            self.velocity = 0.0;
        }
        !settled
    }

    /// Drop it exactly on target, for a dock that has just been built.
    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
    }
}

/// Geometry of the icon row, in logical pixels.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub icon_size: f32,
    pub spacing: f32,
    /// How much the icon under the pointer grows, e.g. 1.8.
    pub magnification: f32,
    /// How far the pointer's influence reaches, counted in icon widths.
    pub reach: f32,
    /// Space between the icons and the edge of the dock's background.
    pub padding: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            icon_size: 58.0,
            spacing: 10.0,
            magnification: 1.8,
            reach: 2.5,
            padding: 8.0,
        }
    }
}

/// One icon, placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// Horizontal centre, in surface coordinates.
    pub center: f32,
    /// Side length after magnification.
    pub size: f32,
}

impl Placed {
    pub fn left(&self) -> f32 {
        self.center - self.size / 2.0
    }

    pub fn contains(&self, x: f32) -> bool {
        x >= self.left() && x <= self.left() + self.size
    }
}

impl Metrics {
    /// Width of the icon row when nothing is hovered.
    pub fn resting_width(&self, count: usize) -> f32 {
        if count == 0 {
            return 0.0;
        }
        count as f32 * self.icon_size + (count - 1) as f32 * self.spacing
    }

    /// The tallest an icon can get — the surface has to be this tall plus
    /// padding, or a magnified icon would be cut off at the top.
    pub fn max_icon_size(&self) -> f32 {
        self.icon_size * self.magnification
    }

    /// How big each icon wants to be, given where the pointer is.
    ///
    /// `cursor` is in the same coordinates as the row; `None` means the pointer
    /// has left, and everything falls back to its resting size.
    pub fn target_scales(&self, count: usize, cursor: Option<f32>, row_center: f32) -> Vec<f32> {
        let Some(cursor) = cursor else {
            return vec![1.0; count];
        };

        let start = row_center - self.resting_width(count) / 2.0;
        let stride = self.icon_size + self.spacing;
        let reach = (self.reach * stride).max(1.0);

        (0..count)
            .map(|i| {
                let resting_center = start + i as f32 * stride + self.icon_size / 2.0;
                let distance = (cursor - resting_center).abs() / reach;
                1.0 + (self.magnification - 1.0) * falloff(distance)
            })
            .collect()
    }

    /// Lay the row out for a given set of scales.
    ///
    /// The row grows as icons do, and it grows *around the pointer*: whatever
    /// fraction along the row the pointer was over stays under the pointer.
    /// Without that the icons would slide out from under the cursor as they
    /// grew, which is the single most common way a dock feels wrong.
    pub fn place(&self, scales: &[f32], cursor: Option<f32>, row_center: f32) -> Vec<Placed> {
        let count = scales.len();
        if count == 0 {
            return Vec::new();
        }

        let widths: Vec<f32> = scales.iter().map(|s| self.icon_size * s).collect();
        let total: f32 = widths.iter().sum::<f32>() + (count - 1) as f32 * self.spacing;

        let start = match cursor {
            Some(cursor) => self.anchor(&widths, cursor, row_center),
            None => row_center - total / 2.0,
        };

        let mut placed = Vec::with_capacity(count);
        let mut x = start;
        for width in widths {
            placed.push(Placed {
                center: x + width / 2.0,
                size: width,
            });
            x += width + self.spacing;
        }
        placed
    }

    /// Where the grown row has to start for the pointer to stay exactly where
    /// it was — not merely on the same icon, but on the same *spot* on it.
    ///
    /// Walking the row element by element and mapping the pointer's distance
    /// along the resting row onto the same distance along the grown one is the
    /// only formulation that is both exact and continuous. Preserving the
    /// pointer's fraction of the whole row instead is a tempting one-liner, and
    /// it is wrong: icons near the pointer grow more than the ones at the ends,
    /// so the same fraction lands on a different icon once they have.
    fn anchor(&self, widths: &[f32], cursor: f32, row_center: f32) -> f32 {
        let resting = self.resting_width(widths.len());
        let resting_start = row_center - resting / 2.0;
        let along = cursor - resting_start;

        // Past either end there is no icon under the pointer to hold in place,
        // and the row must stay where it is instead of chasing the cursor off
        // the screen. Both branches meet the walk below exactly at the edges,
        // so nothing jumps as the pointer arrives.
        if along <= 0.0 {
            return resting_start;
        }
        let total: f32 = widths.iter().sum::<f32>()
            + (widths.len().saturating_sub(1)) as f32 * self.spacing;
        if along >= resting {
            return resting_start + resting - total;
        }

        let mut rested = 0.0;
        let mut grown = 0.0;

        for (i, width) in widths.iter().enumerate() {
            if along <= rested + self.icon_size {
                let within = if self.icon_size > 0.0 {
                    (along - rested) / self.icon_size
                } else {
                    0.0
                };
                return cursor - (grown + within * width);
            }
            rested += self.icon_size;
            grown += width;

            if i + 1 < widths.len() {
                if along <= rested + self.spacing {
                    let within = if self.spacing > 0.0 {
                        (along - rested) / self.spacing
                    } else {
                        0.0
                    };
                    // Gaps keep their width, so this stretch maps one to one.
                    return cursor - (grown + within * self.spacing);
                }
                rested += self.spacing;
                grown += self.spacing;
            }
        }

        cursor - grown
    }

    /// Which icon is under `x`, if any.
    pub fn hit(&self, placed: &[Placed], x: f32) -> Option<usize> {
        placed.iter().position(|p| p.contains(x))
    }

    /// The background panel that sits behind a placed row.
    pub fn background(&self, placed: &[Placed]) -> Option<(f32, f32)> {
        let first = placed.first()?;
        let last = placed.last()?;
        Some((
            first.left() - self.padding,
            last.left() + last.size + self.padding,
        ))
    }
}

/// Raised cosine: 1 at the pointer, 0 at the edge of its reach, with zero
/// gradient at both ends so there is no visible seam where the effect stops.
fn falloff(distance: f32) -> f32 {
    if distance >= 1.0 {
        0.0
    } else {
        0.5 * (1.0 + (std::f32::consts::PI * distance).cos())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics {
            icon_size: 50.0,
            spacing: 10.0,
            magnification: 2.0,
            reach: 2.0,
            padding: 8.0,
        }
    }

    #[test]
    fn nothing_is_magnified_without_a_pointer() {
        let scales = metrics().target_scales(5, None, 500.0);
        assert_eq!(scales, vec![1.0; 5]);
    }

    #[test]
    fn the_icon_under_the_pointer_grows_most_and_its_neighbours_a_little() {
        let m = metrics();
        // Dead centre of the middle icon of five.
        let scales = m.target_scales(5, Some(500.0), 500.0);

        assert!((scales[2] - 2.0).abs() < 0.001, "hovered icon is at full size");
        assert!(scales[1] > 1.0 && scales[1] < scales[2], "neighbour grows less");
        assert!(scales[3] > 1.0 && scales[3] < scales[2]);
        assert!(scales[1] > scales[0], "the effect falls off with distance");
        // Symmetric around the pointer.
        assert!((scales[1] - scales[3]).abs() < 0.001);
    }

    #[test]
    fn the_effect_ends_smoothly_instead_of_stopping_at_an_edge() {
        assert_eq!(falloff(1.0), 0.0);
        assert_eq!(falloff(2.0), 0.0);
        assert!((falloff(0.0) - 1.0).abs() < f32::EPSILON);
        // Nearly flat where it meets zero — no visible seam.
        assert!(falloff(0.98) < 0.002);
    }

    #[test]
    fn the_row_stays_centred_when_nothing_is_hovered() {
        let m = metrics();
        let placed = m.place(&[1.0; 4], None, 500.0);
        let left = placed.first().unwrap().left();
        let right = placed.last().unwrap().left() + placed.last().unwrap().size;
        assert!(((left + right) / 2.0 - 500.0).abs() < 0.001);
    }

    /// The one that matters: an icon must not slide out from under the pointer
    /// while it grows.
    #[test]
    fn the_icon_under_the_pointer_stays_under_the_pointer() {
        let m = metrics();
        let row_center = 500.0;

        // Sweep the whole row. Positions that land in a gap at rest have no
        // icon to stay on, so they are skipped rather than asserted about.
        let mut checked = 0;
        for step in 0..3000 {
            let cursor = 340.0 + step as f32 / 10.0;
            let resting = m.place(&[1.0; 5], Some(cursor), row_center);
            let Some(hovered) = m.hit(&resting, cursor) else {
                continue;
            };

            let scales = m.target_scales(5, Some(cursor), row_center);
            let grown = m.place(&scales, Some(cursor), row_center);

            // A hundredth of a pixel of slack: exactly on the seam between two
            // icons, which side wins is a rounding decision, not a behaviour.
            let icon = grown[hovered];
            assert!(
                cursor >= icon.left() - 0.01 && cursor <= icon.left() + icon.size + 0.01,
                "cursor {cursor} fell off icon {hovered} once it grew"
            );
            checked += 1;
        }
        assert!(checked > 2000, "the sweep should have covered the icons");
    }

    #[test]
    fn the_row_grows_but_never_jumps() {
        let m = metrics();
        let row_center = 500.0;

        // Walk the pointer across the row a pixel at a time; no icon may ever
        // move more than a couple of pixels between two steps.
        let mut previous: Option<Vec<Placed>> = None;
        for step in 0..300 {
            let cursor = 380.0 + step as f32;
            let scales = m.target_scales(5, Some(cursor), row_center);
            let placed = m.place(&scales, Some(cursor), row_center);

            if let Some(previous) = &previous {
                for (before, after) in previous.iter().zip(placed.iter()) {
                    assert!(
                        (before.center - after.center).abs() < 3.0,
                        "an icon jumped {} px at cursor {cursor}",
                        (before.center - after.center).abs()
                    );
                }
            }
            previous = Some(placed);
        }
    }

    #[test]
    fn the_spring_settles_and_then_stops_asking_for_frames() {
        let mut spring = Spring::new(1.0);
        let mut frames = 0;
        while spring.step(2.0, 1.0 / 60.0) {
            frames += 1;
            assert!(frames < 600, "the spring should settle within ten seconds");
        }
        assert!((spring.value - 2.0).abs() < 0.001);
        // Settled springs cost nothing, which is what keeps the dock at <1% CPU.
        assert!(!spring.step(2.0, 1.0 / 60.0));
        assert!(frames > 5, "instant is not an animation");
    }

    #[test]
    fn a_long_stall_cannot_launch_the_animation_into_orbit() {
        let mut spring = Spring::new(1.0);
        // The machine went to sleep for two seconds mid-animation.
        spring.step(2.0, 2.0);
        assert!(spring.value <= 2.5, "value ran away: {}", spring.value);
    }

    #[test]
    fn hit_testing_follows_the_grown_icons() {
        let m = metrics();
        let placed = m.place(&[1.0, 2.0, 1.0], Some(500.0), 500.0);
        let middle = placed[1];
        assert_eq!(m.hit(&placed, middle.center), Some(1));
        assert_eq!(m.hit(&placed, middle.left() - 6.0), None, "the gap is not an icon");
    }

    #[test]
    fn an_empty_dock_does_not_panic() {
        let m = metrics();
        assert_eq!(m.resting_width(0), 0.0);
        assert!(m.place(&[], Some(10.0), 100.0).is_empty());
        assert_eq!(m.background(&[]), None);
    }
}
