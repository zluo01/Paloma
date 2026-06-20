use std::{cell::Cell, rc::Rc};

use futures::channel::mpsc;
use gtk4::{SearchEntry, prelude::*};

use crate::widgets::overlay::model::Msg;

const SEARCH_DEBOUNCE_MS: u32 = 200;

pub(super) struct Search {
    pub(super) entry: SearchEntry,
    suppress: Rc<Cell<bool>>,
}

impl Search {
    pub(super) fn new(dispatcher: mpsc::UnboundedSender<Msg>) -> Self {
        let entry = SearchEntry::builder()
            .placeholder_text("Search or ask Scry...")
            .hexpand(true)
            .search_delay(SEARCH_DEBOUNCE_MS)
            .css_classes(["scry-entry"])
            .build();

        let suppress = Rc::new(Cell::new(false));

        let search_dispatcher = dispatcher.clone();
        let suppress_changed = suppress.clone();
        entry.connect_search_changed(move |entry| {
            if suppress_changed.replace(false) {
                return;
            }
            let content = entry.text().to_string();
            let _ = search_dispatcher.unbounded_send(Msg::LauncherQueryChanged { content });
        });

        Self { entry, suppress }
    }

    pub(crate) fn query(&self) -> String {
        self.entry.text().to_string()
    }

    pub(super) fn focus(&self) {
        self.entry.grab_focus();
    }

    pub(super) fn has_selection(&self) -> bool {
        self.entry.selection_bounds().is_some()
    }

    pub(super) fn clear(&self) {
        if self.entry.text().is_empty() {
            return;
        }
        self.suppress.set(true);
        self.entry.set_text("");
    }
}
