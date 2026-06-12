// Settings window, opened from the tray. Closing it only dismisses the
// window; the rest of the process keeps running.

mod connect_modal;
mod plugin_modal;
mod plugins_tab;
mod services_tab;

use std::sync::Arc;

use adw::{
    prelude::*, ApplicationWindow as AdwApplicationWindow, NavigationPage, NavigationSplitView,
    Sidebar, SidebarItem, SidebarSection, ToolbarView,
};
use gtk4::{Application, ApplicationWindow, PolicyType, ScrolledWindow, Stack};
use libadwaita as adw;
use scry_core::AppContext;

/// CSS fragments contributed by the settings tabs and modals.
/// Aggregated into the global stylesheet by `crate::style::load`.
pub(crate) const CSS_PARTS: &[&str] = &[services_tab::CSS, connect_modal::CSS];

/// Build and present the settings window. Returns the window so the caller
/// can re-present it instead of stacking duplicates.
pub fn open(app: &Application, state: Arc<AppContext>) -> ApplicationWindow {
    // AdwApplicationWindow draws no titlebar of its own; the split view's
    // header bars carry the window controls instead.
    let window = AdwApplicationWindow::builder()
        .application(app)
        .default_width(820)
        .default_height(560)
        .title("Settings")
        .build();

    let stack = Stack::builder()
        .transition_type(gtk4::StackTransitionType::Crossfade)
        .transition_duration(150)
        .build();
    stack.add_titled(
        &services_tab::build(state, window.clone().upcast()),
        Some("services"),
        "Services",
    );
    stack.add_titled(
        &plugins_tab::build(window.clone().upcast()),
        Some("plugins"),
        "Plugins",
    );

    let section = SidebarSection::new();
    section.append(SidebarItem::new("Services"));
    section.append(SidebarItem::new("Plugins"));
    let sidebar = Sidebar::new();
    sidebar.append(section);

    // Window controls only; the content header carries the page title.
    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_show_title(false);

    let sidebar_view = ToolbarView::new();
    sidebar_view.add_top_bar(&sidebar_header);
    sidebar_view.set_content(Some(&sidebar));

    let content_view = ToolbarView::new();
    content_view.add_top_bar(&adw::HeaderBar::new());
    content_view.set_content(Some(
        &ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .child(&stack)
            .build(),
    ));
    let content_page = NavigationPage::new(&content_view, "Services");

    // Sidebar selection drives both the visible tab and the content title.
    {
        let stack = stack.clone();
        let content_page = content_page.clone();
        sidebar.connect_selected_notify(move |sidebar| {
            let name = if sidebar.selected() == 0 {
                "Services"
            } else {
                "Plugins"
            };
            stack.set_visible_child_name(&name.to_lowercase());
            content_page.set_title(name);
        });
    }

    let split = NavigationSplitView::builder()
        .sidebar(&NavigationPage::new(&sidebar_view, "Settings"))
        .content(&content_page)
        .max_sidebar_width(200.0)
        .build();

    window.set_content(Some(&split));
    window.present();
    window.upcast()
}
