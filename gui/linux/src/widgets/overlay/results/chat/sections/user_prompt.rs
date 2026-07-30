use gtk4::Box;

use crate::widgets::overlay::results::chat::helper::{append_content_label, new_section};

const USER_TITLE: &str = "You";
const USER_CLASS: &str = "paloma-chat-section-user";

pub(crate) struct UserPromptSection {
    view: Box,
}

impl UserPromptSection {
    pub(crate) fn new(text: &str) -> Self {
        let view = new_section(Some(USER_TITLE), USER_CLASS);
        append_content_label(&view, text, USER_CLASS);
        Self { view }
    }

    pub(crate) fn widget(&self) -> &Box {
        &self.view
    }
}
