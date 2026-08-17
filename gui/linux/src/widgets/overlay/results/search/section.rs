use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, ListBox, ListBoxRow, Orientation, Revealer,
    RevealerTransitionType, Separator, StateFlags, Widget, prelude::*,
};
use paloma_core::{Action, CapabilityIcon, ExtensionCapabilityId, Item};

use crate::{
    helper::icon_image,
    widgets::overlay::model::{Msg, SearchMsg},
};

const CHAT_ACTION_LABEL: &str = "Chat about it";

const MAX_SECTION_ITEMS: usize = 5;

pub(super) enum RowKind {
    Item {
        extension_capability_id: ExtensionCapabilityId,
        primary_index: usize,
        actions: Vec<Action>,
    },
    ShowMore {
        tail_len: usize,
    },
    Chat,
}

pub(super) struct RowEntry {
    pub(super) row: ListBoxRow,
    pub(super) kind: RowKind,
}

pub(super) fn append_search_section(
    list: &ListBox,
    rows: &mut Vec<RowEntry>,
    extension_capability_id: ExtensionCapabilityId,
    handler_name: &str,
    mut items: Vec<Item>,
    dispatcher: &mpsc::UnboundedSender<Msg>,
) {
    let header = Label::builder()
        .label(handler_name)
        .xalign(0.0)
        .halign(Align::Start)
        .css_classes(["paloma-section-header"])
        .build();
    list.append(&build_static_row(&header));

    let tail = if items.len() > MAX_SECTION_ITEMS + 1 {
        items.split_off(MAX_SECTION_ITEMS)
    } else {
        Vec::new()
    };

    for item in items {
        let entry = build_item_row(extension_capability_id.clone(), item, dispatcher);
        list.append(&entry.row);
        rows.push(entry);
    }

    if !tail.is_empty() {
        let show_more = build_show_more_row(tail.len());
        list.append(&show_more);

        let mut tail_entries = Vec::with_capacity(tail.len());
        for item in tail {
            let entry = build_item_row(extension_capability_id.clone(), item, dispatcher);
            entry.row.set_visible(false);
            list.append(&entry.row);
            tail_entries.push(entry);
        }

        rows.push(RowEntry {
            row: show_more,
            kind: RowKind::ShowMore {
                tail_len: tail_entries.len(),
            },
        });
        rows.extend(tail_entries);
    }
}

pub(super) fn append_chat_row(list: &ListBox, rows: &mut Vec<RowEntry>) {
    if !rows.is_empty() {
        let separator = Separator::builder()
            .orientation(Orientation::Horizontal)
            .margin_top(8)
            .margin_bottom(4)
            .margin_end(4)
            .build();
        list.append(&build_static_row(&separator));
    }

    let item = Item {
        title: CHAT_ACTION_LABEL.to_string(),
        subtitle: None,
        icon: Some(CapabilityIcon::name("dialog-question-symbolic")),
        actions: Vec::new(),
    };
    let row = flat_row(&item_content_row(&item), Some("paloma-chat-action"));
    list.append(&row);
    rows.push(RowEntry {
        row,
        kind: RowKind::Chat,
    });
}

fn build_item_row(
    extension_capability_id: ExtensionCapabilityId,
    item: Item,
    dispatcher: &mpsc::UnboundedSender<Msg>,
) -> RowEntry {
    let content = item_content_row(&item);
    let row = flat_row(&content, None);

    if item.actions.len() > 1 {
        let chip = Label::builder()
            .label("Ctrl ↵")
            .valign(Align::Center)
            .css_classes(["paloma-keycap"])
            .build();
        content.append(&chip);

        let more_action_button = Button::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("More actions")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let action_dispatcher = dispatcher.clone();
        more_action_button.connect_clicked(move |button| {
            if let Some(row) = button
                .ancestor(ListBoxRow::static_type())
                .and_downcast::<ListBoxRow>()
                && let Some(list) = row.parent().and_downcast::<ListBox>()
            {
                list.select_row(Some(&row));
            }
            let _ = action_dispatcher.unbounded_send(Msg::Search(SearchMsg::OpenActionPanel));
        });

        // zero width until hovered, so the time sits flush right otherwise
        let reveal = Revealer::builder()
            .child(&more_action_button)
            .transition_type(RevealerTransitionType::SlideLeft)
            .transition_duration(150)
            .valign(Align::Center)
            .build();
        content.append(&reveal);

        row.connect_state_flags_changed(move |row, _| {
            reveal.set_reveal_child(row.state_flags().contains(StateFlags::PRELIGHT));
        });
    }

    let primary_index = item
        .actions
        .iter()
        .position(|action| action.primary)
        .unwrap_or_default();

    RowEntry {
        row,
        kind: RowKind::Item {
            extension_capability_id,
            primary_index,
            actions: item.actions,
        },
    }
}

fn build_show_more_row(hidden: usize) -> ListBoxRow {
    let content = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Center)
        .build();
    content.append(&Image::from_icon_name("pan-down-symbolic"));
    content.append(&Label::new(Some(&format!("Show {hidden} more"))));

    flat_row(&content, Some("paloma-show-more"))
}

fn build_static_row(content: &impl IsA<Widget>) -> ListBoxRow {
    ListBoxRow::builder()
        .child(content)
        .activatable(false)
        .selectable(false)
        .can_focus(false)
        .css_classes(["paloma-static-row"])
        .build()
}

fn flat_row(content: &impl IsA<Widget>, extra_class: Option<&str>) -> ListBoxRow {
    let row = ListBoxRow::builder()
        .child(content)
        .activatable(true)
        .focusable(false)
        .can_focus(false)
        .css_classes(["paloma-item"])
        .build();
    if let Some(class) = extra_class {
        row.add_css_class(class);
    }
    row
}

fn item_content_row(item: &Item) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .build();

    let icon = item.icon.as_ref().and_then(|icon| icon.icon.as_ref());
    let image = icon_image(icon.map(Into::into), 28, None);
    image.add_css_class("paloma-item-icon");
    row.append(&image);

    let text = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .valign(Align::Center)
        .build();

    let title = Label::builder()
        .label(&item.title)
        .xalign(0.0)
        .halign(Align::Start)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .single_line_mode(true)
        .css_classes(["paloma-item-title"])
        .build();
    text.append(&title);

    if let Some(subtitle) = item.subtitle.as_deref() {
        let subtitle = Label::builder()
            .label(subtitle)
            .xalign(0.0)
            .halign(Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::Middle)
            .single_line_mode(true)
            .css_classes(["paloma-item-subtitle"])
            .build();
        text.append(&subtitle);
    }
    row.append(&text);

    row
}
