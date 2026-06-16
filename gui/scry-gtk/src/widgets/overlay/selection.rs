use std::{cell::RefCell, rc::Rc};

use gtk4::{Box as GtkBox, Image, Widget, prelude::*};

use super::{CHEVRON_COLLAPSED, CHEVRON_EXPANDED, SELECTED_CLASS};

pub(super) type SelectionRef = Rc<RefCell<Selection>>;

pub(super) struct SelectableRow {
    pub(super) row: Widget,
    pub(super) actions: Vec<Widget>,
    pub(super) invokers: Vec<Rc<dyn Fn()>>,
    pub(super) expand_target: Option<(GtkBox, Image)>,
}

pub(super) enum Activation {
    Invoke(Rc<dyn Fn()>),
    Expand(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selected {
    Row(usize),
    Action { row: usize, action: usize },
}

#[derive(Default)]
pub(super) struct Selection {
    rows: Vec<SelectableRow>,
    selected: Option<Selected>,
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
            Some(Selected::Action { row, action }) => {
                if let Some(next) = step_index(action, delta, self.rows[row].actions.len()) {
                    self.select_action(row, next);
                }
            },
            Some(Selected::Row(row)) => {
                if let Some(next) = step_index(row, delta, self.rows.len()) {
                    self.select_row(next);
                }
            },
            None if !self.rows.is_empty() => self.select_row(0),
            None => {},
        }
    }

    /// The widget currently carrying the selection highlight.
    pub(super) fn selected_widget(&self) -> Option<Widget> {
        match self.selected? {
            Selected::Row(row) => Some(self.rows[row].row.clone()),
            Selected::Action { row, action } => Some(self.rows[row].actions[action].clone()),
        }
    }

    pub(super) fn activation(&self) -> Option<Activation> {
        match self.selected? {
            Selected::Action { row, action } => {
                Some(Activation::Invoke(self.rows[row].invokers[action].clone()))
            },
            Selected::Row(row) => {
                let invokers = &self.rows[row].invokers;
                if invokers.len() <= 1 {
                    invokers.first().cloned().map(Activation::Invoke)
                } else {
                    Some(Activation::Expand(row))
                }
            },
        }
    }

    pub(super) fn select_row(&mut self, row: usize) {
        if row >= self.rows.len() || self.selected == Some(Selected::Row(row)) {
            return;
        }
        self.clear_selected();
        self.rows[row].row.add_css_class(SELECTED_CLASS);
        self.selected = Some(Selected::Row(row));
    }

    pub(super) fn select_action(&mut self, row: usize, action: usize) {
        if row >= self.rows.len() || action >= self.rows[row].actions.len() {
            return;
        }
        if self.selected == Some(Selected::Action { row, action }) {
            return;
        }
        self.clear_selected();
        self.show_actions(row, true);
        self.rows[row].actions[action].add_css_class(SELECTED_CLASS);
        self.selected = Some(Selected::Action { row, action });
    }

    pub(super) fn toggle_row(&mut self, row: usize) {
        if row >= self.rows.len() {
            return;
        }
        if let Some(Selected::Action { row: selected, .. }) = self.selected
            && selected == row
        {
            self.collapse_action();
            return;
        }
        if self.rows[row].actions.is_empty() {
            self.select_row(row);
        } else {
            self.select_action(row, 0);
        }
    }

    pub(super) fn collapse_action(&mut self) -> bool {
        let Some(Selected::Action { row, action }) = self.selected else {
            return false;
        };
        self.rows[row].actions[action].remove_css_class(SELECTED_CLASS);
        self.show_actions(row, false);
        self.rows[row].row.add_css_class(SELECTED_CLASS);
        self.selected = Some(Selected::Row(row));
        true
    }

    fn clear_selected(&mut self) {
        match self.selected.take() {
            Some(Selected::Row(row)) => self.rows[row].row.remove_css_class(SELECTED_CLASS),
            Some(Selected::Action { row, action }) => {
                self.rows[row].actions[action].remove_css_class(SELECTED_CLASS);
                self.show_actions(row, false);
            },
            None => {},
        }
    }

    fn show_actions(&self, row: usize, visible: bool) {
        if let Some((actions_box, chevron)) = &self.rows[row].expand_target {
            actions_box.set_visible(visible);
            chevron.set_icon_name(Some(if visible {
                CHEVRON_EXPANDED
            } else {
                CHEVRON_COLLAPSED
            }));
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
