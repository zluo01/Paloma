use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use gtk4::{
    Box as GtkBox, Button, Label, Orientation,
    prelude::{BoxExt, ButtonExt, WidgetExt},
};
use scry_core::ProviderBackendId;

use crate::widgets::overlay::results::chat::{
    helper::new_section,
    markdown::{MarkdownView, ParsedMarkdown},
};

const ASSISTANT_CLASS: &str = "scry-chat-section-assistant";

const RENDER_TIME_BUFFER: Duration = Duration::from_millis(42);

pub(crate) struct AssistantSection {
    view: GtkBox,
    markdown_view: RefCell<MarkdownView>,
    source: Rc<RefCell<String>>,
    deadline: Cell<Instant>,
    pending: Cell<bool>,
}

impl AssistantSection {
    pub(crate) fn new(provider_backend_id: ProviderBackendId) -> Self {
        let source = Rc::new(RefCell::new(String::new()));
        let markdown_view = MarkdownView::new();

        let view = new_section(None, ASSISTANT_CLASS);
        let header = GtkBox::new(Orientation::Horizontal, 8);
        let role = Label::builder()
            .label(provider_backend_id.to_string())
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["scry-chat-role"])
            .build();
        header.append(&role);
        header.append(&copy_button(source.clone()));
        view.append(&header);
        view.append(&markdown_view.widget);

        Self {
            view,
            markdown_view: RefCell::new(markdown_view),
            source,
            deadline: Cell::new(Instant::now()),
            pending: Cell::new(false),
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn append(&self, text: &str) {
        self.source.borrow_mut().push_str(text);
        if Instant::now() > self.deadline.get() {
            self.render_now();
            self.deadline.set(Instant::now() + RENDER_TIME_BUFFER);
            self.pending.set(false);
        } else {
            self.pending.set(true);
        }
    }

    fn render_now(&self) {
        let parse_start = log::log_enabled!(log::Level::Trace).then(Instant::now);
        let (parsed, src_len) = {
            let src = self.source.borrow();
            (ParsedMarkdown::parse(&src), src.len())
        };
        let stats = parse_start.map(|t| {
            (
                t.elapsed().as_micros(),
                parsed.top_level_blocks(),
                parsed.node_count(),
            )
        });
        let apply_start = parse_start.map(|_| Instant::now());
        self.markdown_view.borrow_mut().apply_parsed(parsed);
        if let (Some((parse_us, top, nodes)), Some(apply_start)) = (stats, apply_start) {
            log::trace!(
                "md render: src_len={src_len} top_level={top} nodes={nodes} \
                 parse_us={parse_us} apply_us={}",
                apply_start.elapsed().as_micros()
            );
        }
    }

    pub(crate) fn complete(&self) {
        if self.pending.get() {
            self.render_now();
        }
        self.pending.set(false);
    }
}

fn copy_button(source: Rc<RefCell<String>>) -> Button {
    let copy = Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy markdown")
        .css_classes(["flat", "scry-code-copy"])
        .build();
    copy.connect_clicked(move |button| {
        button.clipboard().set_text(source.borrow().as_str());
    });
    copy
}
