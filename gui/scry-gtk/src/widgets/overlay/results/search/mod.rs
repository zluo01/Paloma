mod action_panel;
mod section;

use std::cell::{Cell, RefCell};

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, Button, Orientation, ScrolledWindow, prelude::*};
use scry_core::Item;

use crate::{
    helper::{Clear, scroll_into_view},
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
            .css_classes(["scry-result-card"])
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
        handler_id: &'static str,
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

        let section =
            Section::search_section(handler_id, handler_name, items, self.dispatcher.clone());

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

    pub(crate) fn open_action_panel(&self) {
        if self.is_action_panel_open() {
            return;
        }

        let Some((button, handler_id, actions)) = self.selected_action().and_then(|row| {
            (row.panel_actions.len() > 1).then(|| {
                (
                    row.button.clone(),
                    row.handler_id,
                    row.panel_actions.clone(),
                )
            })
        }) else {
            return;
        };

        *self.action_panel.borrow_mut() = Some(ActionPanel::new(
            &button,
            handler_id,
            actions,
            self.dispatcher.clone(),
        ));
    }

    pub(crate) fn activate(&self) -> bool {
        let panel_button = self
            .action_panel
            .borrow()
            .as_ref()
            .filter(|panel| panel.is_open())
            .and_then(ActionPanel::selected_button);
        if let Some(button) = panel_button {
            button.emit_clicked();
            return true;
        }

        if let Some(button) = self.selected_button() {
            button.emit_clicked();
            return true;
        }

        false
    }

    pub(crate) fn navigate(&self, delta: i32) -> bool {
        if let Some(panel) = self.action_panel.borrow().as_ref()
            && panel.is_open()
        {
            panel.navigate(delta);
            return true;
        }

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
                self.scroll_selection_into_view(next, actions_len);
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

    pub(crate) fn close_action_panel(&self) -> bool {
        let Some(panel) = self.action_panel.borrow_mut().take() else {
            return false;
        };

        if panel.is_open() {
            panel.close();
            return true;
        }

        false
    }

    fn select_row(&self, idx: usize) {
        if self.selected.get() == Some(idx) {
            return;
        }

        self.clear_selected();
        self.selected.set(Some(idx));
        if let Some(button) = self.selected_button() {
            button.add_css_class(SELECTED_CLASS);
        };
    }

    fn clear_selected(&self) {
        if let Some(button) = self.selected_button() {
            button.remove_css_class(SELECTED_CLASS);
        }
        self.selected.set(None);
    }

    fn push_section(&self, section: &GtkBox) {
        self.widget.append(section);
        self.widget.set_visible(true);
    }

    fn scroll_selection_into_view(&self, index: usize, size: usize) {
        let Some(button) = self.selected_button() else {
            return;
        };
        let Some(scroller) = button
            .ancestor(ScrolledWindow::static_type())
            .and_downcast::<ScrolledWindow>()
        else {
            return;
        };
        // The first/last rows snap the card fully to the top/bottom so its
        // padding (and the chat divider) isn't clipped; the rows sit below the
        // card padding, so a minimal scroll-to would leave that padding cut off.
        // Middle rows use the minimal scroll.
        let adj = scroller.vadjustment();
        match index {
            0 => adj.set_value(0.0),
            i if i + 1 == size => adj.set_value((adj.upper() - adj.page_size()).max(0.0)),
            _ => scroll_into_view(&button),
        }
    }

    fn action_len(&self) -> usize {
        self.sections.borrow().iter().map(|s| s.len()).sum()
    }

    fn selected_button(&self) -> Option<Button> {
        self.selected_action().map(|a| a.button.clone())
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
