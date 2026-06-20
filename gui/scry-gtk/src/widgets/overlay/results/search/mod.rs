mod action_panel;

use std::cell::{Cell, RefCell};

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, Orientation, ScrolledWindow, Separator, Widget,
    prelude::*,
};
use scry_core::{Action, IconRef, Item};

use crate::widgets::{
    clear_children,
    overlay::{
        CHAT_ACTION_LABEL, OVERLAY_WIDTH_PX, SELECTED_CLASS,
        model::Msg,
        results::{search::action_panel::ActionPanel, step_index},
        scroll_into_view,
    },
};

struct SearchAction {
    button: Button,
    handler_id: &'static str,
    panel_actions: Vec<Action>,
}

pub struct SearchView {
    widget: GtkBox,
    actions: RefCell<Vec<SearchAction>>,
    dispatcher: mpsc::UnboundedSender<Msg>,
    selected: Cell<Option<usize>>,
    action_panel: RefCell<Option<ActionPanel>>,
}

impl SearchView {
    pub(crate) fn new(dispatcher: mpsc::UnboundedSender<Msg>) -> Self {
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .visible(false)
            .width_request(OVERLAY_WIDTH_PX)
            .css_classes(["scry-result-card"])
            .build();
        Self {
            widget,
            actions: RefCell::new(vec![]),
            dispatcher,
            selected: Cell::new(None),
            action_panel: RefCell::new(None),
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.widget
    }

    pub(crate) fn clear(&self) {
        self.actions.borrow_mut().clear();
        self.selected.set(None);

        if let Some(panel) = self.action_panel.borrow_mut().take() {
            panel.close();
        }

        self.widget.set_visible(false);
        clear_children(&self.widget);
    }

    pub(crate) fn append_section(
        &self,
        handler_id: &'static str,
        handler_name: &str,
        items: Vec<Item>,
    ) -> bool {
        let items: Vec<Item> = items
            .into_iter()
            .filter(|item| !item.actions.is_empty())
            .collect();
        if items.is_empty() {
            return false;
        }

        let section = GtkBox::builder().orientation(Orientation::Vertical).build();

        let header = Label::builder()
            .label(handler_name)
            .xalign(0.0)
            .halign(Align::Start)
            .css_classes(["scry-section-header"])
            .build();
        section.append(&header);

        let mut rows = Vec::with_capacity(items.len());
        for item in items.into_iter() {
            let result_action = if item.actions.len() == 1 {
                build_row(handler_id, &item, self.dispatcher.clone())
            } else {
                build_actionable_row(handler_id, &item, self.dispatcher.clone())
            };
            section.append(&result_action.button);
            rows.push(result_action);
        }

        self.push_section(&section);
        self.actions.borrow_mut().append(&mut rows);
        true
    }

    /// Append the "Chat about it" pseudo-row; `invoke` enters chat mode.
    pub(crate) fn append_chat_action(&self) {
        let row_idx = self.actions.borrow().len();
        let section = GtkBox::builder().orientation(Orientation::Vertical).build();
        // A divider sets the chat action apart from the launcher results above.
        if row_idx > 0 {
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
            icon: Some(IconRef::Name("dialog-question-symbolic".to_string())),
            actions: Vec::new(),
        };
        let button = flat_button(&item_content_row(&item), Some("scry-chat-action"));
        let action_dispatcher = self.dispatcher.clone();
        button.connect_clicked(move |_| {
            let _ = action_dispatcher.unbounded_send(Msg::ChatPromptSubmitted);
        });

        section.append(&button);

        self.push_section(&section);
        self.actions.borrow_mut().push(SearchAction {
            button,
            handler_id: "",
            panel_actions: vec![],
        });
    }

    pub(crate) fn open_action_panel(&self) {
        if self.is_action_panel_open() {
            return;
        }
        let Some(selected) = self.selected.get() else {
            return;
        };

        let Some((button, handler_id, actions)) =
            self.actions.borrow().get(selected).and_then(|row| {
                (row.panel_actions.len() > 1).then(|| {
                    (
                        row.button.clone(),
                        row.handler_id,
                        row.panel_actions.clone(),
                    )
                })
            })
        else {
            return;
        };

        *self.action_panel.borrow_mut() = Some(ActionPanel::new(
            &button,
            handler_id,
            actions,
            self.dispatcher.clone(),
        ));
    }

    pub(crate) fn activate(&self) -> bool {
        let panel_button = self
            .action_panel
            .borrow()
            .as_ref()
            .filter(|panel| panel.is_open())
            .and_then(ActionPanel::selected_button);
        if let Some(button) = panel_button {
            button.emit_clicked();
            return true;
        }

        let Some(selected) = self.selected.get() else {
            return false;
        };

        let row_button = self
            .actions
            .borrow()
            .get(selected)
            .map(|action| action.button.clone());
        if let Some(button) = row_button {
            button.emit_clicked();
            return true;
        }

        false
    }

    pub(crate) fn navigate(&self, delta: i32) -> bool {
        if let Some(panel) = self.action_panel.borrow().as_ref()
            && panel.is_open()
        {
            panel.navigate(delta);
            return true;
        }

        let actions_len = self.actions.borrow().len();
        let next = match self.selected.get() {
            Some(current) => step_index(current, delta, actions_len),
            None if actions_len > 0 => Some(0),
            None => None,
        };

        match next {
            None => false,
            Some(next) => {
                self.select_row(next);
                self.scroll_selection_into_view();
                true
            },
        }
    }

    pub(crate) fn is_action_panel_open(&self) -> bool {
        self.action_panel
            .borrow()
            .as_ref()
            .is_some_and(ActionPanel::is_open)
    }

    pub(crate) fn close_action_panel(&self) -> bool {
        let Some(panel) = self.action_panel.borrow_mut().take() else {
            return false;
        };

        if panel.is_open() {
            panel.close();
            return true;
        }

        false
    }

    fn select_row(&self, idx: usize) {
        if self.selected.get() == Some(idx) {
            return;
        }

        let Some(button) = self
            .actions
            .borrow()
            .get(idx)
            .map(|action| action.button.clone())
        else {
            return;
        };

        self.clear_selected();
        button.add_css_class(SELECTED_CLASS);
        self.selected.set(Some(idx));
    }

    fn clear_selected(&self) {
        if let Some(idx) = self.selected.get() {
            self.selected.set(None);
            if let Some(button) = self
                .actions
                .borrow()
                .get(idx)
                .map(|action| action.button.clone())
            {
                button.remove_css_class(SELECTED_CLASS);
            }
        }
    }

    fn push_section(&self, section: &GtkBox) {
        self.widget.append(section);
        self.widget.set_visible(true);
    }

    fn scroll_selection_into_view(&self) {
        let Some(idx) = self.selected.get() else {
            return;
        };
        let Some(button) = self
            .actions
            .borrow()
            .get(idx)
            .map(|action| action.button.clone())
        else {
            return;
        };
        let Some(scroller) = button
            .ancestor(ScrolledWindow::static_type())
            .and_downcast::<ScrolledWindow>()
        else {
            return;
        };
        // The first/last rows snap the card fully to the top/bottom so its
        // padding (and the chat divider) isn't clipped; the rows sit below the
        // card padding, so a minimal scroll-to would leave that padding cut off.
        // Middle rows use the minimal scroll.
        let adj = scroller.vadjustment();
        let len = self.actions.borrow().len();
        match idx {
            0 => adj.set_value(0.0),
            i if i + 1 == len => adj.set_value((adj.upper() - adj.page_size()).max(0.0)),
            _ => scroll_into_view(&button),
        }
    }

    pub(crate) fn render_any(&self) -> bool {
        !self.actions.borrow().is_empty()
    }
}

fn build_row(
    handler_id: &'static str,
    item: &Item,
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> SearchAction {
    let content = item_content_row(item);
    let button = flat_button(&content, None);
    let action = item.actions.first().unwrap().clone();

    let action_dispatcher = dispatcher.clone();
    button.connect_clicked(move |_| {
        let _ = action_dispatcher.unbounded_send(Msg::LocalQueryResultActionRequested {
            handler_id,
            action: action.clone(),
        });
    });

    SearchAction {
        button,
        handler_id,
        panel_actions: vec![],
    }
}

fn build_actionable_row(
    handler_id: &'static str,
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
    button.connect_clicked(move |_| {
        let _ = action_dispatcher.unbounded_send(Msg::LocalQueryResultActionRequested {
            handler_id,
            action: primary_action.clone(),
        });
    });

    SearchAction {
        button,
        handler_id,
        panel_actions: item.actions.clone(),
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

    let image = match item.icon.as_ref() {
        Some(IconRef::Name(name)) => Image::from_icon_name(name),
        Some(IconRef::Path(path)) => Image::from_file(path),
        Some(IconRef::Embedded { .. }) | None => Image::new(),
    };
    image.add_css_class("scry-item-icon");
    image.set_pixel_size(28);
    row.append(&image);

    let title = Label::builder()
        .label(&item.title)
        .xalign(0.0)
        .halign(Align::Start)
        .hexpand(true)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .single_line_mode(true)
        .css_classes(["scry-item-title"])
        .build();
    row.append(&title);

    row
}
