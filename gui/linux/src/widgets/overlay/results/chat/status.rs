use gtk4::{
    Align, Box as GtkBox, Label, Orientation,
    prelude::{BoxExt, WidgetExt},
};
use libadwaita::Spinner;

const PENDING_CLASS: &str = "scry-chat-pending";
const ERROR_CLASS: &str = "scry-chat-error";
const CANCEL_CLASS: &str = "scry-chat-cancel";

const THINKING_TEXT: &str = "Thinking…";
const CANCELLED_TEXT: &str = "Cancelled";

pub(super) struct StatusView {
    view: GtkBox,
    indicator: Spinner,
    status: Label,
}

impl StatusView {
    pub(super) fn new() -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .halign(Align::Start)
            .css_classes([PENDING_CLASS])
            .visible(false)
            .build();
        let indicator = Spinner::builder().visible(false).build();
        indicator.set_size_request(14, 14);
        view.append(&indicator);
        let status = Label::builder()
            .label(THINKING_TEXT)
            .wrap(true)
            .max_width_chars(50)
            .xalign(0.0)
            .build();
        view.append(&status);

        Self {
            view,
            indicator,
            status,
        }
    }

    pub(super) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(super) fn start(&self) {
        self.indicator.set_visible(true);
        self.status.set_text(THINKING_TEXT);
        self.view.set_css_classes(&[PENDING_CLASS]);
        self.view.set_visible(true)
    }

    pub(super) fn finish(&self) {
        self.indicator.set_visible(false);
        self.view.set_visible(false)
    }

    pub(super) fn fail(&self, message: &str) {
        self.indicator.set_visible(false);
        self.status.set_text(message);
        self.view.set_css_classes(&[PENDING_CLASS, ERROR_CLASS]);
        self.view.set_visible(true)
    }

    pub(super) fn cancel(&self) {
        self.indicator.set_visible(false);
        self.status.set_text(CANCELLED_TEXT);
        self.view.set_css_classes(&[PENDING_CLASS, CANCEL_CLASS]);
        self.view.set_visible(true)
    }
}
