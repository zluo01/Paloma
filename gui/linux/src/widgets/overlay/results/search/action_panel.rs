use std::cell::Cell;

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, EventControllerKey, Label, Orientation, Popover, PositionType,
    PropagationPhase, Widget, gdk::Key, glib::Propagation, prelude::*,
};
use paloma_core::{Action, ExtensionCapabilityId};

use crate::widgets::{
    keymap::{self, BindingId, Context},
    overlay::{
        SELECTED_CLASS,
        model::{Msg, SearchMsg},
    },
};

const LABEL_MAX_CHARS: i32 = 40;

const GOLDEN_RATIO_WIDTH: f64 = 0.618;

const MIN_PANEL_WIDTH: i32 = 200;

pub(super) struct ActionPanel {
    popover: Popover,
}

impl ActionPanel {
    pub(super) fn new(
        anchor: &impl IsA<Widget>,
        extension_capability_id: ExtensionCapabilityId,
        actions: Vec<Action>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let popover = Popover::builder()
            .has_arrow(false)
            .position(PositionType::Bottom)
            .build();

        let list = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .build();
        list.add_css_class("paloma-actions");

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
                .css_classes(["flat", "paloma-action"])
                .build();

            let action_popover = popover.clone();
            let action_dispatcher = dispatcher.clone();
            let id = extension_capability_id.clone();
            button.connect_clicked(move |_| {
                action_popover.popdown();
                let _ = action_dispatcher.unbounded_send(Msg::Search(
                    SearchMsg::ResultActionRequested {
                        extension_capability_id: id.clone(),
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

        highlight(&buttons, 0);

        // with auto hide, popover takes the higher priority than rest of the tree
        // hence we need to handle the key binding within popover instead of globally
        let selected = Cell::new(0usize);
        let keys = EventControllerKey::new();
        keys.set_propagation_phase(PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, state| {
            match keymap::match_binding(Context::Search, key, state) {
                Some(BindingId::SearchMove) => {
                    let delta = if matches!(key, Key::Up | Key::KP_Up) {
                        -1
                    } else {
                        1
                    };
                    let len = buttons.len() as i32;
                    let next = (selected.get() as i32 + delta).rem_euclid(len) as usize;
                    selected.set(next);
                    highlight(&buttons, next);
                },
                Some(BindingId::SearchSubmit) => {
                    if let Some(button) = buttons.get(selected.get()) {
                        button.emit_clicked();
                    }
                },
                _ => return Propagation::Proceed,
            }
            Propagation::Stop
        });
        popover.add_controller(keys);

        popover.popup();
        Self { popover }
    }

    pub(super) fn is_open(&self) -> bool {
        self.popover.is_visible()
    }

    pub(super) fn close(&self) {
        self.popover.popdown();
    }
}

fn highlight(buttons: &[Button], idx: usize) {
    for (i, button) in buttons.iter().enumerate() {
        if i == idx {
            button.add_css_class(SELECTED_CLASS);
        } else {
            button.remove_css_class(SELECTED_CLASS);
        }
    }
}
