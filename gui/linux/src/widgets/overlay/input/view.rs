use gtk4::{glib, prelude::*, subclass::prelude::*};

/// old bug from GTK: https://gitlab.gnome.org/GNOME/gtk/-/work_items/5063
/// view adjustment is not updated properly during typing cause shift instead of proper expand
mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ComposerView;

    #[glib::object_subclass]
    impl ObjectSubclass for ComposerView {
        const NAME: &'static str = "PalomaComposerView";
        type Type = super::ComposerView;
        type ParentType = gtk4::TextView;
    }

    impl ObjectImpl for ComposerView {}

    impl WidgetImpl for ComposerView {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);

            let view = self.obj();

            // when wrap within ScrolledWindow, this will be the scroller from ScrolledWindow
            // otherwise, TextView internal scroller
            let Some(adjustment) = view.vadjustment() else {
                return;
            };
            let page = f64::from(view.visible_rect().height());
            // update when there is something
            // and current content height is different from the scroller content height
            if page > 0.0 && adjustment.page_size() != page && page <= adjustment.upper() {
                adjustment.set_page_size(page);
                adjustment.set_value(adjustment.value()); // internally do the clamp
            }
        }
    }

    impl TextViewImpl for ComposerView {}
}

glib::wrapper! {
    pub struct ComposerView(ObjectSubclass<imp::ComposerView>)
        @extends gtk4::TextView, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Scrollable;
}
