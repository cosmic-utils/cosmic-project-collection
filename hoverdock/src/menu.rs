//! The right-click menu on an icon.
//!
//! Deliberately the same shape as the one COSMIC's own app list offers, because
//! this applet stands in that slot: open it, run whatever actions the
//! application declares for itself, move it along the row, take it off the
//! dock. Anything more would be a different dock wearing the same clothes.

use tiny_skia::{Pixmap, Rect};

use crate::config::{Colour, Theme};
use crate::launchers::Launcher;
use crate::text::Text;

pub const ROW_HEIGHT: f32 = 32.0;
/// Space above the first row and below the last.
pub const PADDING: f32 = 6.0;
const SEPARATOR_HEIGHT: f32 = 9.0;
const TEXT_SIZE: f32 = 14.0;
const TEXT_INSET: f32 = 14.0;
const RADIUS: f32 = 12.0;
const MIN_WIDTH: f32 = 200.0;
const MAX_WIDTH: f32 = 420.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Raise one of the application's open windows.
    Focus(usize),
    /// Start the application.
    Launch,
    /// Run one of the application's own `[Desktop Action …]` entries.
    RunAction(usize),
    MoveLeft,
    MoveRight,
    /// Take it off the dock — which means out of COSMIC's favourites, so the
    /// change survives switching back to the stock app list.
    Unpin,
}

pub struct Row {
    pub label: String,
    pub action: Action,
    /// Draw a dividing line above this row.
    pub separator_before: bool,
}

/// The menu for one icon.
///
/// `index` and `count` are its place in the row: the ends do not get a "move
/// further that way" entry, because there is nowhere further to go.
pub fn rows(launcher: &Launcher, windows: &[String], index: usize, count: usize) -> Vec<Row> {
    // Open windows come first and without a heading. They are what you are
    // most likely to have opened the menu for, and a list of two windows does
    // not need a label explaining that it is a list of two windows.
    let mut rows: Vec<Row> = windows
        .iter()
        .enumerate()
        .map(|(i, title)| Row {
            label: title.clone(),
            action: Action::Focus(i),
            separator_before: false,
        })
        .collect();

    rows.push(Row {
        label: if windows.is_empty() {
            format!("Öppna {}", launcher.name)
        } else {
            format!("Nytt fönster: {}", launcher.name)
        },
        action: Action::Launch,
        separator_before: !windows.is_empty(),
    });

    for (i, action) in launcher.actions.iter().enumerate() {
        rows.push(Row {
            label: action.name.clone(),
            action: Action::RunAction(i),
            separator_before: false,
        });
    }

    let mut first_move = true;
    if index > 0 {
        rows.push(Row {
            label: "Flytta vänster".into(),
            action: Action::MoveLeft,
            separator_before: true,
        });
        first_move = false;
    }
    if index + 1 < count {
        rows.push(Row {
            label: "Flytta höger".into(),
            action: Action::MoveRight,
            separator_before: first_move,
        });
    }

    rows.push(Row {
        label: "Lossa från dockan".into(),
        action: Action::Unpin,
        separator_before: true,
    });
    rows
}

/// How big a tooltip holding `label` needs to be.
pub fn tooltip_size(label: &str, text: &mut Text) -> (u32, u32) {
    let width = text.width(label, TEXT_SIZE) + TOOLTIP_INSET * 2.0;
    (
        width.ceil().max(24.0) as u32,
        (TEXT_SIZE * 1.4 + TOOLTIP_INSET).ceil() as u32,
    )
}

const TOOLTIP_INSET: f32 = 10.0;

/// Draw a tooltip: just the name, on a plate dark enough to read over anything.
pub fn draw_tooltip(pixmap: &mut Pixmap, theme: &Theme, label: &str, text: &mut Text) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let width = pixmap.width() as f32;
    let height = pixmap.height() as f32;

    if let Some(rect) = Rect::from_ltrb(0.0, 0.0, width, height) {
        crate::render::fill_rounded(
            pixmap,
            rect,
            (height / 2.0).min(10.0),
            theme.background.with_alpha(242).to_skia(),
        );
        if let Some(outline) = theme.outline {
            crate::render::stroke_rounded(
                pixmap,
                rect,
                (height / 2.0).min(10.0),
                1.0,
                outline.to_skia(),
            );
        }
    }

    text.draw(
        pixmap,
        label,
        TOOLTIP_INSET,
        (height - TEXT_SIZE * 1.4) / 2.0,
        TEXT_SIZE,
        Colour(255, 255, 255, 240),
    );
}

/// The top edge of every row, and the menu's total height.
fn row_tops(rows: &[Row]) -> (Vec<f32>, f32) {
    let mut tops = Vec::with_capacity(rows.len());
    let mut y = PADDING;
    for row in rows {
        if row.separator_before {
            y += SEPARATOR_HEIGHT;
        }
        tops.push(y);
        y += ROW_HEIGHT;
    }
    (tops, y + PADDING)
}

/// How big a surface the menu needs.
///
/// Measured rather than guessed: an application whose name is "Extreme Tux
/// Racer" needs a wider menu than one called "Files", and a fixed width is
/// either too narrow for one or absurd for the other.
pub fn size(rows: &[Row], text: &mut Text) -> (u32, u32) {
    let (_, height) = row_tops(rows);
    let widest = rows
        .iter()
        .map(|row| text.width(&row.label, TEXT_SIZE))
        .fold(0.0f32, f32::max);
    let width = (widest + TEXT_INSET * 2.0).clamp(MIN_WIDTH, MAX_WIDTH);
    (width.ceil() as u32, height.ceil() as u32)
}

/// Which row is at a given y, if any. Separators belong to nothing.
pub fn hit(rows: &[Row], y: f32) -> Option<usize> {
    let (tops, _) = row_tops(rows);
    tops.iter()
        .position(|top| y >= *top && y < *top + ROW_HEIGHT)
}

pub fn draw(pixmap: &mut Pixmap, theme: &Theme, rows: &[Row], hovered: Option<usize>, text: &mut Text) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);

    let width = pixmap.width() as f32;
    let height = pixmap.height() as f32;
    let (tops, _) = row_tops(rows);

    // A menu is not glass: it has to be readable over whatever is behind it.
    let background = theme.background.with_alpha(246);
    if let Some(rect) = Rect::from_ltrb(0.0, 0.0, width, height) {
        crate::render::fill_rounded(pixmap, rect, RADIUS, background.to_skia());
        if let Some(outline) = theme.outline {
            crate::render::stroke_rounded(pixmap, rect, RADIUS, 1.0, outline.to_skia());
        }
    }

    let label_colour = Colour(255, 255, 255, 235);
    for (index, (row, top)) in rows.iter().zip(tops.iter()).enumerate() {
        if row.separator_before {
            let y = top - SEPARATOR_HEIGHT / 2.0;
            if let Some(rect) = Rect::from_ltrb(TEXT_INSET, y, width - TEXT_INSET, y + 1.0) {
                crate::render::fill_rounded(pixmap, rect, 0.5, theme.highlight.to_skia());
            }
        }

        if hovered == Some(index) {
            if let Some(rect) = Rect::from_ltrb(
                PADDING,
                *top,
                width - PADDING,
                top + ROW_HEIGHT,
            ) {
                crate::render::fill_rounded(
                    pixmap,
                    rect,
                    8.0,
                    Colour(255, 255, 255, 28).to_skia(),
                );
            }
        }

        // cosmic-text draws from the top of the line box, so centring the row
        // means offsetting by half of what is left over.
        let text_y = top + (ROW_HEIGHT - TEXT_SIZE * 1.4) / 2.0;
        text.draw(
            pixmap,
            &row.label,
            TEXT_INSET,
            text_y,
            TEXT_SIZE,
            label_colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launchers::{DesktopAction, Launcher};

    fn launcher(actions: usize) -> Launcher {
        Launcher {
            id: "test".into(),
            name: "Test".into(),
            icon: None,
            exec: "true".into(),
            startup_class: None,
            terminal: false,
            path: std::path::PathBuf::from("/dev/null"),
            actions: (0..actions)
                .map(|i| DesktopAction {
                    name: format!("Action {i}"),
                    exec: "true".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_ends_of_the_row_cannot_move_further_out() {
        let only = rows(&launcher(0), &[], 0, 1);
        assert!(!only.iter().any(|r| r.action == Action::MoveLeft));
        assert!(!only.iter().any(|r| r.action == Action::MoveRight));

        let first = rows(&launcher(0), &[], 0, 3);
        assert!(!first.iter().any(|r| r.action == Action::MoveLeft));
        assert!(first.iter().any(|r| r.action == Action::MoveRight));

        let last = rows(&launcher(0), &[], 2, 3);
        assert!(last.iter().any(|r| r.action == Action::MoveLeft));
        assert!(!last.iter().any(|r| r.action == Action::MoveRight));
    }

    #[test]
    fn an_applications_own_actions_are_offered_in_its_own_order() {
        let rows = rows(&launcher(2), &[], 1, 3);
        let actions: Vec<Action> = rows.iter().map(|r| r.action).collect();
        assert_eq!(actions[0], Action::Launch);
        assert_eq!(actions[1], Action::RunAction(0));
        assert_eq!(actions[2], Action::RunAction(1));
        assert_eq!(*actions.last().unwrap(), Action::Unpin);
    }

    #[test]
    fn open_windows_are_listed_first_and_can_each_be_picked() {
        let windows = ["Inkorg".to_string(), "Utkast".to_string()];
        let rows = rows(&launcher(1), &windows, 1, 3);
        let actions: Vec<Action> = rows.iter().map(|r| r.action).collect();

        assert_eq!(actions[0], Action::Focus(0));
        assert_eq!(actions[1], Action::Focus(1));
        assert_eq!(rows[0].label, "Inkorg");
        assert_eq!(rows[1].label, "Utkast");
        // With something already open, starting it again is a *new* window and
        // says so, rather than pretending nothing is running.
        assert_eq!(actions[2], Action::Launch);
        assert!(rows[2].label.starts_with("Nytt fönster"));
        assert!(rows[2].separator_before);
    }

    #[test]
    fn with_nothing_open_there_is_no_window_list_and_no_stray_separator() {
        let rows = rows(&launcher(0), &[], 0, 1);
        assert!(!rows.iter().any(|r| matches!(r.action, Action::Focus(_))));
        assert_eq!(rows[0].action, Action::Launch);
        assert!(!rows[0].separator_before);
        assert!(rows[0].label.starts_with("Öppna"));
    }

    #[test]
    fn every_row_can_be_hit_and_nothing_else_can() {
        let rows = rows(&launcher(2), &[], 1, 3);
        let (tops, height) = row_tops(&rows);

        for (index, top) in tops.iter().enumerate() {
            assert_eq!(hit(&rows, top + 1.0), Some(index));
            assert_eq!(hit(&rows, top + ROW_HEIGHT - 1.0), Some(index));
        }
        // The padding above the first row and below the last is not a row.
        assert_eq!(hit(&rows, 1.0), None);
        assert_eq!(hit(&rows, height - 1.0), None);
        // Neither is a separator, wherever one happens to fall.
        let divided = rows
            .iter()
            .position(|row| row.separator_before)
            .expect("this menu has a separator in it");
        assert_eq!(hit(&rows, tops[divided] - SEPARATOR_HEIGHT / 2.0), None);
    }

    #[test]
    fn a_long_name_widens_the_menu_but_only_so_far() {
        let mut text = Text::new();
        let mut short = launcher(0);
        short.name = "Filer".into();
        let mut long = launcher(0);
        long.name = "Extreme Tux Racer med ett orimligt långt namn".into();

        let (narrow, _) = size(&rows(&short, &[], 0, 1), &mut text);
        let (wide, _) = size(&rows(&long, &[], 0, 1), &mut text);
        assert!(wide > narrow);
        assert!(wide as f32 <= MAX_WIDTH);
        assert!(narrow as f32 >= MIN_WIDTH);
    }
}
