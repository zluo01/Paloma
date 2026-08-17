mod action_panel;
mod section;

use std::{cell::RefCell, rc::Rc};

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, ListBox, ListBoxRow, Orientation, SelectionMode, prelude::*};
use paloma_core::{ExtensionCapabilityId, Item};

use crate::{
    helper::{Clear, scroll_selection_into_view},
    widgets::overlay::{
        OVERLAY_WIDTH_PX,
        model::{ChatMsg, Msg, SearchMsg},
        results::{
            search::{
                action_panel::ActionPanel,
                section::{RowEntry, RowKind},
            },
            step_index,
        },
    },
};

pub struct SearchView {
    widget: GtkBox,
    list: ListBox,
    rows: Rc<RefCell<Vec<RowEntry>>>,
    action_panel: RefCell<Option<ActionPanel>>,
    dispatcher: mpsc::UnboundedSender<Msg>,
}

impl SearchView {
    pub(crate) fn new(dispatcher: mpsc::UnboundedSender<Msg>) -> Self {
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .visible(false)
            .width_request(OVERLAY_WIDTH_PX)
            .css_classes(["paloma-result-card"])
            .build();

        let list = ListBox::builder()
            .selection_mode(SelectionMode::Browse)
            .css_classes(["paloma-section-list"])
            .build();
        widget.append(&list);

        let rows: Rc<RefCell<Vec<RowEntry>>> = Rc::new(RefCell::new(vec![]));

        let handler_rows = rows.clone();
        let handler_dispatcher = dispatcher.clone();
        list.connect_row_activated(move |list, row| {
            let rows = handler_rows.borrow();
            let Some(position) = rows.iter().position(|entry| entry.row == *row) else {
                return;
            };
            match &rows[position].kind {
                RowKind::Item {
                    extension_capability_id,
                    primary_index,
                    actions,
                } => {
                    let _ = handler_dispatcher.unbounded_send(Msg::Search(
                        SearchMsg::ResultActionRequested {
                            extension_capability_id: extension_capability_id.clone(),
                            action: actions[*primary_index].clone(),
                        },
                    ));
                },
                RowKind::ShowMore { tail_len } => {
                    row.set_visible(false);
                    let tail = &rows[position + 1..=position + tail_len];
                    for entry in tail {
                        entry.row.set_visible(true);
                    }
                    list.select_row(tail.first().map(|entry| &entry.row));
                },
                RowKind::Chat => {
                    let _ = handler_dispatcher
                        .unbounded_send(Msg::Chat(ChatMsg::PromptSubmitRequested));
                },
            }
        });

        Self {
            widget,
            list,
            rows,
            action_panel: RefCell::new(None),
            dispatcher,
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.widget
    }

    pub(crate) fn clear(&self) {
        self.rows.borrow_mut().clear();

        if let Some(panel) = self.action_panel.borrow_mut().take() {
            panel.close();
        }

        self.widget.set_visible(false);
        self.list.clear();
    }

    pub(crate) fn append_section(
        &self,
        extension_capability_id: ExtensionCapabilityId,
        handler_name: &str,
        items: Vec<Item>,
    ) -> bool {
        let items: Vec<Item> = items
            .into_iter()
            .filter(|item| !item.actions.is_empty())
            .collect();
        if items.is_empty() {
            return false;
        }

        section::append_search_section(
            &self.list,
            &mut self.rows.borrow_mut(),
            extension_capability_id,
            handler_name,
            items,
            &self.dispatcher,
        );
        self.reveal();
        true
    }

    pub(crate) fn append_chat_action(&self) {
        section::append_chat_row(&self.list, &mut self.rows.borrow_mut());
        self.reveal();
    }

    fn reveal(&self) {
        self.widget.set_visible(true);
    }

    pub(crate) fn open_action_panel(&self) {
        if self.is_action_panel_open() {
            return;
        }

        let Some(selected) = self.list.selected_row() else {
            return;
        };
        let rows = self.rows.borrow();
        let Some(entry) = rows.iter().find(|entry| entry.row == selected) else {
            return;
        };

        let RowKind::Item {
            extension_capability_id,
            actions,
            ..
        } = &entry.kind
        else {
            return;
        };
        if actions.len() < 2 {
            return;
        }

        *self.action_panel.borrow_mut() = Some(ActionPanel::new(
            &entry.row,
            extension_capability_id.clone(),
            actions.clone(),
            self.dispatcher.clone(),
        ));
    }

    pub(crate) fn activate(&self) -> bool {
        if let Some(selected) = self.list.selected_row() {
            selected.activate();
            return true;
        }

        false
    }

    /// Action panel navigation is handled within action panel
    /// which has higher priority than this function.
    /// This function only handle search result navigation
    pub(crate) fn navigate(&self, delta: i32) -> bool {
        let rows = self.rows.borrow();
        let visible: Vec<&ListBoxRow> = rows
            .iter()
            .map(|entry| &entry.row)
            .filter(|row| row.is_visible())
            .collect();
        if visible.is_empty() {
            return false;
        }

        let current = self
            .list
            .selected_row()
            .and_then(|selected| visible.iter().position(|row| **row == selected));
        let next = current.map_or(0, |current| step_index(current, delta, visible.len()));
        // when on first or last, no need to reselect.
        if current == Some(next) {
            return true;
        }

        let selected_row = visible[next];
        self.list.select_row(Some(selected_row));
        scroll_selection_into_view(selected_row, next, visible.len());
        true
    }

    pub(crate) fn is_action_panel_open(&self) -> bool {
        self.action_panel
            .borrow()
            .as_ref()
            .is_some_and(ActionPanel::is_open)
    }

    pub(crate) fn clear_action_panel(&self) {
        self.action_panel.borrow_mut().take();
    }

    pub(crate) fn render_any(&self) -> bool {
        !self.rows.borrow().is_empty()
    }
}
