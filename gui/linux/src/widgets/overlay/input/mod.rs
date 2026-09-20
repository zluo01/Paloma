mod view;

use std::{cell::Cell, rc::Rc, time::Duration};

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Image, Inscription, Orientation, Overlay, PolicyType, ScrolledWindow,
    TextView, WrapMode, glib, prelude::*,
};

use crate::widgets::overlay::{
    input::view::ComposerView,
    model::{LauncherMsg, Mode, Msg},
};

const SEARCH_DEBOUNCE_MS: u64 = 200;
const MAX_CONTENT_HEIGHT_PX: i32 = 171;

pub(crate) struct InputView {
    view: GtkBox,
    text: TextView,
    placeholder: Inscription,
    // signal to tell if change is programmatic change or user input change
    suppress: Rc<Cell<bool>>,
    debounce: Rc<Cell<Option<glib::SourceId>>>,
}

impl InputView {
    pub(super) fn new(dispatcher: mpsc::UnboundedSender<Msg>) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .css_classes(["paloma-query"])
            .build();

        let glyph = Image::builder()
            .icon_name("edit-find-symbolic")
            .valign(Align::Start)
            .css_classes(["paloma-query-glyph"])
            .build();
        view.append(&glyph);

        let text: TextView = glib::Object::builder::<ComposerView>()
            .property("wrap-mode", WrapMode::WordChar)
            .property("accepts-tab", false)
            .property("valign", Align::Start)
            .property("css-classes", ["paloma-entry"].as_slice())
            .build()
            .upcast();

        let scroller = ScrolledWindow::builder()
            .child(&text)
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::External)
            .kinetic_scrolling(false)
            .propagate_natural_height(true)
            .max_content_height(MAX_CONTENT_HEIGHT_PX)
            .hexpand(true)
            .valign(Align::Start)
            .build();

        let placeholder = Inscription::builder()
            .text(placeholder(Mode::Search))
            .can_target(false)
            .css_classes(["paloma-entry-placeholder"])
            .build();

        let overlay = Overlay::builder().child(&scroller).build();
        overlay.add_overlay(&placeholder);
        view.append(&overlay);

        let suppress = Rc::new(Cell::new(false));
        let debounce: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));

        let changed_placeholder = placeholder.clone();
        let changed_suppress = suppress.clone();
        let changed_debounce = debounce.clone();
        text.buffer().connect_changed(move |buffer| {
            let (start, end) = buffer.bounds();
            let empty = start == end;

            // show placeholder when no text
            changed_placeholder.set_visible(empty);

            if changed_suppress.replace(false) {
                return;
            }
            if let Some(pending) = changed_debounce.take() {
                pending.remove();
            }
            if empty {
                let _ = dispatcher.unbounded_send(Msg::Launcher(LauncherMsg::QueryChanged {
                    content: String::new(),
                }));
                return;
            }
            let buffer = buffer.clone();
            let dispatcher = dispatcher.clone();
            let slot = changed_debounce.clone();
            let id = glib::timeout_add_local_once(
                Duration::from_millis(SEARCH_DEBOUNCE_MS),
                move || {
                    slot.set(None);
                    let (start, end) = buffer.bounds();
                    let content = buffer.text(&start, &end, false).to_string();
                    let _ = dispatcher
                        .unbounded_send(Msg::Launcher(LauncherMsg::QueryChanged { content }));
                },
            );
            changed_debounce.set(Some(id));
        });

        // make the placeholder disappear when try to type with IME
        let preedit_placeholder = placeholder.clone();
        text.connect_preedit_changed(move |view, preedit| {
            preedit_placeholder.set_visible(view.buffer().char_count() == 0 && preedit.is_empty());
        });

        Self {
            view,
            text,
            placeholder,
            suppress,
            debounce,
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn query(&self) -> String {
        let buffer = self.text.buffer();
        let (start, end) = buffer.bounds();
        buffer.text(&start, &end, false).trim().to_string()
    }

    pub(crate) fn focus(&self) {
        if self.text.has_focus() {
            return;
        }
        self.text.grab_focus();
        let buffer = self.text.buffer();
        buffer.place_cursor(&buffer.end_iter());
        // this make sure when paste text, cursor will move to the end
        self.text.scroll_mark_onscreen(&buffer.get_insert());
    }

    pub(crate) fn move_cursor(&self, delta: i32) -> bool {
        let buffer = self.text.buffer();
        let mut iter = buffer.iter_at_mark(&buffer.get_insert());
        if delta < 0 {
            !self.text.backward_display_line(&mut iter)
        } else {
            !self.text.forward_display_line(&mut iter)
        }
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.text.buffer().has_selection()
    }

    pub(crate) fn clear(&self) {
        let buffer = self.text.buffer();
        if buffer.char_count() == 0 {
            return;
        }
        if let Some(pending) = self.debounce.take() {
            pending.remove();
        }
        self.suppress.set(true);
        buffer.set_text("");
    }

    pub(crate) fn set_mode(&self, mode: Mode) {
        self.placeholder.set_text(Some(placeholder(mode)));
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
