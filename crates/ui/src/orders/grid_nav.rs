//! Keyboard navigation of the line grid (plan 010 AC 13), as a pure function.
//!
//! Editable cells per row: sale `Artikulli | Sasia | Çmimi`, purchase
//! `Artikulli | Sasia | Çmimi i blerjes | Çmimi i shitjes` (columns 0..n). The
//! placeholder row (always the last) has the same cells. The order of the cells
//! for Tab/←/→ is row-major and wraps at both ends; ↑/↓ stay in the column and
//! wrap over the rows.

use crm_services::domain::OrderKind;

/// The Artikulli (product search) column.
pub const PRODUCT_COL: usize = 0;
/// The Sasia column.
pub const QTY_COL: usize = 1;
/// The first price column (sale "Çmimi", purchase "Çmimi i blerjes").
pub const PRICE_COL: usize = 2;
/// The purchase "Çmimi i shitjes" column.
pub const SELL_COL: usize = 3;

/// The shape of a grid: its kind (number of columns) and its rows, including
/// the placeholder row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridLayout {
    /// Shitje (3 editable columns) or Blerje (4).
    pub kind: OrderKind,
    /// Rows including the placeholder (≥ 1).
    pub rows: usize,
}

impl GridLayout {
    /// The editable columns of `kind`.
    pub const fn cols_of(kind: OrderKind) -> usize {
        match kind {
            OrderKind::Sale => 3,
            OrderKind::Purchase => 4,
        }
    }

    /// The editable columns.
    pub const fn cols(&self) -> usize {
        Self::cols_of(self.kind)
    }

    /// Whether `row` is the placeholder (last) row.
    pub const fn is_placeholder(&self, row: usize) -> bool {
        row + 1 == self.rows
    }
}

/// A cell position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    /// Row (0-based, the placeholder included).
    pub row: usize,
    /// Editable column (see the module docs).
    pub col: usize,
}

impl Pos {
    /// `Pos { row, col }`.
    pub const fn new(row: usize, col: usize) -> Pos {
        Pos { row, col }
    }
}

/// The keys the grid reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    /// Tab (Shift+Tab with the shift modifier).
    Tab,
    /// Shift+Tab as the platform's Backtab key.
    Backtab,
    /// →.
    Right,
    /// ←.
    Left,
    /// ↓.
    Down,
    /// ↑.
    Up,
    /// Enter / Return.
    Enter,
    /// Esc.
    Escape,
    /// Delete.
    Delete,
    /// Anything else (text input, editing keys).
    Other,
}

/// Modifier keys held with the key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
}

/// The product suggestion list of the focused Artikulli cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dropdown {
    /// Whether the list is shown.
    pub open: bool,
    /// Whether an item is highlighted.
    pub highlighted: bool,
}

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    /// Commit the cell and focus another one.
    Move(Pos),
    /// Commit, append an empty row and focus its Artikulli.
    AddRowFocusProduct,
    /// Choose the highlighted suggestion (then focus that row's Sasia).
    SelectSuggestion,
    /// Highlight the next suggestion (wrapping; none → first).
    HighlightNext,
    /// Highlight the previous suggestion (wrapping; none → last).
    HighlightPrev,
    /// Close the suggestion list.
    CloseDropdown,
    /// Remove the row.
    RemoveRow,
    /// Commit the value, stay in the cell.
    Commit,
    /// Not handled: the key goes to the text field.
    None,
}

fn step(index: usize, len: usize, forward: bool) -> usize {
    if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
    }
}

/// Moves along the row-major cell order.
fn linear(layout: GridLayout, pos: Pos, forward: bool) -> Pos {
    let cols = layout.cols();
    let len = layout.rows * cols;
    let next = step(pos.row * cols + pos.col, len, forward);
    Pos::new(next / cols, next % cols)
}

/// Moves within the column.
fn vertical(layout: GridLayout, pos: Pos, forward: bool) -> Pos {
    Pos::new(step(pos.row, layout.rows, forward), pos.col)
}

/// The action of `key` pressed in the cell at `pos` (AC 13).
pub fn navigate(
    layout: GridLayout,
    pos: Pos,
    key: NavKey,
    mods: Mods,
    dropdown: Dropdown,
) -> NavAction {
    if layout.rows == 0 || pos.row >= layout.rows || pos.col >= layout.cols() {
        return NavAction::None;
    }
    let product_cell = pos.col == PRODUCT_COL;
    let list_open = product_cell && dropdown.open;
    if mods.ctrl {
        return match key {
            NavKey::Delete if !layout.is_placeholder(pos.row) => NavAction::RemoveRow,
            _ => NavAction::None,
        };
    }
    match key {
        NavKey::Tab if mods.shift => NavAction::Move(linear(layout, pos, false)),
        NavKey::Tab | NavKey::Right => NavAction::Move(linear(layout, pos, true)),
        NavKey::Backtab | NavKey::Left => NavAction::Move(linear(layout, pos, false)),
        NavKey::Down if list_open => NavAction::HighlightNext,
        NavKey::Up if list_open => NavAction::HighlightPrev,
        NavKey::Down => NavAction::Move(vertical(layout, pos, true)),
        NavKey::Up => NavAction::Move(vertical(layout, pos, false)),
        NavKey::Enter if list_open && dropdown.highlighted => NavAction::SelectSuggestion,
        NavKey::Enter if pos.col >= PRICE_COL && layout.is_placeholder(pos.row) => {
            NavAction::AddRowFocusProduct
        }
        NavKey::Enter => NavAction::Commit,
        NavKey::Escape if list_open => NavAction::CloseDropdown,
        NavKey::Escape | NavKey::Delete | NavKey::Other => NavAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALE: GridLayout = GridLayout {
        kind: OrderKind::Sale,
        rows: 3,
    };
    const PURCHASE: GridLayout = GridLayout {
        kind: OrderKind::Purchase,
        rows: 2,
    };
    const NONE: Mods = Mods {
        shift: false,
        ctrl: false,
    };
    const SHIFT: Mods = Mods {
        shift: true,
        ctrl: false,
    };
    const CTRL: Mods = Mods {
        shift: false,
        ctrl: true,
    };
    const CLOSED: Dropdown = Dropdown {
        open: false,
        highlighted: false,
    };
    const OPEN: Dropdown = Dropdown {
        open: true,
        highlighted: false,
    };
    const HIGHLIGHTED: Dropdown = Dropdown {
        open: true,
        highlighted: true,
    };

    fn every_cell(layout: GridLayout) -> Vec<Pos> {
        (0..layout.rows)
            .flat_map(|r| (0..layout.cols()).map(move |c| Pos::new(r, c)))
            .collect()
    }

    fn mv(p: Pos) -> NavAction {
        NavAction::Move(p)
    }

    #[test]
    fn tab_and_right_walk_every_cell_in_row_major_order_and_wrap() {
        for layout in [SALE, PURCHASE] {
            let cells = every_cell(layout);
            for (i, &pos) in cells.iter().enumerate() {
                let next = cells[(i + 1) % cells.len()];
                for dropdown in [CLOSED, OPEN, HIGHLIGHTED] {
                    assert_eq!(navigate(layout, pos, NavKey::Tab, NONE, dropdown), mv(next));
                    assert_eq!(
                        navigate(layout, pos, NavKey::Right, NONE, dropdown),
                        mv(next)
                    );
                }
            }
            let last = *cells.last().unwrap();
            assert_eq!(
                navigate(layout, last, NavKey::Tab, NONE, CLOSED),
                mv(Pos::new(0, 0))
            );
        }
    }

    #[test]
    fn shift_tab_backtab_and_left_walk_backwards_and_wrap() {
        for layout in [SALE, PURCHASE] {
            let cells = every_cell(layout);
            for (i, &pos) in cells.iter().enumerate() {
                let prev = cells[(i + cells.len() - 1) % cells.len()];
                for dropdown in [CLOSED, OPEN] {
                    assert_eq!(
                        navigate(layout, pos, NavKey::Tab, SHIFT, dropdown),
                        mv(prev)
                    );
                    assert_eq!(
                        navigate(layout, pos, NavKey::Backtab, SHIFT, dropdown),
                        mv(prev)
                    );
                    assert_eq!(
                        navigate(layout, pos, NavKey::Left, NONE, dropdown),
                        mv(prev)
                    );
                }
            }
            assert_eq!(
                navigate(layout, Pos::new(0, 0), NavKey::Tab, SHIFT, CLOSED),
                mv(Pos::new(layout.rows - 1, layout.cols() - 1))
            );
        }
    }

    #[test]
    fn up_and_down_stay_in_the_column_and_wrap() {
        for layout in [SALE, PURCHASE] {
            for pos in every_cell(layout) {
                // Number cells ignore the dropdown; Artikulli only when closed.
                let dropdowns: &[Dropdown] = if pos.col == PRODUCT_COL {
                    &[CLOSED]
                } else {
                    &[CLOSED, OPEN, HIGHLIGHTED]
                };
                for &d in dropdowns {
                    let down = Pos::new((pos.row + 1) % layout.rows, pos.col);
                    let up = Pos::new((pos.row + layout.rows - 1) % layout.rows, pos.col);
                    assert_eq!(navigate(layout, pos, NavKey::Down, NONE, d), mv(down));
                    assert_eq!(navigate(layout, pos, NavKey::Up, NONE, d), mv(up));
                }
            }
        }
    }

    #[test]
    fn dropdown_keys_on_the_product_cell() {
        let p = Pos::new(1, PRODUCT_COL);
        assert_eq!(
            navigate(SALE, p, NavKey::Down, NONE, OPEN),
            NavAction::HighlightNext
        );
        assert_eq!(
            navigate(SALE, p, NavKey::Up, NONE, HIGHLIGHTED),
            NavAction::HighlightPrev
        );
        assert_eq!(
            navigate(SALE, p, NavKey::Enter, NONE, HIGHLIGHTED),
            NavAction::SelectSuggestion
        );
        // Open without a highlight: Enter commits.
        assert_eq!(
            navigate(SALE, p, NavKey::Enter, NONE, OPEN),
            NavAction::Commit
        );
        assert_eq!(
            navigate(SALE, p, NavKey::Escape, NONE, OPEN),
            NavAction::CloseDropdown
        );
        assert_eq!(
            navigate(SALE, p, NavKey::Escape, NONE, CLOSED),
            NavAction::None
        );
        // The placeholder row's product cell selects as well.
        assert_eq!(
            navigate(SALE, Pos::new(2, 0), NavKey::Enter, NONE, HIGHLIGHTED),
            NavAction::SelectSuggestion
        );
    }

    #[test]
    fn enter_in_a_placeholder_price_cell_adds_a_row() {
        assert_eq!(
            navigate(SALE, Pos::new(2, PRICE_COL), NavKey::Enter, NONE, CLOSED),
            NavAction::AddRowFocusProduct
        );
        for col in [PRICE_COL, SELL_COL] {
            assert_eq!(
                navigate(PURCHASE, Pos::new(1, col), NavKey::Enter, NONE, CLOSED),
                NavAction::AddRowFocusProduct
            );
        }
        // Not the placeholder, or not a price cell: commit only.
        assert_eq!(
            navigate(SALE, Pos::new(1, PRICE_COL), NavKey::Enter, NONE, CLOSED),
            NavAction::Commit
        );
        assert_eq!(
            navigate(SALE, Pos::new(2, QTY_COL), NavKey::Enter, NONE, CLOSED),
            NavAction::Commit
        );
        assert_eq!(
            navigate(SALE, Pos::new(2, PRODUCT_COL), NavKey::Enter, NONE, OPEN),
            NavAction::Commit
        );
        // A one-row grid: the placeholder is row 0.
        let only = GridLayout {
            kind: OrderKind::Sale,
            rows: 1,
        };
        assert_eq!(
            navigate(only, Pos::new(0, PRICE_COL), NavKey::Enter, NONE, CLOSED),
            NavAction::AddRowFocusProduct
        );
    }

    #[test]
    fn ctrl_delete_removes_entries_but_not_the_placeholder() {
        for layout in [SALE, PURCHASE] {
            for pos in every_cell(layout) {
                let expected = if layout.is_placeholder(pos.row) {
                    NavAction::None
                } else {
                    NavAction::RemoveRow
                };
                assert_eq!(navigate(layout, pos, NavKey::Delete, CTRL, OPEN), expected);
            }
        }
        // Plain Delete edits the text; Ctrl with other keys is not ours.
        assert_eq!(
            navigate(SALE, Pos::new(0, 1), NavKey::Delete, NONE, CLOSED),
            NavAction::None
        );
        assert_eq!(
            navigate(SALE, Pos::new(0, 1), NavKey::Tab, CTRL, CLOSED),
            NavAction::None
        );
    }

    #[test]
    fn other_keys_and_out_of_range_positions_are_ignored() {
        assert_eq!(
            navigate(SALE, Pos::new(0, 1), NavKey::Other, NONE, CLOSED),
            NavAction::None
        );
        assert_eq!(
            navigate(SALE, Pos::new(3, 0), NavKey::Tab, NONE, CLOSED),
            NavAction::None
        );
        assert_eq!(
            navigate(SALE, Pos::new(0, 3), NavKey::Tab, NONE, CLOSED),
            NavAction::None
        );
        assert_eq!(
            navigate(PURCHASE, Pos::new(0, 3), NavKey::Tab, NONE, CLOSED),
            mv(Pos::new(1, 0))
        );
    }
}
