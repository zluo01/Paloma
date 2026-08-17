use std::collections::HashMap;

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, ListBox, ListBoxRow, Orientation, Revealer,
    RevealerTransitionType, SelectionMode, Separator, StateFlags, Widget, prelude::*,
};
use paloma_core::{Action, CapabilityIcon, ExtensionCapabilityId, Item};

use crate::{
    helper::icon_image,
    widgets::overlay::{
        SELECTED_CLASS,
        model::{ChatMsg, Msg, SearchMsg},
    },
};

const CHAT_ACTION_LABEL: &str = "Chat about it";

const MAX_SECTION_ITEMS: usize = 5;

#[derive(Clone)]
pub(super) struct SearchAction {
    pub(super) row: ListBoxRow,
    pub(super) extension_capability_id: ExtensionCapabilityId,
    pub(super) panel_actions: Vec<Action>,
}

pub(super) struct Section {
    view: GtkBox,
    actions: Vec<SearchAction>,
    show_more: Option<ListBoxRow>,
}

impl Section {
    pub(super) fn search_section(
        section_index: usize,
        extension_capability_id: ExtensionCapabilityId,
        handler_name: &str,
        mut items: Vec<Item>,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let view = GtkBox::builder().orientation(Orientation::Vertical).build();

        let header = Label::builder()
            .label(handler_name)
            .xalign(0.0)
            .halign(Align::Start)
            .css_classes(["paloma-section-header"])
            .build();
        view.append(&header);

        let tail = if items.len() > MAX_SECTION_ITEMS + 1 {
            items.split_off(MAX_SECTION_ITEMS)
        } else {
            Vec::new()
        };

        let search_section_list = section_list();
        let mut actions = Vec::with_capacity(items.len() + tail.len());
        let mut primaries = HashMap::with_capacity(items.len() + tail.len());
        for (index, item) in items.iter().enumerate() {
            let (action, primary) = build_item_row(
                (section_index, index),
                extension_capability_id.clone(),
                item,
                dispatcher.clone(),
            );
            search_section_list.append(&action.row);
            primaries.insert(action.row.clone(), primary);
            actions.push(action);
        }

        let mut show_more = None;
        if !tail.is_empty() {
            let row = build_show_more_row(tail.len());
            search_section_list.append(&row);
            show_more = Some(row);

            for (index, item) in tail.iter().enumerate() {
                let (action, primary) = build_item_row(
                    (section_index, items.len() + index),
                    extension_capability_id.clone(),
                    item,
                    dispatcher.clone(),
                );
                action.row.set_visible(false);
                search_section_list.append(&action.row);
                primaries.insert(action.row.clone(), primary);
                actions.push(action);
            }
        }

        let show_more_row = show_more.clone();
        let first_tail_row = actions
            .get(MAX_SECTION_ITEMS)
            .map(|action| action.row.clone());
        search_section_list.connect_row_activated(move |_, row| {
            if show_more_row.as_ref() == Some(row) {
                row.set_visible(false);
                for item_row in primaries.keys() {
                    item_row.set_visible(true);
                }
                if row.has_css_class(SELECTED_CLASS)
                    && let Some(first_tail_row) = &first_tail_row
                {
                    row.remove_css_class(SELECTED_CLASS);
                    first_tail_row.add_css_class(SELECTED_CLASS);
                }
                return;
            }
            if let Some(action) = primaries.get(row) {
                let _ = dispatcher.unbounded_send(Msg::Search(SearchMsg::ResultActionRequested {
                    extension_capability_id: extension_capability_id.clone(),
                    action: action.clone(),
                }));
            }
        });

        view.append(&search_section_list);

        Self {
            view,
            actions,
            show_more,
        }
    }

    pub(super) fn chat_section(
        action_count: usize,
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let section = GtkBox::builder().orientation(Orientation::Vertical).build();
        // A divider sets the chat action apart from the launcher results above.
        if action_count > 0 {
            let separator = Separator::builder()
                .orientation(Orientation::Horizontal)
                .margin_top(8)
                .margin_bottom(4)
                .margin_end(4)
                .build();
            section.append(&separator);
        }

        let item = Item {
            title: CHAT_ACTION_LABEL.to_string(),
            subtitle: None,
            icon: Some(CapabilityIcon::name("dialog-question-symbolic")),
            actions: Vec::new(),
        };
        let row = flat_row(&item_content_row(&item), Some("paloma-chat-action"));

        let chat_section_list = section_list();
        chat_section_list.append(&row);
        chat_section_list.connect_row_activated(move |_, _| {
            let _ = dispatcher.unbounded_send(Msg::Chat(ChatMsg::PromptSubmitRequested));
        });
        section.append(&chat_section_list);

        Self {
            view: section,
            actions: vec![SearchAction {
                row,
                extension_capability_id: ExtensionCapabilityId::default(),
                panel_actions: vec![],
            }],
            show_more: None,
        }
    }

    pub(super) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(super) fn len(&self) -> usize {
        if self.show_more.as_ref().is_some_and(WidgetExt::get_visible) {
            return MAX_SECTION_ITEMS + 1;
        }
        self.actions.len()
    }

    pub(super) fn action(&self, index: usize) -> Option<SearchAction> {
        if self.show_more.as_ref().is_some_and(WidgetExt::get_visible) && index == MAX_SECTION_ITEMS
        {
            return self.show_more.clone().map(|row| SearchAction {
                row,
                extension_capability_id: ExtensionCapabilityId::default(),
                panel_actions: vec![],
            });
        }
        self.actions.get(index).cloned()
    }
}

fn build_item_row(
    target: (usize, usize),
    extension_capability_id: ExtensionCapabilityId,
    item: &Item,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> (SearchAction, Action) {
    let primary = item
        .actions
        .iter()
        .find(|a| a.primary)
        .or_else(|| item.actions.first())
        .cloned()
        .unwrap();

    let content = item_content_row(item);
    let row = flat_row(&content, None);

    let mut panel_actions = vec![];
    if item.actions.len() > 1 {
        panel_actions = item.actions.clone();

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
        more_action_button.connect_clicked(move |_| {
            let _ = dispatcher.unbounded_send(Msg::Search(SearchMsg::OpenActionPanel {
                target: Some(target),
            }));
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

    (
        SearchAction {
            row,
            extension_capability_id,
            panel_actions,
        },
        primary,
    )
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

fn section_list() -> ListBox {
    ListBox::builder()
        .selection_mode(SelectionMode::None)
        .css_classes(["paloma-section-list"])
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
