mod action_panel;
mod section;

use std::cell::{Cell, RefCell};

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, ListBoxRow, Orientation, prelude::*};
use paloma_core::{ExtensionCapabilityId, Item};

use crate::{
    helper::{Clear, scroll_selection_into_view},
    widgets::overlay::{
        OVERLAY_WIDTH_PX, SELECTED_CLASS,
        model::Msg,
        results::{
            search::{
                action_panel::ActionPanel,
                section::{SearchAction, Section},
            },
            step_index,
        },
    },
};

pub struct SearchView {
    widget: GtkBox,
    sections: RefCell<Vec<Section>>,
    selected: Cell<Option<usize>>,
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
        Self {
            widget,
            sections: RefCell::new(vec![]),
            dispatcher,
            selected: Cell::new(None),
            action_panel: RefCell::new(None),
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.widget
    }

    pub(crate) fn clear(&self) {
        self.sections.borrow_mut().clear();
        self.selected.set(None);

        if let Some(panel) = self.action_panel.borrow_mut().take() {
            panel.close();
        }

        self.widget.set_visible(false);
        self.widget.clear();
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

        let section = Section::search_section(
            self.sections.borrow().len(),
            extension_capability_id,
            handler_name,
            items,
            self.dispatcher.clone(),
        );

        self.push_section(section.widget());
        self.sections.borrow_mut().push(section);
        true
    }

    /// Append the "Chat about it" pseudo-row; `invoke` enters chat mode.
    pub(crate) fn append_chat_action(&self) {
        let section = Section::chat_section(self.action_len(), self.dispatcher.clone());

        self.push_section(section.widget());
        self.sections.borrow_mut().push(section);
    }

    pub(crate) fn open_action_panel(&self, target: Option<(usize, usize)>) {
        if self.is_action_panel_open() {
            return;
        }

        if let Some((section_index, local_index)) = target {
            let offset: usize = self
                .sections
                .borrow()
                .iter()
                .take(section_index)
                .map(Section::len)
                .sum();
            self.select_row(offset + local_index);
        }

        let Some((row, extension_capability_id, actions)) =
            self.selected_action().and_then(|action| {
                (action.panel_actions.len() > 1).then(|| {
                    (
                        action.row.clone(),
                        action.extension_capability_id,
                        action.panel_actions.clone(),
                    )
                })
            })
        else {
            return;
        };

        *self.action_panel.borrow_mut() = Some(ActionPanel::new(
            &row,
            extension_capability_id,
            actions,
            self.dispatcher.clone(),
        ));
    }

    pub(crate) fn activate(&self) -> bool {
        if let Some(row) = self.selected_row() {
            row.activate();
            return true;
        }

        false
    }

    /// Action panel navigation is handled within action panel
    /// which has higher priority than this function.
    /// This function only handle search result navigation
    pub(crate) fn navigate(&self, delta: i32) -> bool {
        let actions_len = self.action_len();
        let next = match self.selected.get() {
            Some(current) => step_index(current, delta, actions_len),
            None if actions_len > 0 => Some(0),
            None => None,
        };

        match next {
            None => false,
            Some(next) => {
                self.select_row(next);
                if let Some(row) = self.selected_row() {
                    scroll_selection_into_view(&row, next, actions_len);
                }
                true
            },
        }
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

    fn select_row(&self, idx: usize) {
        if self.selected.get() == Some(idx) {
            return;
        }

        self.clear_selected();
        self.selected.set(Some(idx));
        if let Some(row) = self.selected_row() {
            row.add_css_class(SELECTED_CLASS);
        };
    }

    fn clear_selected(&self) {
        if let Some(row) = self.selected_row() {
            row.remove_css_class(SELECTED_CLASS);
        }
        self.selected.set(None);
    }

    fn push_section(&self, section: &GtkBox) {
        self.widget.append(section);
        self.widget.set_visible(true);
    }

    fn action_len(&self) -> usize {
        self.sections.borrow().iter().map(|s| s.len()).sum()
    }

    fn selected_row(&self) -> Option<ListBoxRow> {
        self.selected_action().map(|a| a.row)
    }

    fn selected_action(&self) -> Option<SearchAction> {
        let mut selected = self.selected.get()?;
        for section in self.sections.borrow().iter() {
            let len = section.len();
            if selected < len {
                return section.action(selected);
            }
            selected -= len;
        }
        None
    }

    pub(crate) fn render_any(&self) -> bool {
        !self.sections.borrow().is_empty()
    }
}
