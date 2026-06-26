use gtk4::{
    Viewport,
    prelude::{BoxExt, CastNone, IsA, StaticType, WidgetExt},
};
use libadwaita::{PreferencesGroup, PreferencesPage, prelude::*};

pub(crate) fn scroll_into_view(widget: &impl IsA<gtk4::Widget>) {
    if let Some(viewport) = widget
        .ancestor(Viewport::static_type())
        .and_downcast::<Viewport>()
    {
        viewport.scroll_to(widget, None);
    }
}

pub(crate) trait Clear {
    type Child: IsA<gtk4::Widget>;

    fn first(&self) -> Option<Self::Child>;
    fn remove_child(&self, child: &Self::Child);

    fn clear(&self) {
        while let Some(child) = self.first() {
            self.remove_child(&child);
        }
    }
}

impl Clear for gtk4::Box {
    type Child = gtk4::Widget;

    fn first(&self) -> Option<gtk4::Widget> {
        self.first_child()
    }
    fn remove_child(&self, child: &gtk4::Widget) {
        self.remove(child);
    }
}

impl Clear for PreferencesGroup {
    type Child = gtk4::Widget;

    fn first(&self) -> Option<gtk4::Widget> {
        self.row(0)
    }
    fn remove_child(&self, child: &gtk4::Widget) {
        self.remove(child);
    }
}

impl Clear for PreferencesPage {
    type Child = PreferencesGroup;

    fn first(&self) -> Option<PreferencesGroup> {
        self.group(0)
    }
    fn remove_child(&self, child: &PreferencesGroup) {
        self.remove(child);
    }
}
