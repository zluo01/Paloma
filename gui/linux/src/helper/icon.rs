use gtk4::Image;
use libadwaita::{gdk::Texture, glib::Bytes};
use paloma_core::{Icon, capability_icon};

pub(crate) enum IconSource<'a> {
    Name(&'a str),
    Path(&'a str),
    Embedded(&'a [u8]),
}

impl<'a> From<&'a Icon> for IconSource<'a> {
    fn from(icon: &'a Icon) -> Self {
        match icon {
            Icon::Name(name) => IconSource::Name(name),
            Icon::Path(path) => IconSource::Path(path),
            Icon::Embedded(data) => IconSource::Embedded(data),
        }
    }
}

impl<'a> From<&'a capability_icon::Icon> for IconSource<'a> {
    fn from(icon: &'a capability_icon::Icon) -> Self {
        match icon {
            capability_icon::Icon::Name(name) => IconSource::Name(name),
            capability_icon::Icon::Path(path) => IconSource::Path(path),
            capability_icon::Icon::Embedded(data) => IconSource::Embedded(data),
        }
    }
}

pub(crate) fn icon_image(
    source: Option<IconSource<'_>>,
    pixel_size: i32,
    fallback: Option<&str>,
) -> Image {
    let image = match source {
        Some(IconSource::Name(name)) => Image::from_icon_name(name),
        Some(IconSource::Path(path)) => Image::from_file(path),
        Some(IconSource::Embedded(data)) => match Texture::from_bytes(&Bytes::from(data)) {
            Ok(texture) => Image::from_paintable(Some(&texture)),
            Err(e) => {
                log::warn!("failed to load embedded icon: {e}");
                fallback_image(fallback)
            },
        },
        None => fallback_image(fallback),
    };
    image.set_pixel_size(pixel_size);
    image
}

fn fallback_image(icon_name: Option<&str>) -> Image {
    icon_name.map_or_else(Image::new, Image::from_icon_name)
}
