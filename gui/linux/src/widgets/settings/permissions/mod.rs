mod model;

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gtk4::{Align, Box as GtkBox, Button, Orientation, SearchEntry, glib, prelude::*};
use libadwaita::{
    ActionRow, ApplicationWindow, Clamp, PreferencesGroup, PreferencesPage, prelude::*,
};
use paloma_core::{AppContext, Permission};

use self::model::{Command, Msg, State};
use crate::{
    helper::Clear,
    runtime,
    widgets::settings::helper::{placeholder, show_error_dialog},
};

const SEARCH_DELAY_MS: u32 = 150;
const SEARCH_WIDTH_RATIO: f64 = 0.618;
static SEARCH_WIDTH: LazyLock<i32> =
    LazyLock::new(|| (Clamp::new().maximum_size() as f64 * SEARCH_WIDTH_RATIO).round() as i32);

pub(crate) struct PermissionsPage {
    view: GtkBox,
    permission_view: PreferencesPage,
    app_context: Arc<AppContext>,
    window: glib::WeakRef<ApplicationWindow>,
    state: RefCell<State>,
}

impl PermissionsPage {
    pub(crate) fn new(app_context: Arc<AppContext>, window: &ApplicationWindow) -> Rc<Self> {
        let search = SearchEntry::builder()
            .placeholder_text("Search permissions")
            .hexpand(true)
            .search_delay(SEARCH_DELAY_MS)
            .build();

        let search_clamp = Clamp::builder()
            .maximum_size(*SEARCH_WIDTH)
            .child(&search)
            .build();

        let page = PreferencesPage::builder().vexpand(true).build();

        let view = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .build();
        view.append(&search_clamp);
        view.append(&page);

        let permission_page = Rc::new(Self {
            view,
            permission_view: page,
            app_context,
            window: window.downgrade(),
            state: RefCell::new(State::default()),
        });

        let weak = Rc::downgrade(&permission_page);
        search.connect_search_changed(move |entry| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::SearchChanged(entry.text().to_string()));
            }
        });

        permission_page
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn refresh(self: &Rc<Self>) {
        let app_context = self.app_context.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app_context.get_permissions().await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::PermissionsLoaded(result));
                }
            },
        );
    }

    fn dispatch(self: &Rc<Self>, msg: Msg) {
        let commands = self.state.borrow_mut().update(msg);
        for command in commands {
            self.run(command);
        }
    }

    fn run(self: &Rc<Self>, command: Command) {
        match command {
            Command::Render => self.render(),
            Command::DeletePermission(prefix) => self.delete_permission(prefix),
            Command::ShowErrorDialog(message) => {
                if let Some(window) = self.window.upgrade() {
                    show_error_dialog(&window, "Permission Operation Failed", &message);
                }
            },
            Command::LogWarning(message) => log::warn!("{message}"),
        }
    }

    fn delete_permission(self: &Rc<Self>, prefix: String) {
        let app_context = self.app_context.clone();
        let prefix_for_delete = prefix.clone();
        let weak = Rc::downgrade(self);
        runtime::spawn_with(
            async move { app_context.delete_permission(&prefix_for_delete).await },
            move |result| {
                if let Some(this) = weak.upgrade() {
                    this.dispatch(Msg::DeleteFinished(prefix, result));
                }
            },
        );
    }

    fn render(self: &Rc<Self>) {
        self.permission_view.clear();

        let state = self.state.borrow();
        let sections = state.visible_sections();

        if sections.is_empty() {
            let text = if state.has_query() {
                "No permissions match the search."
            } else {
                "No saved permissions."
            };
            let group = PreferencesGroup::new();
            group.add(&placeholder(text));
            self.permission_view.add(&group);
        } else {
            for section in sections {
                let group = PreferencesGroup::builder().title(&section.title).build();
                for permission in section.permissions {
                    let deleting = state.is_deleting(&permission.prefix);
                    group.add(&self.permission_row(permission, deleting));
                }
                self.permission_view.add(&group);
            }
        }
    }

    fn permission_row(self: &Rc<Self>, permission: &Permission, deleting: bool) -> ActionRow {
        let row = ActionRow::builder()
            .title(&permission.prefix)
            .title_lines(2)
            .subtitle(permission_subtitle(permission))
            .build();
        row.add_suffix(&self.delete_button(permission.prefix.as_str(), deleting));
        row
    }

    fn delete_button(self: &Rc<Self>, prefix: &str, deleting: bool) -> Button {
        let button = Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Delete permission")
            .valign(Align::Center)
            .sensitive(!deleting)
            .css_classes(["flat", "circular"])
            .build();

        let weak = Rc::downgrade(self);
        let prefix = prefix.to_string();
        button.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispatch(Msg::DeleteClicked(prefix.clone()));
            }
        });
        button
    }
}

fn permission_subtitle(permission: &Permission) -> &'static str {
    if permission.with_glob {
        "Glob match"
    } else {
        "Exact command"
    }
}
