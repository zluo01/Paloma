use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use gtk4::{
    Align, Box as GtkBox, Button, Entry, ListBoxRow, Orientation, glib, prelude::*,
    subclass::prelude::*,
};
use libadwaita::{ExpanderRow, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::KeyValueRow)]
    pub struct KeyValueRow {
        pub(super) entries: RefCell<Vec<(Entry, Entry)>>,
        pub(super) ignore_case: Cell<bool>,
        #[property(get)]
        pub(super) has_duplicates: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for KeyValueRow {
        const NAME: &'static str = "PalomaKeyValueRow";
        type Type = super::KeyValueRow;
        type ParentType = ExpanderRow;
    }

    #[glib::derived_properties]
    impl ObjectImpl for KeyValueRow {}

    impl WidgetImpl for KeyValueRow {}

    impl ListBoxRowImpl for KeyValueRow {}

    impl PreferencesRowImpl for KeyValueRow {}

    impl ExpanderRowImpl for KeyValueRow {}
}

glib::wrapper! {
    /// An expander row holding an editable list of key/value pairs.
    pub struct KeyValueRow(ObjectSubclass<imp::KeyValueRow>)
        @extends ExpanderRow, libadwaita::PreferencesRow, ListBoxRow, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl KeyValueRow {
    pub(super) fn new(title: &str, pairs: &HashMap<String, String>, ignore_case: bool) -> Self {
        let row: Self = glib::Object::builder()
            .property("title", title)
            .property("expanded", true)
            .build();
        row.imp().ignore_case.set(ignore_case);

        let add = Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();

        let weak = row.downgrade();
        add.connect_clicked(move |_| {
            if let Some(row) = weak.upgrade() {
                row.add_pair("", "");
            }
        });
        row.add_suffix(&add);

        let mut pairs: Vec<_> = pairs.iter().collect();
        pairs.sort();
        for (key, value) in pairs {
            row.add_pair(key, value);
        }
        if row.imp().entries.borrow().is_empty() {
            row.add_pair("", "");
        }
        row.flag_duplicates();
        row
    }

    pub(super) fn pairs(&self) -> HashMap<String, String> {
        self.imp()
            .entries
            .borrow()
            .iter()
            .map(|(key, value)| {
                (
                    key.text().trim().to_string(),
                    value.text().trim().to_string(),
                )
            })
            .filter(|(key, _)| !key.is_empty())
            .collect()
    }

    fn add_pair(&self, key: &str, value: &str) {
        let key = Entry::builder()
            .placeholder_text("Name")
            .text(key)
            .hexpand(true)
            .build();
        let value = Entry::builder()
            .placeholder_text("Value")
            .text(value)
            .hexpand(true)
            .build();
        let remove = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Remove")
            .valign(Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let content = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(6)
            .build();
        content.append(&key);
        content.append(&value);
        content.append(&remove);
        let pair = ListBoxRow::builder()
            .activatable(false)
            .child(&content)
            .build();
        self.add_row(&pair);
        self.imp().entries.borrow_mut().push((key.clone(), value));

        // Weak: the closures live on children of the row.
        let weak = self.downgrade();
        key.connect_changed({
            let weak = weak.clone();
            move |_| {
                if let Some(row) = weak.upgrade() {
                    row.flag_duplicates();
                }
            }
        });
        let pair = pair.downgrade();
        remove.connect_clicked(move |_| {
            let (Some(row), Some(pair)) = (weak.upgrade(), pair.upgrade()) else {
                return;
            };
            row.remove(&pair);
            row.imp()
                .entries
                .borrow_mut()
                .retain(|(entry, _)| entry != &key);
            row.flag_duplicates();
        });
    }

    fn flag_duplicates(&self) {
        let entries = self.imp().entries.borrow();
        let ignore_case = self.imp().ignore_case.get();
        let name = |key: &Entry| {
            let name = key.text().trim().to_string();
            if ignore_case {
                name.to_lowercase()
            } else {
                name
            }
        };

        let mut duplicates = HashMap::new();
        for (key, _) in entries.iter() {
            *duplicates.entry(name(key)).or_insert(0) += 1;
        }
        duplicates.retain(|name, count| !name.is_empty() && *count > 1);

        for (key, _) in entries.iter() {
            if duplicates.contains_key(&name(key)) {
                key.add_css_class("error");
                key.set_tooltip_text(Some("This name is used more than once."));
            } else {
                key.remove_css_class("error");
                key.set_tooltip_text(None);
            }
        }

        let has_duplicate = !duplicates.is_empty();
        if self.imp().has_duplicates.replace(has_duplicate) != has_duplicate {
            self.notify_has_duplicates();
        }
    }
}
