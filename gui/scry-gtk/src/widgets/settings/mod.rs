mod helper;
mod permissions;
mod plugins;
mod services;
mod shortcuts;

use std::sync::Arc;

use gtk4::{Stack, StackTransitionType, glib};
use libadwaita::{
    Application, ApplicationWindow, HeaderBar, NavigationPage, NavigationSplitView, Sidebar,
    SidebarItem, SidebarSection, ToolbarView, prelude::*,
};
use scry_core::AppContext;

use crate::widgets::settings::{
    permissions::PermissionsPage, plugins::PluginsPage, services::ServicesPage,
};

pub(crate) const CSS_PARTS: &[&str] = &[services::CSS];

#[derive(Clone, Copy)]
enum Page {
    Services,
    Plugins,
    Permissions,
    Shortcuts,
}

impl Page {
    const ALL: &[Self] = &[
        Self::Services,
        Self::Plugins,
        Self::Permissions,
        Self::Shortcuts,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Services => "Services",
            Self::Plugins => "Plugins",
            Self::Permissions => "Permissions",
            Self::Shortcuts => "Shortcuts",
        }
    }
}

pub(crate) struct SettingsWindow {
    window: ApplicationWindow,
}

impl SettingsWindow {
    pub(crate) fn new(app: &Application, app_context: Arc<AppContext>) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Settings")
            .default_width(820)
            .default_height(560)
            .build();

        // sidebar
        let sidebar_host = ToolbarView::new();
        sidebar_host.add_top_bar(&HeaderBar::builder().show_title(false).build());
        let sidebar_page = NavigationPage::builder()
            .title("Settings")
            .child(&sidebar_host)
            .build();

        let section = SidebarSection::new();
        for page in Page::ALL {
            section.append(SidebarItem::new(page.title()));
        }
        let sidebar = Sidebar::new();
        sidebar.append(section);
        sidebar_host.set_content(Some(&sidebar));

        let stack = Stack::builder()
            .transition_type(StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();
        let content_toolbar = ToolbarView::new();
        content_toolbar.add_top_bar(&HeaderBar::new());
        content_toolbar.set_content(Some(&stack));
        let content_page = NavigationPage::builder()
            .title(Page::ALL[0].title())
            .child(&content_toolbar)
            .build();

        let parent: ApplicationWindow = window.clone().upcast();
        let services = ServicesPage::new(app_context.clone(), &parent);
        let plugins = PluginsPage::new(app_context.clone(), &parent);
        let permissions = PermissionsPage::new(app_context, &parent);
        stack.add_titled(
            services.widget(),
            Some(Page::Services.title()),
            Page::Services.title(),
        );
        stack.add_titled(
            plugins.widget(),
            Some(Page::Plugins.title()),
            Page::Plugins.title(),
        );
        stack.add_titled(
            permissions.widget(),
            Some(Page::Permissions.title()),
            Page::Permissions.title(),
        );
        stack.add_titled(
            &shortcuts::build(),
            Some(Page::Shortcuts.title()),
            Page::Shortcuts.title(),
        );

        if let Some(first) = Page::ALL.first() {
            sidebar.set_selected(0);
            stack.set_visible_child_name(first.title());
            content_page.set_title(first.title());
        }

        let content_page_cb = content_page.clone();
        sidebar.connect_selected_notify(move |sidebar| {
            if let Some(page) = Page::ALL.get(sidebar.selected() as usize) {
                stack.set_visible_child_name(page.title());
                content_page_cb.set_title(page.title());
                match page {
                    Page::Services => services.refresh(),
                    Page::Plugins => plugins.refresh(),
                    Page::Permissions => permissions.refresh(),
                    _ => {},
                }
            }
        });

        let split_view = NavigationSplitView::builder()
            .max_sidebar_width(200.0)
            .sidebar(&sidebar_page)
            .content(&content_page)
            .build();

        window.set_content(Some(&split_view));

        let win = window.clone();
        window.connect_close_request(move |_| {
            win.set_visible(false);
            glib::Propagation::Stop
        });

        Self { window }
    }

    pub(crate) fn present(&self) {
        self.window.present();
    }
}
