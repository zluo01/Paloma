use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, pango,
    prelude::{BoxExt, ButtonExt, WidgetExt},
};

pub(crate) fn new_section(header: Option<&str>, classname: &str) -> GtkBox {
    let section = GtkBox::builder()
        .css_classes(["paloma-chat-section", classname])
        .orientation(Orientation::Vertical)
        .build();

    if let Some(header) = header {
        let role = Label::builder().label(header).xalign(0.0).build();
        role.add_css_class("paloma-chat-role");
        section.append(&role);
    }

    section
}

pub(crate) fn append_content_label(parent: &GtkBox, text: &str, classname: &str) {
    let content = Label::builder()
        .label(text)
        .xalign(0.0)
        .halign(Align::Fill)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .selectable(true)
        .hexpand(true)
        .width_chars(1)
        .css_classes(["paloma-chat-text", classname])
        .build();
    parent.append(&content);
}

pub(crate) fn code_card(caption: &str, body: &str) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .build();
    card.add_css_class("paloma-code");

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.append(
        &Label::builder()
            .label(caption)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["paloma-code-caption"])
            .build(),
    );
    let copy = Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy")
        .css_classes(["flat", "paloma-code-copy"])
        .build();
    {
        let body = body.to_string();
        copy.connect_clicked(move |button| button.clipboard().set_text(&body));
    }
    header.append(&copy);
    card.append(&header);

    let content = Label::builder()
        .label(body)
        .xalign(0.0)
        .halign(Align::Fill)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .selectable(true)
        .width_chars(1)
        .css_classes(["paloma-code-body", "paloma-chat-text", "monospace"])
        .build();
    card.append(&content);

    card
}
