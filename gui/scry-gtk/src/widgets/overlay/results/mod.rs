//! Search results view: per-handler action rows plus the "Chat about it" row.
//! Rows use the shared [`Selection`](super::selection::Selection) for keyboard
//! navigation while the search entry keeps GTK focus.

use std::rc::Rc;

use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, Orientation, Separator, Widget, prelude::*,
};
use scry_core::{IconRef, Item};

use super::{
    CHAT_ACTION_LABEL, InvokeFn, OVERLAY_WIDTH_PX,
    selection::{RowAction, SelectableRow, SelectionRef},
};
use crate::widgets::clear_children;

/// Result card, section, and row styling.
pub(super) const CSS: &str = include_str!("style.css");

/// Styled results card. Clones share the same GTK widgets.
#[derive(Clone)]
pub(super) struct ResultsView {
    widget: GtkBox,
    sections: GtkBox,
}

impl ResultsView {
    pub(super) fn new() -> Self {
        let sections = GtkBox::builder().orientation(Orientation::Vertical).build();
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .visible(false)
            .width_request(OVERLAY_WIDTH_PX)
            .build();
        widget.add_css_class("scry-result-card");
        widget.append(&sections);
        Self { widget, sections }
    }

    pub(super) fn widget(&self) -> &GtkBox {
        &self.widget
    }

    pub(super) fn clear(&self, selection: &SelectionRef) {
        selection.borrow_mut().clear();
        self.widget.set_visible(false);
        clear_children(&self.sections);
    }

    /// Append a capability section (header + one row per item) and register the
    /// rows for keyboard navigation. Items with no actions are skipped; returns
    /// whether the section rendered any rows.
    pub(super) fn append_section(
        &self,
        selection: &SelectionRef,
        handler_id: &'static str,
        handler_name: &str,
        items: Vec<Item>,
        on_invoke: InvokeFn,
        close_panel: &Rc<dyn Fn()>,
    ) -> bool {
        let items: Vec<Item> = items.into_iter().filter(has_actions).collect();
        if items.is_empty() {
            return false;
        }

        let row_start = selection.borrow().len();
        let section = new_section();

        let header = Label::builder()
            .label(handler_name)
            .xalign(0.0)
            .halign(Align::Start)
            .build();
        header.add_css_class("scry-section-header");
        section.append(&header);

        let mut rows = Vec::with_capacity(items.len());
        for (offset, item) in items.into_iter().enumerate() {
            let (widget, selectable) = build_row(
                row_start + offset,
                handler_id,
                &item,
                &on_invoke,
                selection,
                close_panel,
            );
            section.append(&widget);
            rows.push(selectable);
        }

        self.push_section(&section);
        selection.borrow_mut().append_rows(rows);
        true
    }

    /// Append the "Chat about it" pseudo-row; `invoke` enters chat mode.
    pub(super) fn append_chat_action(
        &self,
        selection: &SelectionRef,
        invoke: Rc<dyn Fn()>,
        close_panel: &Rc<dyn Fn()>,
    ) {
        let row_idx = selection.borrow().len();
        let section = new_section();
        // A divider sets the chat action apart from the launcher results above.
        if row_idx > 0 {
            let separator = Separator::new(Orientation::Horizontal);
            separator.add_css_class("scry-results-separator");
            section.append(&separator);
        }

        let item = Item {
            title: CHAT_ACTION_LABEL.to_string(),
            icon: Some(IconRef::Name("dialog-question-symbolic".to_string())),
            actions: Vec::new(),
        };
        let button = flat_button(&item_content_row(&item), Some("scry-chat-action"));
        let widget: Widget = button.clone().upcast();

        let selection_for_click = selection.clone();
        let invoke_for_click = invoke.clone();
        let close_panel = close_panel.clone();
        button.connect_clicked(move |_| {
            close_panel();
            selection_for_click.borrow_mut().select_row(row_idx);
            invoke_for_click();
        });
        section.append(&widget);

        self.push_section(&section);
        selection.borrow_mut().append_rows(vec![SelectableRow {
            row: widget,
            primary: Some(invoke),
            actions: Vec::new(),
        }]);
    }

    fn push_section(&self, section: &GtkBox) {
        self.sections.append(section);
        self.widget.set_visible(true);
    }
}

fn new_section() -> GtkBox {
    GtkBox::builder().orientation(Orientation::Vertical).build()
}

/// Whether an item is renderable. Action-less items are skipped: they'd be inert
/// selected rows.
fn has_actions(item: &Item) -> bool {
    !item.actions.is_empty()
}

/// Whether a response renders anything — at least one action-bearing item. Drives
/// the controller's "keep stale results until the new query shows something" gate
/// and the chat-vs-no-results path.
pub(super) fn renders_any(items: &[Item]) -> bool {
    items.iter().any(has_actions)
}

/// A single flat result row: runs its primary action on click, and (for
/// multi-action items) shows a persistent `Ctrl K` action-panel hint.
fn build_row(
    row_idx: usize,
    handler_id: &'static str,
    item: &Item,
    on_invoke: &InvokeFn,
    selection: &SelectionRef,
    close_panel: &Rc<dyn Fn()>,
) -> (Widget, SelectableRow) {
    let actions: Vec<RowAction> = item
        .actions
        .iter()
        .map(|action| {
            let on_invoke = Rc::clone(on_invoke);
            let action = action.clone();
            RowAction {
                label: action.label.clone(),
                invoke: Rc::new(move || on_invoke(handler_id, action.clone())),
            }
        })
        .collect();
    // Enter runs the primary action: the flagged one, else the only/first one.
    let primary_idx = item.actions.iter().position(|a| a.primary).unwrap_or(0);
    let primary = actions.get(primary_idx).map(|a| a.invoke.clone());

    let content = item_content_row(item);
    // Rows with a panel always advertise it (not only when selected), so every
    // multi-action row shows the same affordance.
    if actions.len() > 1 {
        let chip = Label::new(Some("Ctrl K"));
        chip.add_css_class("scry-keycap");
        chip.set_valign(Align::Center);
        content.append(&chip);
    }

    let button = flat_button(&content, None);
    let widget: Widget = button.clone().upcast();

    let selection_for_click = selection.clone();
    let primary_for_click = primary.clone();
    let close_panel = close_panel.clone();
    button.connect_clicked(move |_| {
        close_panel();
        selection_for_click.borrow_mut().select_row(row_idx);
        if let Some(primary) = &primary_for_click {
            primary();
        }
    });

    let selectable = SelectableRow {
        row: widget.clone(),
        primary,
        actions,
    };
    (widget, selectable)
}

/// A flat, non-focusable button row (selection is a CSS class, not GTK focus).
fn flat_button(content: &impl IsA<Widget>, extra_class: Option<&str>) -> Button {
    let button = Button::builder().child(content).build();
    button.add_css_class("flat");
    button.add_css_class("scry-item");
    if let Some(class) = extra_class {
        button.add_css_class(class);
    }
    keep_entry_focused(&button);
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
        .build();
    title.add_css_class("scry-item-title");
    row.append(&title);

    row
}

/// Keep GTK focus on the search entry; row selection is a CSS class.
fn keep_entry_focused(button: &Button) {
    button.set_focusable(false);
    button.set_can_focus(false);
}

#[cfg(test)]
mod tests {
    use scry_core::{Action, Item};

    use super::{has_actions, renders_any};

    fn action() -> Action {
        Action {
            label: "Open".into(),
            params: Vec::new(),
            primary: true,
        }
    }

    fn item(actions: Vec<Action>) -> Item {
        Item {
            title: "item".into(),
            icon: None,
            actions,
        }
    }

    #[test]
    fn item_with_an_action_is_renderable() {
        assert!(has_actions(&item(vec![action()])));
    }

    #[test]
    fn item_without_actions_is_skipped() {
        assert!(!has_actions(&item(Vec::new())));
    }

    #[test]
    fn response_renders_when_any_item_has_actions() {
        assert!(renders_any(&[item(Vec::new()), item(vec![action()])]));
    }

    #[test]
    fn response_with_only_action_less_items_renders_nothing() {
        assert!(!renders_any(&[item(Vec::new()), item(Vec::new())]));
    }

    #[test]
    fn empty_response_renders_nothing() {
        assert!(!renders_any(&[]));
    }
}
