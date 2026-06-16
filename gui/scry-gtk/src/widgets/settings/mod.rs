mod plugins;
mod services;

use std::{cell::RefCell, rc::Rc, sync::Arc};

use gtk4::{Align, Image, Widget, glib, subclass::prelude::*};
use libadwaita::{
    Application, ApplicationWindow, PreferencesGroup, Sidebar, SidebarItem, SidebarSection,
    prelude::*,
};
use scry_core::AppContext;

pub(crate) const CSS_PARTS: &[&str] = &[services::CSS];

mod imp {
    use gtk4::{CompositeTemplate, Stack, glib};
    use libadwaita::{NavigationPage, ToolbarView, subclass::prelude::*};

    #[derive(CompositeTemplate, Default)]
    #[template(file = "window.ui")]
    pub struct SettingsWindow {
        /// The sidebar page's toolbar; its content is set to the AdwSidebar.
        #[template_child]
        pub sidebar_host: TemplateChild<ToolbarView>,
        #[template_child]
        pub stack: TemplateChild<Stack>,
        #[template_child]
        pub content_page: TemplateChild<NavigationPage>,
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

impl SettingsWindow {
    fn new(app: &Application, state: Arc<AppContext>) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        let imp = window.imp();

        // The sidebar's sections/items don't template cleanly, so it's built
        // in code and hosted in the templated toolbar.
        let section = SidebarSection::new();
        section.append(SidebarItem::new("Services"));
        section.append(SidebarItem::new("Plugins"));
        let sidebar = Sidebar::new();
        sidebar.append(section);
        imp.sidebar_host.set_content(Some(&sidebar));

        imp.stack.add_titled(
            &services::build(state.clone(), window.clone().upcast()),
            Some("services"),
            "Services",
        );
        imp.stack.add_titled(
            &plugins::build(state, window.clone().upcast()),
            Some("plugins"),
            "Plugins",
        );

        let stack = imp.stack.get();
        let content_page = imp.content_page.get();
        sidebar.connect_selected_notify(move |sidebar| {
            let name = if sidebar.selected() == 0 {
                "Services"
            } else {
                "Plugins"
            };
            stack.set_visible_child_name(&name.to_lowercase());
            content_page.set_title(name);
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

/// An `AdwPreferencesGroup` whose rows are rebuilt on refresh. It tracks the
/// rows it holds so they can be cleared — the group has no remove-all.
#[derive(Clone)]
pub(super) struct Group {
    pub(super) widget: PreferencesGroup,
    rows: Rc<RefCell<Vec<Widget>>>,
}

impl Group {
    pub(super) fn new(title: &str) -> Self {
        Self {
            widget: PreferencesGroup::builder().title(title).build(),
            rows: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub(super) fn clear(&self) {
        for row in self.rows.borrow_mut().drain(..) {
            self.widget.remove(&row);
        }
    }

    pub(super) fn add(&self, row: impl IsA<Widget>) {
        let row = row.upcast();
        self.widget.add(&row);
        self.rows.borrow_mut().push(row);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.borrow().is_empty()
    }
}

pub(super) fn unhealthy_icon(error: Option<&str>) -> Image {
    Image::builder()
        .icon_name("dialog-information-symbolic")
        .tooltip_text(error.unwrap_or("unknown error"))
        .valign(Align::Center)
        .build()
}
