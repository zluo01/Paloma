use std::collections::HashMap;

use futures::FutureExt;
use gtk4::{
    Box as GtkBox, TextBuffer, TextView, WrapMode, glib,
    prelude::{BoxExt, TextBufferExt, WidgetExt},
};
use libadwaita::WrapBox;
use paloma_core::UserPromptAttachment;

use crate::{
    helper::{IMAGE_PLACEHOLDER_PREFIX, decode_texture, image_picture, image_placeholder},
    widgets::overlay::results::chat::{helper::new_section, sections::lazy_image::LazyImage},
};

const USER_TITLE: &str = "You";
const USER_CLASS: &str = "paloma-chat-section-user";
const INLINE_IMAGE_HEIGHT_PX: i32 = 16;
const DISPLAY_IMAGE_HEIGHT_PX: i32 = 120;

pub(crate) struct UserPromptSection {
    view: GtkBox,
}

impl UserPromptSection {
    pub(crate) fn new(text: &str, attachments: Vec<UserPromptAttachment>) -> Self {
        let view = new_section(Some(USER_TITLE), USER_CLASS);

        let buffer = TextBuffer::new(None);
        let text_view = TextView::builder()
            .buffer(&buffer)
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(WrapMode::WordChar)
            .hexpand(true)
            .focusable(false)
            .can_focus(false)
            .left_margin(0)
            .right_margin(0)
            .top_margin(0)
            .bottom_margin(0)
            .css_classes(["paloma-chat-text", USER_CLASS])
            .build();
        view.append(&text_view);

        let image_row = WrapBox::builder()
            .child_spacing(6)
            .line_spacing(6)
            .margin_top(6)
            .visible(false)
            .build();
        view.append(&image_row);

        let decodes: HashMap<u32, _> = attachments
            .into_iter()
            .map(|UserPromptAttachment::Image { id, data, .. }| {
                (id, decode_texture(glib::Bytes::from_owned(data)).shared())
            })
            .collect();

        for segment in split_to_segments(text) {
            match segment {
                Segment::Text(run) => buffer.insert(&mut buffer.end_iter(), run),
                Segment::Image(id) => match decodes.get(&id) {
                    Some(decode) => {
                        let inline =
                            LazyImage::new(&text_view, INLINE_IMAGE_HEIGHT_PX, decode.clone());
                        buffer.insert_paintable(&mut buffer.end_iter(), &inline);
                        let display =
                            LazyImage::new(&text_view, DISPLAY_IMAGE_HEIGHT_PX, decode.clone());
                        image_row.append(&image_picture(&display, None));
                        image_row.set_visible(true);
                    },
                    None => buffer.insert(&mut buffer.end_iter(), &image_placeholder(id)),
                },
            }
        }

        Self { view }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }
}

enum Segment<'a> {
    Text(&'a str),
    Image(u32),
}

fn split_to_segments(text: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(IMAGE_PLACEHOLDER_PREFIX) {
        let after = &rest[start + IMAGE_PLACEHOLDER_PREFIX.len()..];
        let Some(end) = after.find(']') else {
            break;
        };
        match after[..end].parse::<u32>() {
            Ok(id) => {
                if start > 0 {
                    segments.push(Segment::Text(&rest[..start]));
                }
                segments.push(Segment::Image(id));
                rest = &after[end + 1..];
            },
            Err(_) => {
                let consumed = start + IMAGE_PLACEHOLDER_PREFIX.len();
                segments.push(Segment::Text(&rest[..consumed]));
                rest = &rest[consumed..];
            },
        }
    }
    if !rest.is_empty() {
        segments.push(Segment::Text(rest));
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str) -> Vec<String> {
        split_to_segments(text)
            .into_iter()
            .map(|segment| match segment {
                Segment::Text(run) => format!("t:{run}"),
                Segment::Image(id) => format!("i:{id}"),
            })
            .collect()
    }

    #[test]
    fn splits_placeholders_in_order() {
        assert_eq!(
            render("compare [Image #1] with [Image #2], ok?"),
            ["t:compare ", "i:1", "t: with ", "i:2", "t:, ok?"]
        );
    }

    #[test]
    fn placeholder_at_edges() {
        assert_eq!(render("[Image #1]"), ["i:1"]);
        assert_eq!(render("[Image #1] hi [Image #2]"), ["i:1", "t: hi ", "i:2"]);
    }

    #[test]
    fn malformed_placeholders_stay_text() {
        assert_eq!(
            render("see [Image #x] and [Image #"),
            ["t:see [Image #", "t:x] and [Image #"]
        );
        assert_eq!(render("plain text"), ["t:plain text"]);
    }
}
