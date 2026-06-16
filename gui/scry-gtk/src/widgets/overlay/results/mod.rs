//! Capability-results card: per-handler sections of activatable rows,
//! plus the "Chat about it" pseudo-row. Rows register with the shared
//! [`Selection`](super::selection::Selection) for keyboard navigation;
//! they are never GTK-focusable so the search entry keeps focus.

use std::rc::Rc;

use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, Orientation, Widget, glib, prelude::*,
    subclass::prelude::*,
};
use scry_core::{IconRef, Item};

use super::{
    CHAT_ACTION_LABEL, CHEVRON_COLLAPSED, InvokeFn, OVERLAY_WIDTH_PX,
    selection::{SelectableRow, SelectionRef},
};
use crate::widgets::clear_children;

/// Result card, section, and row styling.
pub(super) const CSS: &str = include_str!("style.css");

mod imp {
    use gtk4::{Box as GtkBox, CompositeTemplate, glib, subclass::prelude::*};

    #[derive(CompositeTemplate, Default)]
    #[template(file = "results.ui")]
    pub struct ResultsView {
        #[template_child]
        pub sections: TemplateChild<GtkBox>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ResultsView {
        const NAME: &'static str = "ScryResults";
        type Type = super::ResultsView;
        type ParentType = GtkBox;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ResultsView {}
    impl WidgetImpl for ResultsView {}
    impl BoxImpl for ResultsView {}
}

glib::wrapper! {
    pub struct ResultsView(ObjectSubclass<imp::ResultsView>)
        @extends GtkBox, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl ResultsView {
    pub(super) fn new() -> Self {
        let view: Self = glib::Object::new();
        view.set_width_request(OVERLAY_WIDTH_PX);
        view
    }

    pub(super) fn clear(&self, selection: &SelectionRef) {
        selection.borrow_mut().clear();
        self.set_visible(false);
        clear_children(&self.imp().sections);
    }

    /// Append a capability section (header + one row per item) and
    /// register the rows for keyboard navigation.
    pub(super) fn append_section(
        &self,
        selection: &SelectionRef,
        handler_id: &str,
        handler_name: &str,
        items: Vec<Item>,
        on_invoke: InvokeFn,
    ) {
        if items.is_empty() {
            return;
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
                handler_id.to_string(),
                item,
                &on_invoke,
                selection,
            );
            section.append(&widget);
            rows.push(selectable);
        }

        self.push_section(&section);
        selection.borrow_mut().append_rows(rows);
    }

    /// Append the "Chat about it" pseudo-row; `invoke` enters chat mode.
    pub(super) fn append_chat_action(&self, selection: &SelectionRef, invoke: Rc<dyn Fn()>) {
        let row_start = selection.borrow().len();
        let section = new_section();

        let item = Item {
            title: CHAT_ACTION_LABEL.to_string(),
            icon: Some(IconRef::Name("dialog-question-symbolic".to_string())),
            actions: Vec::new(),
        };
        let (row, selectable) = build_button_row(
            row_start,
            &item,
            Some("scry-chat-action"),
            vec![invoke],
            selection,
        );
        section.append(&row);

        self.push_section(&section);
        selection.borrow_mut().append_rows(vec![selectable]);
    }

    fn push_section(&self, section: &GtkBox) {
        self.imp().sections.append(section);
        self.set_visible(true);
    }
}

fn new_section() -> GtkBox {
    GtkBox::builder().orientation(Orientation::Vertical).build()
}

fn build_row(
    row_idx: usize,
    handler_id: String,
    item: Item,
    on_invoke: &InvokeFn,
    selection: &SelectionRef,
) -> (Widget, SelectableRow) {
    if item.actions.len() <= 1 {
        let invokers = build_invokers(&handler_id, &item, on_invoke);
        build_button_row(row_idx, &item, None, invokers, selection)
    } else {
        build_expandable_row(row_idx, handler_id, item, on_invoke, selection)
    }
}

/// Single-action row: a flat button that selects itself and fires its
/// first invoker on click.
fn build_button_row(
    row_idx: usize,
    item: &Item,
    extra_class: Option<&str>,
    invokers: Vec<Rc<dyn Fn()>>,
    selection: &SelectionRef,
) -> (Widget, SelectableRow) {
    let content = item_content_row(item);
    let button = Button::builder().child(&content).build();
    button.add_css_class("flat");
    button.add_css_class("scry-item");
    if let Some(class) = extra_class {
        button.add_css_class(class);
    }
    keep_entry_focused(&button);

    let invokers_for_click = invokers.clone();
    let selection_for_click = selection.clone();
    button.connect_clicked(move |_| {
        selection_for_click.borrow_mut().select_row(row_idx);
        if let Some(invoke) = invokers_for_click.first() {
            invoke();
        }
    });

    let widget: Widget = button.upcast();
    let selectable = SelectableRow {
        row: widget.clone(),
        actions: Vec::new(),
        invokers,
        expand_target: None,
    };
    (widget, selectable)
}

fn build_expandable_row(
    row_idx: usize,
    handler_id: String,
    item: Item,
    on_invoke: &InvokeFn,
    selection: &SelectionRef,
) -> (Widget, SelectableRow) {
    let container = GtkBox::builder().orientation(Orientation::Vertical).build();
    container.add_css_class("scry-item");

    let header_row = item_content_row(&item);
    let chevron = Image::from_icon_name(CHEVRON_COLLAPSED);
    chevron.add_css_class("scry-item-chevron");
    chevron.set_pixel_size(16);
    header_row.append(&chevron);

    let header = Button::builder().child(&header_row).build();
    header.add_css_class("flat");
    header.add_css_class("scry-item-header");
    keep_entry_focused(&header);
    container.append(&header);

    let actions_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .visible(false)
        .build();
    actions_box.add_css_class("scry-item-actions");
    container.append(&actions_box);

    let mut action_widgets = Vec::with_capacity(item.actions.len());
    let mut invokers = Vec::with_capacity(item.actions.len());

    for (idx, action) in item.actions.iter().enumerate() {
        let btn = Button::builder().label(&action.label).build();
        btn.add_css_class("flat");
        btn.add_css_class("scry-item-action");
        keep_entry_focused(&btn);
        actions_box.append(&btn);

        let on_invoke = Rc::clone(on_invoke);
        let handler_id_for_invoke = handler_id.clone();
        let action_for_invoke = action.clone();
        let invoke: Rc<dyn Fn()> = Rc::new(move || {
            on_invoke(handler_id_for_invoke.clone(), action_for_invoke.clone());
        });

        let invoke_for_click = invoke.clone();
        let selection_for_click = selection.clone();
        btn.connect_clicked(move |_| {
            selection_for_click.borrow_mut().select_action(row_idx, idx);
            invoke_for_click();
        });

        action_widgets.push(btn.upcast());
        invokers.push(invoke);
    }

    let selection_for_click = selection.clone();
    header.connect_clicked(move |_| {
        selection_for_click.borrow_mut().toggle_row(row_idx);
    });

    let widget: Widget = container.upcast();
    let selectable = SelectableRow {
        row: header.upcast(),
        actions: action_widgets,
        invokers,
        expand_target: Some((actions_box, chevron)),
    };
    (widget, selectable)
}

fn build_invokers(handler_id: &str, item: &Item, on_invoke: &InvokeFn) -> Vec<Rc<dyn Fn()>> {
    item.actions
        .iter()
        .map(|action| {
            let on_invoke = Rc::clone(on_invoke);
            let handler_id = handler_id.to_string();
            let action = action.clone();
            Rc::new(move || on_invoke(handler_id.clone(), action.clone())) as Rc<dyn Fn()>
        })
        .collect()
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

/// Rows must never steal GTK focus — the search entry owns it so the
/// user can keep typing; row "selection" is a CSS class, not focus.
fn keep_entry_focused(button: &Button) {
    button.set_focusable(false);
    button.set_can_focus(false);
}
