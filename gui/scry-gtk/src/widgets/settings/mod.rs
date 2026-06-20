mod plugins;
mod services;
mod shortcuts;

use std::sync::Arc;

use gtk4::{Align, Image, glib, subclass::prelude::*};
use libadwaita::{
    Application, ApplicationWindow, PreferencesGroup, Sidebar, SidebarItem, SidebarSection,
    prelude::*,
};
use scry_core::AppContext;

pub(crate) const CSS_PARTS: &[&str] = &[services::CSS];

mod imp {
    use std::{cell::OnceCell, rc::Rc};

    use gtk4::{CompositeTemplate, Stack, glib};
    use libadwaita::{NavigationPage, ToolbarView, subclass::prelude::*};

    use super::{plugins::PluginsPage, services::ServicesPage};

    #[derive(CompositeTemplate, Default)]
    #[template(file = "window.ui")]
    pub struct SettingsWindow {
        /// Hosts the dynamic AdwSidebar.
        #[template_child]
        pub sidebar_host: TemplateChild<ToolbarView>,
        #[template_child]
        pub stack: TemplateChild<Stack>,
        #[template_child]
        pub content_page: TemplateChild<NavigationPage>,
        /// Plain page controllers retained for callbacks and state.
        pub(super) services: OnceCell<Rc<ServicesPage>>,
        pub(super) plugins: OnceCell<Rc<PluginsPage>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SettingsWindow {
        const NAME: &'static str = "ScrySettingsWindow";
        type Type = super::SettingsWindow;
        type ParentType = libadwaita::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SettingsWindow {}
    impl WidgetImpl for SettingsWindow {}
    impl WindowImpl for SettingsWindow {}
    impl ApplicationWindowImpl for SettingsWindow {}
    impl AdwApplicationWindowImpl for SettingsWindow {}
}

glib::wrapper! {
    pub struct SettingsWindow(ObjectSubclass<imp::SettingsWindow>)
        @extends libadwaita::ApplicationWindow, gtk4::ApplicationWindow, gtk4::Window, gtk4::Widget,
        @implements gtk4::gio::ActionGroup, gtk4::gio::ActionMap, gtk4::Accessible,
            gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Native, gtk4::Root,
            gtk4::ShortcutManager;
}

/// One settings page: stable stack id plus visible title.
#[derive(Clone, Copy)]
struct PageSpec {
    id: &'static str,
    title: &'static str,
}

// Sidebar order.
const SERVICES_PAGE: PageSpec = PageSpec {
    id: "services",
    title: "Services",
};
const PLUGINS_PAGE: PageSpec = PageSpec {
    id: "plugins",
    title: "Plugins",
};
const SHORTCUTS_PAGE: PageSpec = PageSpec {
    id: "shortcuts",
    title: "Shortcuts",
};
const PAGES: &[PageSpec] = &[SERVICES_PAGE, PLUGINS_PAGE, SHORTCUTS_PAGE];

impl SettingsWindow {
    fn new(app: &Application, state: Arc<AppContext>) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        let imp = window.imp();

        // AdwSidebar sections/items are dynamic, so build them in code.
        let section = SidebarSection::new();
        for page in PAGES {
            section.append(SidebarItem::new(page.title));
        }
        let sidebar = Sidebar::new();
        sidebar.append(section);
        imp.sidebar_host.set_content(Some(&sidebar));

        // Services and Plugins need retained controllers; Shortcuts is widget-only.
        let parent: ApplicationWindow = window.clone().upcast();
        let services = services::ServicesPage::new(state.clone(), &parent);
        let plugins = plugins::PluginsPage::new(state, &parent);
        imp.stack.add_titled(
            services.widget(),
            Some(SERVICES_PAGE.id),
            SERVICES_PAGE.title,
        );
        imp.stack
            .add_titled(plugins.widget(), Some(PLUGINS_PAGE.id), PLUGINS_PAGE.title);
        imp.stack.add_titled(
            &shortcuts::build(),
            Some(SHORTCUTS_PAGE.id),
            SHORTCUTS_PAGE.title,
        );
        // The stack owns widgets; these cells own the plain Rust controllers.
        imp.services
            .set(services)
            .unwrap_or_else(|_| panic!("services page set once in SettingsWindow::new"));
        imp.plugins
            .set(plugins)
            .unwrap_or_else(|_| panic!("plugins page set once in SettingsWindow::new"));

        // Select the first page before connecting the notify handler.
        if let Some(first) = PAGES.first() {
            sidebar.set_selected(0);
            imp.stack.set_visible_child_name(first.id);
            imp.content_page.set_title(first.title);
        }

        let stack = imp.stack.get();
        let content_page = imp.content_page.get();
        sidebar.connect_selected_notify(move |sidebar| {
            if let Some(page) = PAGES.get(sidebar.selected() as usize) {
                stack.set_visible_child_name(page.id);
                content_page.set_title(page.title);
            }
        });

        window
    }
}

/// Build and present the settings window.
pub(crate) fn open(app: &Application, state: Arc<AppContext>) -> ApplicationWindow {
    let window = SettingsWindow::new(app, state);
    window.present();
    window.upcast()
}

pub(super) fn clear_group(group: &PreferencesGroup) {
    while let Some(row) = group.row(0) {
        group.remove(&row);
    }
}

pub(super) fn group_is_empty(group: &PreferencesGroup) -> bool {
    group.row(0).is_none()
}

pub(super) fn unhealthy_icon(error: Option<&str>) -> Image {
    Image::builder()
        .icon_name("help-about-symbolic")
        .tooltip_text(error.unwrap_or("unknown error"))
        .css_classes(["scry-unhealthy-icon"])
        .valign(Align::Center)
        .build()
}
