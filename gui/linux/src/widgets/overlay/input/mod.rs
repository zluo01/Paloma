mod view;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use futures::channel::mpsc;
use gtk4::{
    Align, Box as GtkBox, Image, Inscription, Orientation, Overflow, Overlay, Picture, PolicyType,
    ScrolledWindow, TextChildAnchor, TextView, WrapMode, gdk, gio, glib, glib::shell_quote,
    graphene, gsk, prelude::*,
};
use log::warn;

use crate::widgets::overlay::{
    input::view::ComposerView,
    model::{LauncherMsg, Mode, Msg},
};

const SEARCH_DEBOUNCE_MS: u64 = 200;
const MAX_CONTENT_HEIGHT_PX: i32 = 171;
const THUMBNAIL_HEIGHT_PX: i32 = 22;
const PREVIEW_HEIGHT_PX: i32 = 240;
const MAX_THUMBNAIL_ASPECT: f32 = 3.0;
const MAX_IMAGE_BYTES: i64 = 20 * 1024 * 1024;
const IMAGE_MEDIA_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

#[allow(dead_code)]
struct Attachment {
    anchor: TextChildAnchor,
    media_type: String,
    data: glib::Bytes,
}

struct LoadedImage {
    media_type: String,
    data: glib::Bytes,
    texture: gdk::Texture,
}

pub(crate) struct InputView {
    view: GtkBox,
    text: TextView,
    placeholder: Inscription,
    // signal to tell if change is programmatic change or user input change
    suppress: Rc<Cell<bool>>,
    debounce: Rc<Cell<Option<glib::SourceId>>>,
    attachments: Rc<RefCell<Vec<Attachment>>>,
}

impl InputView {
    pub(super) fn new(dispatcher: mpsc::UnboundedSender<Msg>) -> Self {
        let view = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .css_classes(["paloma-query"])
            .build();

        let glyph = Image::builder()
            .icon_name("edit-find-symbolic")
            .valign(Align::Start)
            .css_classes(["paloma-query-glyph"])
            .build();
        view.append(&glyph);

        let text: TextView = glib::Object::builder::<ComposerView>()
            .property("wrap-mode", WrapMode::WordChar)
            .property("accepts-tab", false)
            .property("valign", Align::Start)
            .property("css-classes", ["paloma-entry"].as_slice())
            .build()
            .upcast();

        let scroller = ScrolledWindow::builder()
            .child(&text)
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::External)
            .kinetic_scrolling(false)
            .propagate_natural_height(true)
            .max_content_height(MAX_CONTENT_HEIGHT_PX)
            .hexpand(true)
            .valign(Align::Start)
            .build();

        let placeholder = Inscription::builder()
            .text(placeholder(Mode::Search))
            .can_target(false)
            .css_classes(["paloma-entry-placeholder"])
            .build();

        let overlay = Overlay::builder().child(&scroller).build();
        overlay.add_overlay(&placeholder);
        view.append(&overlay);

        let suppress = Rc::new(Cell::new(false));
        let debounce: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));

        let changed_placeholder = placeholder.clone();
        let changed_suppress = suppress.clone();
        let changed_debounce = debounce.clone();
        text.buffer().connect_changed(move |buffer| {
            let (start, end) = buffer.bounds();
            let empty = start == end;

            // show placeholder when no text
            changed_placeholder.set_visible(empty);

            if changed_suppress.replace(false) {
                return;
            }
            if let Some(pending) = changed_debounce.take() {
                pending.remove();
            }
            if empty {
                let _ = dispatcher.unbounded_send(Msg::Launcher(LauncherMsg::QueryChanged {
                    content: String::new(),
                }));
                return;
            }
            let buffer = buffer.clone();
            let dispatcher = dispatcher.clone();
            let slot = changed_debounce.clone();
            let id = glib::timeout_add_local_once(
                Duration::from_millis(SEARCH_DEBOUNCE_MS),
                move || {
                    slot.set(None);
                    let (start, end) = buffer.bounds();
                    let content = buffer.text(&start, &end, false).to_string();
                    let _ = dispatcher
                        .unbounded_send(Msg::Launcher(LauncherMsg::QueryChanged { content }));
                },
            );
            changed_debounce.set(Some(id));
        });

        // make the placeholder disappear when try to type with IME
        let preedit_placeholder = placeholder.clone();
        text.connect_preedit_changed(move |view, preedit| {
            preedit_placeholder.set_visible(view.buffer().char_count() == 0 && preedit.is_empty());
        });

        let attachments: Rc<RefCell<Vec<Attachment>>> = Rc::new(RefCell::new(Vec::new()));

        let paste_attachments = attachments.clone();
        text.connect_paste_clipboard(move |view| {
            let clipboard = view.clipboard();
            if !clipboard
                .formats()
                .contains_type(gdk::FileList::static_type())
            {
                return;
            }
            view.stop_signal_emission_by_name("paste-clipboard");
            let view = view.clone();
            let attachments = paste_attachments.clone();
            glib::spawn_future_local(async move {
                let files = match clipboard
                    .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
                    .await
                {
                    Ok(value) => value
                        .get::<gdk::FileList>()
                        .map(|list| list.files())
                        .unwrap_or_default(),
                    Err(err) => {
                        warn!("fail to read from clipboard. {err}");
                        Vec::new()
                    },
                };
                let buffer = view.buffer();
                let is_link = |file: &gio::File| {
                    matches!(file.uri_scheme().as_deref(), Some("http" | "https"))
                };
                if files.is_empty() || files.iter().all(is_link) {
                    buffer.paste_clipboard(&clipboard, None, view.is_editable());
                    return;
                }
                let mut pasted = Vec::new();
                for file in &files {
                    let file_path = match file.path() {
                        Some(path) => shell_quote(path),
                        None => shell_quote(file.uri().as_str()),
                    };
                    pasted.push((
                        file_path.to_string_lossy().into_owned(),
                        load_image(file).await,
                    ));
                }
                // allow Ctrl+z to revert the paste
                buffer.begin_user_action();
                buffer.delete_selection(true, view.is_editable()); // replace the highlighted text
                for (path, image) in pasted {
                    if !image.is_some_and(|image| attach_image(&view, &attachments, image)) {
                        buffer.insert_interactive_at_cursor(&path, view.is_editable());
                    }
                    buffer.insert_interactive_at_cursor(" ", view.is_editable());
                }
                buffer.end_user_action();
                view.scroll_mark_onscreen(&buffer.get_insert());
            });
        });

        Self {
            view,
            text,
            placeholder,
            suppress,
            debounce,
            attachments,
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn query(&self) -> String {
        let buffer = self.text.buffer();
        let (start, end) = buffer.bounds();
        buffer.text(&start, &end, false).trim().to_string()
    }

    pub(crate) fn focus(&self) {
        if self.text.has_focus() {
            return;
        }
        self.text.grab_focus();
        let buffer = self.text.buffer();
        buffer.place_cursor(&buffer.end_iter());
        // this make sure when paste text, cursor will move to the end
        self.text.scroll_mark_onscreen(&buffer.get_insert());
    }

    pub(crate) fn move_cursor(&self, delta: i32) -> bool {
        let buffer = self.text.buffer();
        let mut iter = buffer.iter_at_mark(&buffer.get_insert());
        if delta < 0 {
            !self.text.backward_display_line(&mut iter)
        } else {
            !self.text.forward_display_line(&mut iter)
        }
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.text.buffer().has_selection()
    }

    pub(crate) fn clear(&self) {
        self.attachments.borrow_mut().clear();
        let buffer = self.text.buffer();
        if buffer.char_count() == 0 {
            return;
        }
        if let Some(pending) = self.debounce.take() {
            pending.remove();
        }
        self.suppress.set(true);
        buffer.set_text("");
    }

    pub(crate) fn set_mode(&self, mode: Mode) {
        self.placeholder.set_text(Some(placeholder(mode)));
    }
}

async fn load_image(file: &gio::File) -> Option<LoadedImage> {
    let path = file.path()?;
    let (content_type, _) = gio::content_type_guess(Some(&path), None);
    let media_type = gio::content_type_get_mime_type(&content_type)?;
    if !IMAGE_MEDIA_TYPES.contains(&media_type.as_str()) {
        return None;
    }
    let size = match file
        .query_info_future(
            gio::FILE_ATTRIBUTE_STANDARD_SIZE,
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await
    {
        Ok(info) => info.size(),
        Err(err) => {
            warn!("fail to query image {}. {err}", path.display());
            return None;
        },
    };
    if size > MAX_IMAGE_BYTES {
        warn!(
            "image {} is {size} bytes, over the {MAX_IMAGE_BYTES} limit, fallback to file path",
            path.display()
        );
        return None;
    }
    let (data, _) = match file.load_contents_future().await {
        Ok(contents) => contents,
        Err(err) => {
            warn!("fail to read image {}. {err}", path.display());
            return None;
        },
    };
    let data = glib::Bytes::from_owned(data);
    let texture = match gdk::Texture::from_bytes(&data) {
        Ok(texture) => texture,
        Err(err) => {
            warn!("fail to decode image {}. {err}", path.display());
            return None;
        },
    };
    Some(LoadedImage {
        media_type: media_type.to_string(),
        data,
        texture,
    })
}

fn attach_image(
    view: &TextView,
    attachments: &RefCell<Vec<Attachment>>,
    image: LoadedImage,
) -> bool {
    let (Some(preview), Some(thumbnail)) = (
        thumbnail(view, &image.texture, PREVIEW_HEIGHT_PX),
        thumbnail(view, &image.texture, THUMBNAIL_HEIGHT_PX),
    ) else {
        warn!("fail to render image thumbnail, fallback to file path");
        return false;
    };
    let buffer = view.buffer();
    let mut iter = buffer.iter_at_mark(&buffer.get_insert());
    let anchor = buffer.create_child_anchor(&mut iter);
    let picture = Picture::builder()
        .paintable(&thumbnail)
        .can_shrink(false)
        .overflow(Overflow::Hidden)
        .css_classes(["paloma-attachment"])
        .has_tooltip(true)
        .build();
    let preview = Picture::for_paintable(&preview);
    picture.connect_query_tooltip(move |_, _, _, _, tooltip| {
        tooltip.set_custom(Some(&preview));
        true
    });
    view.add_child_at_anchor(&picture, &anchor);
    attachments.borrow_mut().push(Attachment {
        anchor,
        media_type: image.media_type,
        data: image.data,
    });
    true
}

/// aspect ratio scale along the provided height, center-cropped past MAX_THUMBNAIL_ASPECT
fn thumbnail(view: &TextView, texture: &gdk::Texture, height: i32) -> Option<gdk::Paintable> {
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

fn placeholder(mode: Mode) -> &'static str {
    match mode {
        Mode::Search => "Search, or ask anything…",
        Mode::Chat => "Reply…",
        Mode::Session => "Search sessions…",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_follows_overlay_mode() {
        assert_eq!(placeholder(Mode::Search), "Search, or ask anything…");
        assert_eq!(placeholder(Mode::Chat), "Reply…");
        assert_eq!(placeholder(Mode::Session), "Search sessions…");
    }
}
