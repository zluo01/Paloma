use std::{
    cell::{Cell, RefCell},
    future::Future,
};

use gtk4::{
    IconLookupFlags, IconTheme, TextDirection, Widget,
    gdk::{self, Paintable},
    glib,
    prelude::*,
    subclass::prelude::*,
};
use libadwaita::SpinnerPaintable;
use log::warn;

use crate::helper::thumbnail;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LazyImage {
        pub(super) height: Cell<i32>,
        pub(super) spinner: RefCell<Option<SpinnerPaintable>>,
        pub(super) image: RefCell<Option<Paintable>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LazyImage {
        const NAME: &'static str = "PalomaLazyImage";
        type Type = super::LazyImage;
        type Interfaces = (Paintable,);
    }

    impl ObjectImpl for LazyImage {}

    impl PaintableImpl for LazyImage {
        fn intrinsic_width(&self) -> i32 {
            self.image
                .borrow()
                .as_ref()
                // we use height here so the initial placeholder render as a square
                .map_or(self.height.get(), |image| image.intrinsic_width())
        }

        fn intrinsic_height(&self) -> i32 {
            self.image
                .borrow()
                .as_ref()
                .map_or(self.height.get(), |image| image.intrinsic_height())
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            if let Some(image) = self.image.borrow().as_ref() {
                image.snapshot(snapshot, width, height);
            } else if let Some(spinner) = self.spinner.borrow().as_ref() {
                spinner.snapshot(snapshot, width, height);
            }
        }
    }
}

glib::wrapper! {
    pub struct LazyImage(ObjectSubclass<imp::LazyImage>) @implements Paintable;
}

impl LazyImage {
    pub(crate) fn new<F>(widget: &impl IsA<Widget>, height: i32, texture: F) -> Self
    where
        F: Future<Output = Option<gdk::Texture>> + 'static,
    {
        let lazy: Self = glib::Object::new();
        lazy.imp().height.set(height);
        let spinner = SpinnerPaintable::new(Some(widget));
        let forward = lazy.downgrade();
        spinner.connect_invalidate_contents(move |_| {
            if let Some(lazy) = forward.upgrade() {
                lazy.invalidate_contents();
            }
        });
        lazy.imp().spinner.replace(Some(spinner));

        let widget = widget.as_ref().downgrade();
        let weak = lazy.downgrade();
        glib::spawn_future_local(async move {
            let texture = texture.await;
            let (Some(widget), Some(lazy)) = (widget.upgrade(), weak.upgrade()) else {
                return;
            };
            let rendered = texture.and_then(|texture| {
                thumbnail(&widget, &texture, height.min(texture.height()))
                    .ok_or_else(|| warn!("no renderer to draw the image thumbnail"))
                    .ok()
            });
            let paintable = rendered.unwrap_or_else(|| {
                IconTheme::for_display(&widget.display())
                    .lookup_icon(
                        "image-missing-symbolic",
                        &[],
                        height,
                        widget.scale_factor(),
                        TextDirection::None,
                        IconLookupFlags::empty(),
                    )
                    .upcast()
            });
            lazy.set_image(paintable);
        });
        lazy
    }

    fn set_image(&self, image: Paintable) {
        self.imp().image.replace(Some(image));
        self.imp().spinner.take();
        self.invalidate_size();
        self.invalidate_contents();
    }
}
