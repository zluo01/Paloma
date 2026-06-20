use gtk4::{
    Box as GtkBox, TextBuffer, TextView, WrapMode,
    prelude::{BoxExt, TextBufferExt},
};

use crate::widgets::overlay::results::chat::helper::new_section;

const REASONING_TITLE: &str = "Thinking";
const REASONING_CLASS: &str = "scry-chat-section-thinking";

pub(crate) struct ReasoningSection {
    view: GtkBox,
    buffer: TextBuffer,
}

impl ReasoningSection {
    pub(crate) fn new() -> Self {
        let view = new_section(Some(REASONING_TITLE), REASONING_CLASS);

        let buffer = TextBuffer::new(None);
        let text_view = TextView::builder()
            .buffer(&buffer)
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(WrapMode::WordChar)
            .hexpand(true)
            .focusable(true)
            .can_focus(true)
            .focus_on_click(true)
            .left_margin(0)
            .right_margin(0)
            .top_margin(0)
            .bottom_margin(0)
            .css_classes(["scry-chat-text", REASONING_CLASS])
            .build();
        view.append(&text_view);

        Self { view, buffer }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn append(&self, text: &str) {
        self.buffer.insert(&mut self.buffer.end_iter(), text);
    }
}
