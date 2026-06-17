//! Ctrl+K action panel for rows with multiple actions.
//!
//! The panel lives in a keyboard-less content window, so `keys.rs` drives
//! navigation while the search bar keeps GTK focus.

use std::{cell::Cell, rc::Rc};

use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, Popover, PositionType, Widget, prelude::*,
};

use super::{SELECTED_CLASS, selection::RowAction};

/// Cap external action labels; long labels ellipsize instead of widening the panel.
const LABEL_MAX_CHARS: i32 = 40;

pub(super) struct ActionPanel {
    popover: Popover,
    invokers: Vec<Rc<dyn Fn()>>,
    buttons: Vec<Button>,
    selected: Cell<usize>,
}

impl ActionPanel {
    /// Build and pop up a panel of `actions` under `anchor`, first highlighted.
    /// `on_closed` runs on every close path (to hand focus back to the entry).
    pub(super) fn new(
        anchor: &Widget,
        actions: Vec<RowAction>,
        on_closed: impl Fn() + 'static,
    ) -> Self {
        let list = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .build();
        list.add_css_class("scry-actions");

        let mut invokers = Vec::with_capacity(actions.len());
        let mut buttons = Vec::with_capacity(actions.len());
        for action in actions {
            // Labels come from plugins/MCP, so bound them like result titles.
            let label = Label::builder()
                .label(&action.label)
                .xalign(0.0)
                .halign(Align::Start)
                .single_line_mode(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .max_width_chars(LABEL_MAX_CHARS)
                .build();
            let button = Button::builder().child(&label).build();
            button.add_css_class("flat");
            button.add_css_class("scry-action");
            button.set_can_focus(false);
            button.set_focusable(false);
            list.append(&button);
            buttons.push(button);
            invokers.push(action.invoke);
        }

        let popover = Popover::builder().autohide(false).has_arrow(false).build();
        popover.set_child(Some(&list));
        popover.set_position(PositionType::Bottom);
        popover.set_parent(anchor);
        popover.connect_closed(move |popover| {
            popover.unparent();
            on_closed();
        });

        // Pointer activation (the panel can't get keyboard focus here). Tear the
        // popover down before running the action, which may hide/clear the
        // overlay — same order as a result-row click.
        for (button, invoke) in buttons.iter().zip(&invokers) {
            let popover = popover.clone();
            let invoke = invoke.clone();
            button.connect_clicked(move |_| {
                popover.popdown();
                invoke();
            });
        }

        let panel = Self {
            popover,
            invokers,
            buttons,
            selected: Cell::new(0),
        };
        panel.highlight(0);
        panel.popover.popup();
        panel
    }

    pub(super) fn is_open(&self) -> bool {
        self.popover.is_visible()
    }

    pub(super) fn navigate(&self, delta: i32) {
        let len = self.buttons.len() as i32;
        if len == 0 {
            return;
        }
        let next = (self.selected.get() as i32 + delta).clamp(0, len - 1) as usize;
        self.selected.set(next);
        self.highlight(next);
    }

    /// Close, then run the highlighted action — UI teardown before side effects.
    pub(super) fn activate(&self) {
        let invoke = self.invokers.get(self.selected.get()).cloned();
        self.close();
        if let Some(invoke) = invoke {
            invoke();
        }
    }

    pub(super) fn close(&self) {
        self.popover.popdown();
    }

    fn highlight(&self, idx: usize) {
        for (i, button) in self.buttons.iter().enumerate() {
            if i == idx {
                button.add_css_class(SELECTED_CLASS);
            } else {
                button.remove_css_class(SELECTED_CLASS);
            }
        }
    }
}
