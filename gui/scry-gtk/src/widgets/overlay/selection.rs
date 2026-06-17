use std::{cell::RefCell, rc::Rc};

use gtk4::{Widget, prelude::*};

use super::SELECTED_CLASS;

pub(super) type SelectionRef = Rc<RefCell<Selection>>;

/// One of a row's actions, listed in the Ctrl+K panel.
#[derive(Clone)]
pub(super) struct RowAction {
    pub(super) label: String,
    pub(super) invoke: Rc<dyn Fn()>,
}

pub(super) struct SelectableRow {
    pub(super) row: Widget,
    /// Run on Enter / row click (the item's primary action).
    pub(super) primary: Option<Rc<dyn Fn()>>,
    /// Offered in the Ctrl+K panel; a panel opens only when there's more than one.
    pub(super) actions: Vec<RowAction>,
}

/// Single-level keyboard selection over the result rows. The rows are not
/// GTK-focusable; the highlight is a CSS class driven from `keys.rs`.
#[derive(Default)]
pub(super) struct Selection {
    rows: Vec<SelectableRow>,
    selected: Option<usize>,
}

impl Selection {
    pub(super) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.rows.clear();
        self.selected = None;
    }

    pub(super) fn append_rows(&mut self, rows: Vec<SelectableRow>) {
        let first_added = self.rows.len();
        self.rows.extend(rows);
        if self.selected.is_none() && first_added < self.rows.len() {
            self.select_row(first_added);
        }
    }

    pub(super) fn navigate(&mut self, delta: i32) {
        match self.selected {
            Some(current) => {
                if let Some(next) = step_index(current, delta, self.rows.len()) {
                    self.select_row(next);
                }
            },
            None if !self.rows.is_empty() => self.select_row(0),
            None => {},
        }
    }

    pub(super) fn selected_widget(&self) -> Option<Widget> {
        Some(self.rows[self.selected?].row.clone())
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The selected row's primary action (Enter / click).
    pub(super) fn activate(&self) -> Option<Rc<dyn Fn()>> {
        self.rows[self.selected?].primary.clone()
    }

    /// Anchor widget + all the row's actions for the Ctrl+K panel; `None` unless
    /// the selected row offers more than one action.
    pub(super) fn selected_actions(&self) -> Option<(Widget, Vec<RowAction>)> {
        let row = &self.rows[self.selected?];
        (row.actions.len() > 1).then(|| (row.row.clone(), row.actions.clone()))
    }

    pub(super) fn select_row(&mut self, idx: usize) {
        if idx >= self.rows.len() || self.selected == Some(idx) {
            return;
        }
        self.clear_selected();
        self.rows[idx].row.add_css_class(SELECTED_CLASS);
        self.selected = Some(idx);
    }

    fn clear_selected(&mut self) {
        if let Some(idx) = self.selected.take() {
            self.rows[idx].row.remove_css_class(SELECTED_CLASS);
        }
    }
}

fn step_index(current: usize, delta: i32, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some((current as i32 + delta).clamp(0, len as i32 - 1) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::step_index;

    #[test]
    fn none_when_empty() {
        assert_eq!(step_index(0, 1, 0), None);
    }

    #[test]
    fn steps_within_range() {
        assert_eq!(step_index(1, 1, 4), Some(2));
    }

    #[test]
    fn clamps_at_upper_bound() {
        assert_eq!(step_index(3, 1, 4), Some(3));
    }

    #[test]
    fn clamps_at_lower_bound() {
        assert_eq!(step_index(0, -1, 4), Some(0));
    }
}
