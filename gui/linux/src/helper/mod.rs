use gtk4::{
    ScrolledWindow, Viewport,
    prelude::{AdjustmentExt, BoxExt, CastNone, IsA, StaticType, WidgetExt},
};
use libadwaita::{PreferencesGroup, PreferencesPage, prelude::*};

/// The first/last rows snap the card fully to the top/bottom so its padding
/// isn't clipped; middle rows use the minimal scroll.
pub(crate) fn scroll_selection_into_view(
    widget: &impl IsA<gtk4::Widget>,
    index: usize,
    size: usize,
) {
    let Some(scroller) = widget
        .ancestor(ScrolledWindow::static_type())
        .and_downcast::<ScrolledWindow>()
    else {
        return;
    };
    let adj = scroller.vadjustment();
    match index {
        0 => adj.set_value(0.0),
        i if i + 1 == size => adj.set_value((adj.upper() - adj.page_size()).max(0.0)),
        _ => scroll_into_view(widget),
    }
}

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

impl Clear for gtk4::ListBox {
    type Child = gtk4::ListBoxRow;

    fn first(&self) -> Option<gtk4::ListBoxRow> {
        self.row_at_index(0)
    }
    fn remove_child(&self, child: &gtk4::ListBoxRow) {
        self.remove(child);
    }
}
