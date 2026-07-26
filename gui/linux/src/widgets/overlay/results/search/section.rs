use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, Orientation, Revealer, RevealerTransitionType,
    Separator, Widget, prelude::*,
};
use scry_core::{Action, CapabilityIcon, ExtensionCapabilityId, Item, capability_icon};

use crate::widgets::overlay::model::{ChatMsg, Msg, SearchMsg};

const CHAT_ACTION_LABEL: &str = "Chat about it";

const MAX_SECTION_ITEMS: usize = 5;

#[derive(Clone)]
pub(super) struct SearchAction {
    pub(super) button: Button,
    pub(super) extension_capability_id: ExtensionCapabilityId,
    pub(super) panel_actions: Vec<Action>,
}

pub(super) struct Section {
    view: GtkBox,
    actions: Vec<SearchAction>,
    show_more: Option<SearchAction>,
}

impl Section {
    pub(super) fn search_section(
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
            .css_classes(["scry-section-header"])
            .build();
        view.append(&header);

        let tail = if items.len() > MAX_SECTION_ITEMS + 1 {
            items.split_off(MAX_SECTION_ITEMS)
        } else {
            Vec::new()
        };

        let mut actions = Vec::with_capacity(items.len() + tail.len());
        for item in &items {
            let row = build_item_row(extension_capability_id.clone(), item, &dispatcher);
            view.append(&row.button);
            actions.push(row);
        }

        let mut show_more = None;
        if !tail.is_empty() {
            let folded = GtkBox::builder().orientation(Orientation::Vertical).build();
            for item in &tail {
                let row = build_item_row(extension_capability_id.clone(), item, &dispatcher);
                folded.append(&row.button);
                actions.push(row);
            }
            let revealer = Revealer::builder()
                .transition_type(RevealerTransitionType::SlideDown)
                .child(&folded)
                .build();

            let row = build_show_more_row(tail.len());
            let reveal = revealer.clone();
            row.button.connect_clicked(move |button| {
                button.set_visible(false);
                reveal.set_reveal_child(true);
            });
            view.append(&row.button);
            view.append(&revealer);
            show_more = Some(row);
        }

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
        let button = flat_button(&item_content_row(&item), Some("scry-chat-action"));
        let action_dispatcher = dispatcher.clone();
        button.connect_clicked(move |_| {
            let _ = action_dispatcher.unbounded_send(Msg::Chat(ChatMsg::PromptSubmitRequested));
        });

        section.append(&button);

        Self {
            view: section,
            actions: vec![SearchAction {
                button,
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
        if self
            .show_more
            .as_ref()
            .is_some_and(|row| row.button.get_visible())
        {
            return MAX_SECTION_ITEMS + 1;
        }
        self.actions.len()
    }

    pub(super) fn action(&self, index: usize) -> Option<SearchAction> {
        if self
            .show_more
            .as_ref()
            .is_some_and(|row| row.button.get_visible())
            && index == MAX_SECTION_ITEMS
        {
            return self.show_more.clone();
        }
        self.actions.get(index).cloned()
    }
}

fn build_item_row(
    extension_capability_id: ExtensionCapabilityId,
    item: &Item,
    dispatcher: &mpsc::UnboundedSender<Msg>,
) -> SearchAction {
    if item.actions.len() == 1 {
        build_row(extension_capability_id, item, dispatcher.clone())
    } else {
        build_actionable_row(extension_capability_id, item, dispatcher.clone())
    }
}

fn build_row(
    extension_capability_id: ExtensionCapabilityId,
    item: &Item,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> SearchAction {
    let content = item_content_row(item);
    let button = flat_button(&content, None);
    let action = item.actions.first().unwrap().clone();

    let action_dispatcher = dispatcher.clone();
    let id = extension_capability_id.clone();
    button.connect_clicked(move |_| {
        let _ = action_dispatcher.unbounded_send(Msg::Search(SearchMsg::ResultActionRequested {
            extension_capability_id: id.clone(),
            action: action.clone(),
        }));
    });

    SearchAction {
        button,
        extension_capability_id,
        panel_actions: vec![],
    }
}

fn build_actionable_row(
    extension_capability_id: ExtensionCapabilityId,
    item: &Item,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> SearchAction {
    let primary_idx = item.actions.iter().position(|a| a.primary).unwrap_or(0);
    let primary = item.actions.get(primary_idx).cloned().unwrap();

    let content = item_content_row(item);
    let chip = Label::builder()
        .label("Ctrl ↵")
        .valign(Align::Center)
        .css_classes(["scry-keycap"])
        .build();
    content.append(&chip);

    let button = flat_button(&content, None);

    let primary_action = primary.clone();
    let action_dispatcher = dispatcher.clone();
    let id = extension_capability_id.clone();
    button.connect_clicked(move |_| {
        let _ = action_dispatcher.unbounded_send(Msg::Search(SearchMsg::ResultActionRequested {
            extension_capability_id: id.clone(),
            action: primary_action.clone(),
        }));
    });

    SearchAction {
        button,
        extension_capability_id,
        panel_actions: item.actions.clone(),
    }
}

fn build_show_more_row(hidden: usize) -> SearchAction {
    let content = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::Center)
        .build();
    content.append(&Image::from_icon_name("pan-down-symbolic"));
    content.append(&Label::new(Some(&format!("Show {hidden} more"))));

    SearchAction {
        button: flat_button(&content, Some("scry-show-more")),
        extension_capability_id: ExtensionCapabilityId::default(),
        panel_actions: vec![],
    }
}

fn flat_button(content: &impl IsA<Widget>, extra_class: Option<&str>) -> Button {
    let button = Button::builder()
        .child(content)
        .focusable(false)
        .can_focus(false)
        .css_classes(["flat", "scry-item"])
        .build();
    if let Some(class) = extra_class {
        button.add_css_class(class);
    }
    button
}

fn item_content_row(item: &Item) -> GtkBox {
    let row = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .build();

    let image = match item.icon.as_ref().and_then(|icon| icon.icon.as_ref()) {
        Some(capability_icon::Icon::Name(name)) => Image::from_icon_name(name),
        Some(capability_icon::Icon::Path(path)) => Image::from_file(path),
        Some(capability_icon::Icon::Embedded(_)) | None => Image::new(),
    };
    image.add_css_class("scry-item-icon");
    image.set_pixel_size(28);
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
        .css_classes(["scry-item-title"])
        .build();
    text.append(&title);

    if let Some(subtitle) = item.subtitle.as_deref() {
        let subtitle = Label::builder()
            .label(subtitle)
            .xalign(0.0)
            .halign(Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::Middle)
            .single_line_mode(true)
            .css_classes(["scry-item-subtitle"])
            .build();
        text.append(&subtitle);
    }
    row.append(&text);

    row
}
