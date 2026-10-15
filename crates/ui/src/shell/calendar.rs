//! The month grid behind the `DateField` calendar popup: 6 weeks of 7 days,
//! Monday first, with the days outside the month, today, the selected date and
//! the days the field's bounds rule out marked. Pure date math over jiff; the
//! Slint glue is in [`super::format`].

use jiff::Span;
use jiff::civil::Date;

/// Cells in the grid: six weeks of seven days.
pub const CELLS: usize = 42;

/// One cell of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub date: Date,
    /// In the shown month (the others are dimmed).
    pub in_month: bool,
    pub today: bool,
    pub selected: bool,
    /// Inside `min ..= max` (the others cannot be picked).
    pub enabled: bool,
}

/// Whether `date` lies in `min ..= max` (a missing bound is open).
pub fn allowed(date: Date, min: Option<Date>, max: Option<Date>) -> bool {
    min.is_none_or(|min| date >= min) && max.is_none_or(|max| date <= max)
}

/// The first cell: the Monday on or before the 1st of `month`'s month.
pub fn grid_start(month: Date) -> Date {
    let first = month.first_of_month();
    let lead = i64::from(first.weekday().to_monday_zero_offset());
    first.checked_sub(Span::new().days(lead)).unwrap_or(first)
}

/// The six-week grid of the month containing `month`.
pub fn month_grid(
    month: Date,
    selected: Option<Date>,
    min: Option<Date>,
    max: Option<Date>,
    today: Date,
) -> Vec<Cell> {
    let start = grid_start(month);
    let (year, number) = (month.year(), month.month());
    start
        .series(Span::new().days(1))
        .take(CELLS)
        .map(|date| Cell {
            date,
            in_month: date.year() == year && date.month() == number,
            today: date == today,
            selected: Some(date) == selected,
            enabled: allowed(date, min, max),
        })
        .collect()
}

/// `date` moved by whole months, the day clamped to the target month's length
/// (31.01 + 1 month = 28.02 or 29.02); `date` itself when out of range.
pub fn add_months(date: Date, months: i32) -> Date {
    date.checked_add(Span::new().months(months)).unwrap_or(date)
}

/// `date` moved into `min ..= max`.
pub fn clamp(date: Date, min: Option<Date>, max: Option<Date>) -> Date {
    match (min, max) {
        (Some(min), _) if date < min => min,
        (_, Some(max)) if date > max => max,
        _ => date,
    }
}

/// Headless helpers for driving a `DateField` calendar in feature tests.
#[cfg(test)]
pub(crate) mod testing {
    use i_slint_backend_testing::{AccessibleRole, ElementHandle, ElementQuery, ElementRoot};
    use slint::SharedString;
    use slint::platform::{Key, PointerEventButton, WindowEvent};

    /// The `DateField` with element id `id` (e.g. "Filters::from").
    pub fn field(root: &impl ElementRoot, id: &str) -> ElementHandle {
        ElementHandle::find_by_element_id(root, id)
            .next()
            .unwrap_or_else(|| panic!("date field {id} not found"))
    }

    fn part(field: &ElementHandle, id: &str) -> ElementHandle {
        field
            .query_descendants()
            .match_id(id)
            .find_first()
            .unwrap_or_else(|| panic!("{id} not found in the date field"))
    }

    /// The calendar button at the right of the field.
    pub fn button(field: &ElementHandle) -> ElementHandle {
        part(field, "DateField::open")
    }

    /// The text box's value.
    pub fn text(field: &ElementHandle) -> String {
        part(field, "DateField::edit")
            .accessible_value()
            .unwrap_or_default()
            .to_string()
    }

    /// Opens the calendar through its button (the default action of a click).
    pub fn open(field: &ElementHandle) {
        button(field).invoke_accessible_default_action();
    }

    /// Clicks the calendar button with the mouse.
    pub fn click_button(field: &ElementHandle) {
        button(field).mock_single_click(PointerEventButton::Left);
    }

    fn in_popup(root: &impl ElementRoot, id: &str) -> Option<ElementHandle> {
        ElementQuery::from_root(root)
            .match_descendants()
            .match_id(id)
            .find_first()
    }

    /// An element of the open calendar by id ("DateField::next", ...).
    pub fn popup_part(root: &impl ElementRoot, id: &str) -> ElementHandle {
        in_popup(root, id).unwrap_or_else(|| panic!("{id} not found"))
    }

    /// Whether a calendar popup is open in `root`'s window.
    pub fn is_open(root: &impl ElementRoot) -> bool {
        in_popup(root, "DateField::month-title").is_some()
    }

    /// The open calendar's "Tetor 2026" title.
    pub fn title(root: &impl ElementRoot) -> String {
        in_popup(root, "DateField::month-title")
            .and_then(|t| t.accessible_label())
            .unwrap_or_default()
            .to_string()
    }

    /// The open calendar's day cell for `display` (dd.MM.yyyy).
    pub fn day(root: &impl ElementRoot, display: &str) -> Option<ElementHandle> {
        let display = SharedString::from(display);
        ElementQuery::from_root(root)
            .match_descendants()
            .match_id("DateField::day-cell")
            .match_predicate(move |e| e.accessible_label().as_ref() == Some(&display))
            .find_first()
    }

    /// The open calendar's day cells (42), in grid order. The search reaches a
    /// popup's items more than once, so repeats are dropped.
    pub fn days(root: &impl ElementRoot) -> Vec<ElementHandle> {
        let mut seen = Vec::new();
        let mut cells = Vec::new();
        for cell in ElementQuery::from_root(root)
            .match_descendants()
            .match_id("DateField::day-cell")
            .find_all()
        {
            let label = cell.accessible_label().unwrap_or_default();
            if !seen.contains(&label) {
                seen.push(label);
                cells.push(cell);
            }
        }
        cells
    }

    /// The open calendar's "Sot" button.
    pub fn today_button(root: &impl ElementRoot) -> ElementHandle {
        ElementQuery::from_root(root)
            .match_descendants()
            .match_id("DateField::today")
            .match_accessible_role(AccessibleRole::Button)
            .find_first()
            .expect("the Sot button")
    }

    /// Clicks a calendar element with the mouse.
    pub fn click(e: &ElementHandle) {
        e.mock_single_click(PointerEventButton::Left);
    }

    fn send(window: &slint::Window, text: impl Into<SharedString>, pressed: bool) {
        let text = text.into();
        window.dispatch_event(if pressed {
            WindowEvent::KeyPressed { text }
        } else {
            WindowEvent::KeyReleased { text }
        });
    }

    /// Presses and releases `key` (Alt held when `alt`).
    pub fn key(window: &slint::Window, key: Key, alt: bool) {
        if alt {
            send(window, Key::Alt, true);
        }
        send(window, key, true);
        send(window, key, false);
        if alt {
            send(window, Key::Alt, false);
        }
    }

    /// A mouse click at window position (x, y).
    pub fn click_at(window: &slint::Window, x: f32, y: f32) {
        let position = slint::LogicalPosition::new(x, y);
        window.dispatch_event(WindowEvent::PointerMoved { position });
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{Weekday, date};

    fn days(cells: &[Cell]) -> Vec<i8> {
        cells.iter().map(|c| c.date.day()).collect()
    }

    #[test]
    fn february_2028_is_a_leap_month_starting_on_tuesday() {
        let grid = month_grid(date(2028, 2, 17), None, None, None, date(2026, 10, 14));
        assert_eq!(grid.len(), CELLS);
        assert_eq!(grid[0].date, date(2028, 1, 31), "one leading Monday");
        assert!(!grid[0].in_month);
        assert_eq!(grid[1].date, date(2028, 2, 1));
        assert_eq!(grid[29].date, date(2028, 2, 29));
        assert!(grid[29].in_month);
        assert_eq!(grid[30].date, date(2028, 3, 1));
        assert!(!grid[30].in_month);
        assert_eq!(grid.iter().filter(|c| c.in_month).count(), 29);
        // A common year: 28 days.
        let common = month_grid(date(2027, 2, 1), None, None, None, date(2026, 10, 14));
        assert_eq!(common.iter().filter(|c| c.in_month).count(), 28);
        // 2100 is not a leap year, 2000 was.
        let feb = |y| {
            month_grid(date(y, 2, 1), None, None, None, date(2026, 1, 1))
                .iter()
                .filter(|c| c.in_month)
                .count()
        };
        assert_eq!((feb(2100), feb(2000)), (28, 29));
    }

    #[test]
    fn every_row_starts_on_monday() {
        for (y, m) in [
            (2026, 1),
            (2026, 3),
            (2026, 6),
            (2026, 10),
            (2026, 11),
            (2028, 2),
        ] {
            let grid = month_grid(date(y, m, 1), None, None, None, date(2026, 10, 14));
            for week in grid.chunks(7) {
                assert_eq!(week[0].date.weekday(), Weekday::Monday, "{y}-{m}");
                assert_eq!(week[6].date.weekday(), Weekday::Sunday, "{y}-{m}");
            }
            let first = grid.iter().position(|c| c.in_month).unwrap();
            assert_eq!(grid[first].date, date(y, m, 1));
            // Consecutive days.
            for pair in grid.windows(2) {
                assert_eq!(pair[0].date.tomorrow().unwrap(), pair[1].date);
            }
        }
    }

    #[test]
    fn a_month_starting_on_sunday_has_six_leading_days() {
        // 1 November 2026 and 1 March 2026 are Sundays.
        for month in [date(2026, 11, 1), date(2026, 3, 1)] {
            assert_eq!(month.weekday(), Weekday::Sunday);
            let grid = month_grid(month, None, None, None, date(2026, 10, 14));
            assert_eq!(grid.iter().position(|c| c.in_month), Some(6));
            assert_eq!(grid[6].date, month);
        }
        let nov = month_grid(date(2026, 11, 5), None, None, None, date(2026, 10, 14));
        assert_eq!(days(&nov[..7]), vec![26, 27, 28, 29, 30, 31, 1]);
        assert_eq!(nov[41].date, date(2026, 12, 6));
    }

    #[test]
    fn a_month_starting_on_monday_has_no_leading_days() {
        // 1 June 2026 is a Monday.
        let grid = month_grid(date(2026, 6, 30), None, None, None, date(2026, 10, 14));
        assert_eq!(grid[0].date, date(2026, 6, 1));
        assert!(grid[0].in_month);
        assert_eq!(grid[29].date, date(2026, 6, 30));
        assert_eq!(grid[30].date, date(2026, 7, 1));
        assert_eq!(grid[41].date, date(2026, 7, 12));
    }

    #[test]
    fn today_selected_and_bounds_are_marked() {
        let today = date(2026, 10, 14);
        let grid = month_grid(
            date(2026, 10, 1),
            Some(date(2026, 10, 20)),
            Some(date(2026, 10, 5)),
            Some(date(2026, 10, 25)),
            today,
        );
        let cell = |d: Date| *grid.iter().find(|c| c.date == d).unwrap();
        assert!(cell(today).today && !cell(today).selected);
        assert!(cell(date(2026, 10, 20)).selected);
        assert_eq!(grid.iter().filter(|c| c.today).count(), 1);
        assert_eq!(grid.iter().filter(|c| c.selected).count(), 1);
        assert!(!cell(date(2026, 10, 4)).enabled);
        assert!(cell(date(2026, 10, 5)).enabled, "the bounds are inclusive");
        assert!(cell(date(2026, 10, 25)).enabled);
        assert!(!cell(date(2026, 10, 26)).enabled);
        assert!(
            month_grid(date(2026, 10, 1), None, None, None, today)
                .iter()
                .all(|c| c.enabled)
        );
    }

    #[test]
    fn months_move_and_clamp() {
        assert_eq!(add_months(date(2026, 1, 31), 1), date(2026, 2, 28));
        assert_eq!(add_months(date(2028, 1, 31), 1), date(2028, 2, 29));
        assert_eq!(add_months(date(2026, 12, 15), 1), date(2027, 1, 15));
        assert_eq!(add_months(date(2026, 1, 15), -1), date(2025, 12, 15));
        assert_eq!(add_months(date(9999, 12, 1), 1), date(9999, 12, 1));
        let (lo, hi) = (Some(date(2026, 3, 1)), Some(date(2026, 3, 31)));
        assert_eq!(clamp(date(2026, 2, 1), lo, hi), date(2026, 3, 1));
        assert_eq!(clamp(date(2026, 4, 1), lo, hi), date(2026, 3, 31));
        assert_eq!(clamp(date(2026, 3, 9), lo, hi), date(2026, 3, 9));
        assert_eq!(clamp(date(2026, 4, 1), None, None), date(2026, 4, 1));
    }
}
