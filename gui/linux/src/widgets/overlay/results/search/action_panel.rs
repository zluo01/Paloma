use std::cell::Cell;

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, Popover, PositionType, Widget, prelude::*,
};
use scry_core::Action;

use crate::widgets::overlay::{
    SELECTED_CLASS,
    model::{Msg, SearchMsg},
};

const LABEL_MAX_CHARS: i32 = 40;

const GOLDEN_RATIO_WIDTH: f64 = 0.618;

const MIN_PANEL_WIDTH: i32 = 200;

pub(super) struct ActionPanel {
    popover: Popover,
    buttons: Vec<Button>,
    selected: Cell<usize>,
}

impl ActionPanel {
    pub(super) fn new(
        anchor: &impl IsA<Widget>,
        handler_id: &'static str,
        actions: Vec<Action>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let popover = Popover::builder()
            .autohide(false)
            .has_arrow(false)
            .position(PositionType::Bottom)
            .build();

        let list = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .build();
        list.add_css_class("scry-actions");

        let mut buttons = Vec::with_capacity(actions.len());
        for action in actions {
            let label = Label::builder()
                .label(&action.label)
                .xalign(0.0)
                .halign(Align::Start)
                .single_line_mode(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .max_width_chars(LABEL_MAX_CHARS)
                .build();
            let button = Button::builder()
                .child(&label)
                .focusable(false)
                .can_focus(false)
                .css_classes(["flat", "scry-action"])
                .build();

            let action_popover = popover.clone();
            let action_dispatcher = dispatcher.clone();
            button.connect_clicked(move |_| {
                action_popover.popdown();
                let _ = action_dispatcher.unbounded_send(Msg::Search(
                    SearchMsg::ResultActionRequested {
                        handler_id,
                        action: action.clone(),
                    },
                ));
            });
            list.append(&button);
            buttons.push(button);
        }

        let width = (anchor.width() as f64 * GOLDEN_RATIO_WIDTH).round() as i32;
        list.set_width_request(width.max(MIN_PANEL_WIDTH));

        popover.set_child(Some(&list));
        popover.set_parent(anchor);
        popover.connect_closed(move |popover| {
            popover.unparent();
            let _ = dispatcher.unbounded_send(Msg::Search(SearchMsg::ActionPanelClosed));
        });

        let panel = Self {
            popover,
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
        let next = (self.selected.get() as i32 + delta).rem_euclid(len) as usize;
        self.selected.set(next);
        self.highlight(next);
    }

    pub(super) fn selected_button(&self) -> Option<Button> {
        self.buttons.get(self.selected.get()).cloned()
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
