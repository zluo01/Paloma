mod view;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use bytes::Bytes;
use futures::{channel::mpsc, future::join_all};
use gtk4::{
    Align, Box as GtkBox, Image, Inscription, Orientation, Overflow, Overlay, Picture, PolicyType,
    ScrolledWindow, TextBuffer, TextChildAnchor, TextIter, TextView, WrapMode, gdk,
    gdk::{Clipboard, Paintable},
    gio, glib,
    glib::shell_quote,
    graphene, gsk,
    prelude::*,
};
use log::warn;
use paloma_core::UserPromptAttachment;

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
// this will show when trying to Ctrl+Z a deleted image
const OBJECT_REPLACEMENT_CHARACTER: char = '\u{FFFC}';
const IMAGE_MEDIA_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

struct Attachment {
    anchor: TextChildAnchor,
    media_type: String,
    data: glib::Bytes,
}

impl Attachment {
    fn to_user_prompt(&self, id: u32) -> UserPromptAttachment {
        UserPromptAttachment::Image {
            id,
            media_type: self.media_type.clone(),
            data: Bytes::from_owner(self.data.clone()),
        }
    }
}

struct LoadedImage {
    media_type: String,
    data: glib::Bytes,
    texture: gdk::Texture,
}

struct Thumbnails {
    inline: Paintable,
    preview: Paintable,
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
        let attachments: Rc<RefCell<Vec<Attachment>>> = Rc::new(RefCell::new(Vec::new()));

        let changed_placeholder = placeholder.clone();
        let changed_suppress = suppress.clone();
        let changed_debounce = debounce.clone();
        let changed_attachments = attachments.clone();
        text.buffer().connect_changed(move |buffer| {
            let (start, end) = buffer.bounds();
            let empty = start == end;

            // show placeholder when no text
            changed_placeholder.set_visible(empty);
            // proactively cleanup any leftover on each edit.
            changed_attachments
                .borrow_mut()
                .retain(|attachment| !attachment.anchor.is_deleted());

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

        let paste_attachments = attachments.clone();
        text.connect_paste_clipboard(move |view| {
            let clipboard = view.clipboard();
            let formats = clipboard.formats();

            let formats = [
                formats.contains_type(gdk::FileList::static_type()), // files
                formats.contains_type(glib::Type::STRING),           // has text type, text/plain
                formats.contains_type(gdk::Texture::static_type()),  // displayable image type
            ];

            let has_files = match formats {
                [true, _, _] => true,          // only files
                [false, false, true] => false, // only in-memory images
                _ => return,                   // every else hand back to default handler
            };

            view.stop_signal_emission_by_name("paste-clipboard");
            let view = view.clone();
            let attachments = paste_attachments.clone();
            glib::spawn_future_local(async move {
                if has_files {
                    handle_clipboard_files(&clipboard, &view, &attachments).await
                } else {
                    handle_clipboard_images(&clipboard, &view, &attachments).await
                }
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

    pub(crate) fn query(&self) -> (String, Vec<UserPromptAttachment>) {
        let buffer = self.text.buffer();
        let attachments = self.attachments.borrow();
        let mut anchored: Vec<(TextIter, &Attachment)> = attachments
            .iter()
            .filter(|attachment| !attachment.anchor.is_deleted())
            .map(|attachment| (buffer.iter_at_child_anchor(&attachment.anchor), attachment))
            .collect();
        anchored.sort_by_key(|(anchor, _)| *anchor);

        let mut text = String::new();
        let mut images = Vec::with_capacity(anchored.len());
        let mut cursor = buffer.start_iter();
        for (anchor, attachment) in anchored {
            text.push_str(&buffer.text(&cursor, &anchor, false));
            let id = images.len() as u32 + 1;
            text.push_str(&format!("[Image #{id}]"));
            images.push(attachment.to_user_prompt(id));
            cursor = anchor;
            cursor.forward_char();
        }
        text.push_str(&buffer.text(&cursor, &buffer.end_iter(), false));
        text.retain(|c| c != OBJECT_REPLACEMENT_CHARACTER);
        (text.trim().to_string(), images)
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

    /// programmatically move the cursor for multiline text input
    /// true if moved, false if it is either the first or last line
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

/// in-memory images such as screenshot or copy from web
async fn handle_clipboard_images(
    clipboard: &Clipboard,
    view: &TextView,
    attachments: &RefCell<Vec<Attachment>>,
) {
    let loaded = read_in_memory_image(clipboard)
        .await
        .and_then(|image| prepare_thumbnails(view, &image).map(|thumbnails| (image, thumbnails)));
    let (image, thumbnails) = match loaded {
        Ok(loaded) => loaded,
        Err(err) => {
            warn!("fail to paste image from clipboard. {err}");
            return;
        },
    };
    write_clipboard_to_buffer(view, |buffer| {
        attach_image(view, attachments, image, thumbnails);
        buffer.insert_interactive_at_cursor(" ", view.is_editable());
    });
}

async fn handle_clipboard_files(
    clipboard: &Clipboard,
    view: &TextView,
    attachments: &RefCell<Vec<Attachment>>,
) {
    let buffer = view.buffer();
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
    let is_link = |file: &gio::File| matches!(file.uri_scheme().as_deref(), Some("http" | "https"));
    if files.is_empty() || files.iter().all(is_link) {
        buffer.paste_clipboard(clipboard, None, view.is_editable());
        return;
    }

    let images = join_all(files.iter().map(load_image)).await;
    let pasted: Vec<_> = files
        .iter()
        .zip(images)
        .map(|(file, image)| {
            let file_path = match file.path() {
                Some(path) => shell_quote(path),
                None => shell_quote(file.uri().as_str()),
            };
            let file_path = file_path.to_string_lossy().into_owned();
            let image = match image {
                Ok(Some(image)) => {
                    prepare_thumbnails(view, &image).map(|thumbnails| (image, thumbnails))
                },
                Ok(None) => return (file_path, None),
                Err(err) => Err(err),
            };
            let image = image
                .inspect_err(|err| {
                    warn!("fail to paste image {file_path}, fallback to file path. {err}")
                })
                .ok();
            (file_path, image)
        })
        .collect();

    write_clipboard_to_buffer(view, |buffer| {
        for (path, image) in pasted {
            match image {
                Some((image, thumbnails)) => attach_image(view, attachments, image, thumbnails),
                None => {
                    buffer.insert_interactive_at_cursor(&path, view.is_editable());
                },
            }
            buffer.insert_interactive_at_cursor(" ", view.is_editable());
        }
    });
}

fn write_clipboard_to_buffer(view: &TextView, writer: impl FnOnce(&TextBuffer)) {
    let buffer = view.buffer();

    // allow Ctrl+Z to revert the paste
    buffer.begin_user_action();
    buffer.delete_selection(true, view.is_editable()); // replace the highlighted text
    writer(&buffer);
    buffer.end_user_action();
    view.scroll_mark_onscreen(&buffer.get_insert());
}

async fn read_in_memory_image(clipboard: &Clipboard) -> Result<LoadedImage, InputViewError> {
    let (stream, media_type) = clipboard
        .read_future(IMAGE_MEDIA_TYPES, glib::Priority::DEFAULT)
        .await
        .map_err(InputViewError::Read)?;
    let output_stream = gio::MemoryOutputStream::new_resizable();
    output_stream
        .splice_future(
            &stream,
            gio::OutputStreamSpliceFlags::CLOSE_SOURCE | gio::OutputStreamSpliceFlags::CLOSE_TARGET,
            glib::Priority::DEFAULT,
        )
        .await
        .map_err(InputViewError::Read)?;
    let data = output_stream.steal_as_bytes();
    if data.len() as i64 > MAX_IMAGE_BYTES {
        return Err(InputViewError::ImageTooLarge(data.len() as i64));
    }
    decode_image(&media_type, data).await
}

async fn load_image(file: &gio::File) -> Result<Option<LoadedImage>, InputViewError> {
    let Some(path) = file.path() else {
        return Ok(None);
    };
    let (content_type, _) = gio::content_type_guess(Some(&path), None);
    let Some(media_type) = gio::content_type_get_mime_type(&content_type) else {
        return Ok(None);
    };
    if !IMAGE_MEDIA_TYPES.contains(&media_type.as_str()) {
        return Ok(None);
    }
    let size = file
        .query_info_future(
            gio::FILE_ATTRIBUTE_STANDARD_SIZE,
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await
        .map_err(InputViewError::Read)?
        .size();
    if size > MAX_IMAGE_BYTES {
        return Err(InputViewError::ImageTooLarge(size));
    }
    let (data, _) = file
        .load_contents_future()
        .await
        .map_err(InputViewError::Read)?;
    decode_image(&media_type, glib::Bytes::from_owned(data))
        .await
        .map(Some)
}

async fn decode_image(media_type: &str, data: glib::Bytes) -> Result<LoadedImage, InputViewError> {
    let decoded =
        gio::spawn_blocking(move || gdk::Texture::from_bytes(&data).map(|texture| (data, texture)))
            .await;
    match decoded {
        Ok(Ok((data, texture))) => Ok(LoadedImage {
            media_type: media_type.to_string(),
            data,
            texture,
        }),
        Ok(Err(err)) => Err(InputViewError::Decode(err)),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<&str>()
                .map(|reason| reason.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".to_string());
            Err(InputViewError::Panic(reason))
        },
    }
}

fn prepare_thumbnails(view: &TextView, image: &LoadedImage) -> Result<Thumbnails, InputViewError> {
    let (Some(preview), Some(inline)) = (
        thumbnail(view, &image.texture, PREVIEW_HEIGHT_PX),
        thumbnail(view, &image.texture, THUMBNAIL_HEIGHT_PX),
    ) else {
        return Err(InputViewError::Render);
    };
    Ok(Thumbnails { inline, preview })
}

fn attach_image(
    view: &TextView,
    attachments: &RefCell<Vec<Attachment>>,
    image: LoadedImage,
    thumbnails: Thumbnails,
) {
    let buffer = view.buffer();
    let mut iter = buffer.iter_at_mark(&buffer.get_insert());
    let anchor = buffer.create_child_anchor(&mut iter);
    let picture = Picture::builder()
        .paintable(&thumbnails.inline)
        .can_shrink(false)
        .overflow(Overflow::Hidden)
        .css_classes(["paloma-attachment"])
        .has_tooltip(true)
        .build();
    let preview = Picture::for_paintable(&thumbnails.preview);
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
}

/// aspect ratio scale along the provided height, center-cropped past MAX_THUMBNAIL_ASPECT
fn thumbnail(view: &TextView, texture: &gdk::Texture, height: i32) -> Option<Paintable> {
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

#[derive(Debug, thiserror::Error)]
enum InputViewError {
    #[error("{0} bytes, over the {MAX_IMAGE_BYTES} limit")]
    ImageTooLarge(i64),
    #[error("read failed: {0}")]
    Read(glib::Error),
    #[error("decode failed: {0}")]
    Decode(glib::Error),
    #[error("decoder panicked: {0}")]
    Panic(String),
    #[error("no renderer to draw the thumbnail")]
    Render,
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
