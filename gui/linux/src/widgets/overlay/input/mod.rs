use std::{cell::Cell, rc::Rc};

use futures::channel::mpsc;
use gtk4::{Box as GtkBox, Orientation, SearchEntry, prelude::*};

use crate::widgets::overlay::model::{LauncherMsg, Mode, Msg};

const SEARCH_DEBOUNCE_MS: u32 = 200;

pub(crate) struct InputView {
    view: GtkBox,
    entry: SearchEntry,
    suppress: Rc<Cell<bool>>,
}

impl InputView {
    pub(super) fn new(dispatcher: mpsc::UnboundedSender<Msg>) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .height_request(64)
            .spacing(12)
            .css_classes(["paloma-query"])
            .build();

        let entry = SearchEntry::builder()
            .placeholder_text(placeholder(Mode::Search))
            .hexpand(true)
            .search_delay(SEARCH_DEBOUNCE_MS)
            .css_classes(["paloma-entry"])
            .build();
        view.append(&entry);

        let suppress = Rc::new(Cell::new(false));

        let search_dispatcher = dispatcher;
        let suppress_changed = suppress.clone();
        entry.connect_search_changed(move |entry| {
            if suppress_changed.replace(false) {
                return;
            }
            let content = entry.text().to_string();
            let _ = search_dispatcher
                .unbounded_send(Msg::Launcher(LauncherMsg::QueryChanged { content }));
        });

        Self {
            view,
            entry,
            suppress,
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn query(&self) -> String {
        self.entry.text().trim().to_string()
    }

    pub(crate) fn focus(&self) {
        self.entry.grab_focus();
        // Focusing the entry selects its text, which would make the first
        // keystroke after a summon discard a restored query.
        self.entry.set_position(-1);
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.entry.selection_bounds().is_some()
    }

    pub(crate) fn clear(&self) {
        if self.entry.text().is_empty() {
            return;
        }
        self.suppress.set(true);
        self.entry.set_text("");
    }

    pub(crate) fn set_mode(&self, mode: Mode) {
        self.entry.set_placeholder_text(Some(placeholder(mode)));
    }
}

fn placeholder(mode: Mode) -> &'static str {
    match mode {
        Mode::Search => "Search, or ask anything…",
        Mode::Chat => "Reply…",
        Mode::Session => "Search sessions…",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_follows_overlay_mode() {
        assert_eq!(placeholder(Mode::Search), "Search, or ask anything…");
        assert_eq!(placeholder(Mode::Chat), "Reply…");
        assert_eq!(placeholder(Mode::Session), "Search sessions…");
    }
}
