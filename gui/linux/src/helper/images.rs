use gtk4::{
    ContentFit, Overflow, Picture, TextChildAnchor, TextView, Widget, gdk, gdk::Paintable, gio,
    glib, graphene, gsk, prelude::*,
};
use log::warn;

const MAX_THUMBNAIL_ASPECT: f32 = 3.0;

pub(crate) const IMAGE_PLACEHOLDER_PREFIX: &str = "[Image #";

pub(crate) struct LoadedImage {
    pub(crate) media_type: String,
    pub(crate) data: glib::Bytes,
    pub(crate) texture: gdk::Texture,
}

pub(crate) fn image_placeholder(id: u32) -> String {
    format!("{IMAGE_PLACEHOLDER_PREFIX}{id}]")
}

pub(crate) async fn decode_texture(data: glib::Bytes) -> Option<gdk::Texture> {
    match decode(data).await {
        Ok((_, texture)) => Some(texture),
        Err(err) => {
            warn!("fail to decode image. {err}");
            None
        },
    }
}

pub(crate) async fn decode_image(
    media_type: &str,
    data: glib::Bytes,
) -> Result<LoadedImage, ImageHelperError> {
    let (data, texture) = decode(data).await?;
    Ok(LoadedImage {
        media_type: media_type.to_string(),
        data,
        texture,
    })
}

async fn decode(data: glib::Bytes) -> Result<(glib::Bytes, gdk::Texture), ImageHelperError> {
    let decoded =
        gio::spawn_blocking(move || gdk::Texture::from_bytes(&data).map(|texture| (data, texture)))
            .await;
    match decoded {
        Ok(Ok(decoded)) => Ok(decoded),
        Ok(Err(err)) => Err(ImageHelperError::Decode(err)),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_owned());
            Err(ImageHelperError::Panic(reason))
        },
    }
}

pub(crate) fn attach_image(
    view: &TextView,
    inline: &Paintable,
    preview: Option<&Paintable>,
) -> TextChildAnchor {
    let buffer = view.buffer();
    let mut iter = buffer.iter_at_mark(&buffer.get_insert());
    let anchor = buffer.create_child_anchor(&mut iter);
    view.add_child_at_anchor(&image_picture(inline, preview), &anchor);
    anchor
}

pub(crate) fn image_picture(inline: &impl IsA<Paintable>, preview: Option<&Paintable>) -> Picture {
    let picture = Picture::builder()
        .paintable(inline)
        .can_shrink(false)
        .content_fit(ContentFit::ScaleDown)
        .overflow(Overflow::Hidden)
        .css_classes(["paloma-attachment"])
        .has_tooltip(preview.is_some())
        .build();
    if let Some(preview) = preview {
        let preview = Picture::for_paintable(preview);
        picture.connect_query_tooltip(move |_, _, _, _, tooltip| {
            tooltip.set_custom(Some(&preview));
            true
        });
    }
    picture
}

/// aspect ratio scale along the provided height, center-cropped past MAX_THUMBNAIL_ASPECT
pub(crate) fn thumbnail(
    view: &impl IsA<Widget>,
    texture: &gdk::Texture,
    height: i32,
) -> Option<Paintable> {
    let height = height as f32;
    let (texture_width, texture_height) = (texture.width() as f32, texture.height() as f32);
    let aspect =
        (texture_width / texture_height).clamp(1.0 / MAX_THUMBNAIL_ASPECT, MAX_THUMBNAIL_ASPECT);
    let width = (height * aspect).round().max(1.0);
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    let cover = (width / texture_width).max(height / texture_height);
    let (covered_width, covered_height) = (texture_width * cover, texture_height * cover);
    let device_scale = view.scale_factor() as f32;
    let snapshot = gtk4::Snapshot::new();
    snapshot.scale(device_scale, device_scale);
    snapshot.push_clip(&bounds);
    snapshot.append_scaled_texture(
        texture,
        gsk::ScalingFilter::Trilinear,
        &graphene::Rect::new(
            (width - covered_width) / 2.0,
            (height - covered_height) / 2.0,
            covered_width,
            covered_height,
        ),
    );
    snapshot.pop();
    let node = snapshot.to_node()?;
    let renderer = view.native()?.renderer()?;
    let pixels = renderer.render_texture(
        &node,
        Some(&graphene::Rect::new(
            0.0,
            0.0,
            width * device_scale,
            height * device_scale,
        )),
    );
    let snapshot = gtk4::Snapshot::new();
    snapshot.append_texture(&pixels, &bounds);
    snapshot.to_paintable(Some(&graphene::Size::new(width, height)))
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ImageHelperError {
    #[error("decode failed: {0}")]
    Decode(glib::Error),
    #[error("decoder panicked: {0}")]
    Panic(String),
}
