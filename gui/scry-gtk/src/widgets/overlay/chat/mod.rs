//! Chat surface: one [`ChatTurn`] per user prompt, with stream events appended
//! as role sections. Assistant text renders as markdown; transcript text is
//! selectable but not editable.

use std::{cell::RefCell, rc::Rc};

use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, TextBuffer, TextView, Widget, WrapMode, glib,
    pango, prelude::*,
};
use libadwaita::Spinner;

mod decisions;
mod markdown;

use scry_core::UserDecision;

use self::{
    decisions::{PendingDecisions, decision_prompt},
    markdown::{MarkdownView, ParsedMarkdown},
};
use super::OnDecideFn;
use crate::widgets::clear_children;

const USER_TITLE: &str = "You";
const REASONING_TITLE: &str = "Thinking";
const ASSISTANT_TITLE: &str = "Scry";
const USER_CLASS: &str = "scry-chat-section-user";
const REASONING_CLASS: &str = "scry-chat-section-thinking";
const ASSISTANT_CLASS: &str = "scry-chat-section-assistant";
const TOOL_CLASS: &str = "scry-chat-section-tool";

/// Chat card, turn, section, and text styling.
pub(super) const CSS: &str = include_str!("style.css");

/// Styled chat card. Clones share the same GTK widgets and state.
#[derive(Clone)]
pub(super) struct ChatView {
    widget: GtkBox,
    turns_box: GtkBox,
    turns: Rc<RefCell<Vec<ChatTurn>>>,
    /// Unresolved permission prompts, arrow-key navigable as one sequence
    /// across tool calls.
    pending_decisions: Rc<RefCell<PendingDecisions>>,
}

struct ChatTurn {
    /// Outer box: `body` first, then `pending`, so the indicator always
    /// sits below the last section without reordering.
    container: GtkBox,
    body: GtkBox,
    /// Spinner + status row shown while a turn streams.
    pending: GtkBox,
    spinner: Spinner,
    status: Label,
    reasoning: String,
    reasoning_block: Option<TextBlock>,
    assistant_block: Option<AssistantBlock>,
}

/// At most this often does a streaming assistant section re-render markdown,
/// bounding the per-delta full reparse; a segment end flushes immediately.
const RENDER_THROTTLE: std::time::Duration = std::time::Duration::from_millis(33);

/// Role section for assistant output. Its widgets stay on screen, but the live
/// render handle is dropped when a tool call splits the turn.
struct AssistantBlock {
    section: GtkBox,
    handle: AssistantHandle,
}

/// Coalesced render state for one assistant section, behind an `Rc<RefCell>` so the
/// throttle timer (a `Weak`) and the copy button outlive the transient handles used
/// while streaming.
struct RenderState {
    view: MarkdownView,
    /// The segment's markdown source, shared with the section copy button.
    source: Rc<RefCell<String>>,
    dirty: bool,
    pending: Option<glib::SourceId>,
}

impl Drop for RenderState {
    fn drop(&mut self) {
        // Cancel a still-armed timer so it can't fire into a freed section (e.g.
        // after `ChatView::reset` drops the turns).
        if let Some(id) = self.pending.take() {
            id.remove();
        }
    }
}

/// Cheap-cloneable handle to a [`RenderState`]; exposes only delta append and
/// flush. Timers, the dirty flag, and the source cell stay private.
#[derive(Clone)]
struct AssistantHandle(Rc<RefCell<RenderState>>);

impl AssistantHandle {
    /// Append streamed text (so copy is immediately current) and coalesce the
    /// render: render now on the first delta of a burst, then at most once per
    /// [`RENDER_THROTTLE`] until idle.
    fn append_delta(&self, text: &str) {
        let arm = {
            let state = self.0.borrow();
            state.source.borrow_mut().push_str(text);
            state.pending.is_none()
        };
        if arm {
            self.render_now();
            self.arm();
        } else {
            self.0.borrow_mut().dirty = true;
        }
    }

    /// Parse the latest source under a brief source borrow, drop it, then apply —
    /// no source borrow is held across the GTK build (the `RenderState` borrow is,
    /// but nothing re-enters it). Clears `dirty`.
    fn render_now(&self) {
        let mut state = self.0.borrow_mut();
        let parse_start = log::log_enabled!(log::Level::Trace).then(std::time::Instant::now);
        let (parsed, src_len) = {
            let src = state.source.borrow();
            (ParsedMarkdown::parse(&src), src.len())
        };
        // Stats are read before `apply_parsed` consumes `parsed`, and only under
        // trace, so the normal path stays parse + prefix-diff apply.
        let stats = parse_start.map(|t| {
            (
                t.elapsed().as_micros(),
                parsed.top_level_blocks(),
                parsed.node_count(),
            )
        });
        let apply_start = parse_start.map(|_| std::time::Instant::now());
        state.view.apply_parsed(parsed);
        state.dirty = false;
        if let (Some((parse_us, top, nodes)), Some(apply_start)) = (stats, apply_start) {
            log::trace!(
                "md render: src_len={src_len} top_level={top} nodes={nodes} \
                 parse_us={parse_us} apply_us={}",
                apply_start.elapsed().as_micros()
            );
        }
    }

    /// Schedule a single coalesced re-render after [`RENDER_THROTTLE`].
    fn arm(&self) {
        let weak = Rc::downgrade(&self.0);
        let id = glib::timeout_add_local_once(RENDER_THROTTLE, move || {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let handle = AssistantHandle(state);
            let dirty = {
                let mut state = handle.0.borrow_mut();
                state.pending = None;
                state.dirty
            };
            if dirty {
                handle.render_now();
                handle.arm();
            }
        });
        self.0.borrow_mut().pending = Some(id);
    }

    /// Render any pending content now and cancel the timer — for segment ends
    /// (turn finished/failed/cancelled, or a tool call splitting the turn).
    fn flush(&self) {
        let (dirty, pending) = {
            let mut state = self.0.borrow_mut();
            (state.dirty, state.pending.take())
        };
        if dirty {
            self.render_now();
        }
        if let Some(id) = pending {
            id.remove();
        }
    }
}

impl ChatView {
    pub(super) fn new(width: i32) -> Self {
        let turns_box = GtkBox::builder().orientation(Orientation::Vertical).build();
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .valign(Align::Start)
            .visible(false)
            .width_request(width)
            .build();
        widget.add_css_class("scry-chat-card");
        widget.append(&turns_box);
        Self {
            widget,
            turns_box,
            turns: Rc::new(RefCell::new(Vec::new())),
            pending_decisions: Rc::new(RefCell::new(PendingDecisions::default())),
        }
    }

    pub(super) fn widget(&self) -> &GtkBox {
        &self.widget
    }

    pub(super) fn reset(&self) {
        self.pending_decisions.borrow_mut().clear();
        self.turns.borrow_mut().clear();
        clear_children(&self.turns_box);
    }

    pub(super) fn show(&self) {
        self.widget.set_visible(true);
    }

    pub(super) fn hide(&self) {
        self.widget.set_visible(false);
    }

    /// Render handle of the current turn's assistant section, if one exists. Cloned
    /// under a brief borrow so the caller can flush/render outside it (never hold a
    /// `turns` borrow across a GTK render).
    fn last_assistant_handle(&self) -> Option<AssistantHandle> {
        self.turns
            .borrow()
            .last()
            .and_then(ChatTurn::assistant_handle_existing)
    }

    pub(super) fn append_text(&self, text: &str) {
        let handle = {
            let mut turns = self.turns.borrow_mut();
            current_turn(&mut turns, &self.turns_box).assistant_handle()
        };
        // Append + render run outside the `turns` borrow.
        handle.append_delta(text);
    }

    /// Push a new turn with `prompt` as its user bubble. This also completes
    /// the prior turn; history replay can deliver consecutive `UserPrompt`s
    /// without an intervening `Done`.
    pub(super) fn start_turn(&self, prompt: &str) {
        if let Some(handle) = self.last_assistant_handle() {
            handle.flush();
        }
        let mut turns = self.turns.borrow_mut();
        if let Some(prev) = turns.last_mut() {
            prev.mark_complete();
        }
        turns.push(ChatTurn::new(prompt, &self.turns_box));
    }

    pub(super) fn finish_turn(&self) {
        if let Some(handle) = self.last_assistant_handle() {
            handle.flush();
        }
        if let Some(turn) = self.turns.borrow().last() {
            turn.mark_complete();
        }
    }

    pub(super) fn fail_turn(&self, message: &str) {
        if let Some(handle) = self.last_assistant_handle() {
            handle.flush();
        }
        if let Some(turn) = self.turns.borrow().last() {
            turn.mark_ended(message, "scry-chat-error");
        }
    }

    pub(super) fn cancel_turn(&self) {
        if let Some(handle) = self.last_assistant_handle() {
            handle.flush();
        }
        if let Some(turn) = self.turns.borrow().last() {
            turn.mark_ended("Cancelled", "scry-chat-cancel");
        }
    }

    pub(super) fn append_reasoning(&self, text: &str) {
        let mut turns = self.turns.borrow_mut();
        let turn = current_turn(&mut turns, &self.turns_box);
        turn.reasoning.push_str(text);
        turn.render_reasoning();
    }

    pub(super) fn add_tool_call(
        &self,
        name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
        on_decide: OnDecideFn,
    ) {
        // Flush the in-progress assistant section before it is split off below the
        // tool call (outside the `turns` borrow).
        if let Some(handle) = self.last_assistant_handle() {
            handle.flush();
        }
        let mut turns = self.turns.borrow_mut();
        let turn = current_turn(&mut turns, &self.turns_box);
        turn.add_tool_call(
            name,
            arguments,
            description,
            decisions,
            on_decide,
            &self.pending_decisions,
        );
    }

    /// Move the permission-prompt highlight by `delta`. Returns false
    /// when no prompt is pending (the key should fall through).
    pub(super) fn navigate_decisions(&self, delta: i32) -> bool {
        self.pending_decisions.borrow_mut().navigate(delta)
    }

    pub(super) fn selected_decision(&self) -> Option<Button> {
        self.pending_decisions.borrow().selected_button()
    }

    /// Confirm the highlighted permission decision via the same
    /// `clicked` path a mouse click takes. Returns false when no
    /// prompt is pending.
    pub(super) fn activate_selected_decision(&self) -> bool {
        let Some(button) = self.pending_decisions.borrow().selected_button() else {
            return false;
        };
        button.emit_clicked();
        true
    }

    /// Copy the **first** selected text widget's selection to the clipboard,
    /// returning whether anything was copied. Selection copy is per-widget — the
    /// markdown renderer is a widget tree, so a drag can't span blocks; the
    /// assistant section's "Copy markdown" button is the reliable full-answer copy.
    /// The transcript window has no keyboard, so Ctrl+C never reaches its text
    /// widgets; the controller calls this directly.
    pub(super) fn copy_selection(&self) -> bool {
        let Some(text) = selected_text(self.turns_box.upcast_ref::<Widget>()) else {
            return false;
        };
        self.turns_box.clipboard().set_text(&text);
        true
    }
}

impl ChatTurn {
    fn new(prompt: &str, parent: &GtkBox) -> Self {
        let container = GtkBox::builder().orientation(Orientation::Vertical).build();
        container.add_css_class("scry-chat-turn");
        parent.append(&container);

        let body = GtkBox::builder().orientation(Orientation::Vertical).build();
        container.append(&body);

        let section = append_section(&body, USER_TITLE, USER_CLASS);
        append_content_label(&section, prompt, USER_CLASS);

        let pending = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .halign(Align::Start)
            .build();
        pending.add_css_class("scry-chat-pending");
        let spinner = Spinner::new();
        spinner.set_size_request(14, 14);
        pending.append(&spinner);
        let status = Label::builder()
            .label("Thinking…")
            .wrap(true)
            .max_width_chars(50)
            .xalign(0.0)
            .build();
        pending.append(&status);
        container.append(&pending);

        Self {
            container,
            body,
            pending,
            spinner,
            status,
            reasoning: String::new(),
            reasoning_block: None,
            assistant_block: None,
        }
    }

    /// Hide the pending row and tag the turn complete.
    fn mark_complete(&self) {
        self.pending.set_visible(false);
        self.container.add_css_class("complete");
    }

    /// Replace the pending row with an error/cancel status.
    fn mark_ended(&self, text: &str, class: &str) {
        self.spinner.set_visible(false);
        self.status.set_text(text);
        self.pending.add_css_class(class);
        self.container.add_css_class("complete");
    }

    fn render_reasoning(&mut self) {
        if self.reasoning_block.is_none() {
            self.reasoning_block = Some(TextBlock::plain(
                &self.body,
                REASONING_TITLE,
                REASONING_CLASS,
                "",
            ));
            // Keep the assistant section below the reasoning it follows.
            if let Some(assistant) = &self.assistant_block {
                self.body.remove(&assistant.section);
                self.body.append(&assistant.section);
            }
        }
        if let Some(block) = &self.reasoning_block {
            block.set_plain(&self.reasoning);
        }
    }

    /// Ensure the assistant section exists and return a clone of its render handle.
    fn assistant_handle(&mut self) -> AssistantHandle {
        if self.assistant_block.is_none() {
            let source = Rc::new(RefCell::new(String::new()));
            let view = MarkdownView::new();
            let view_widget = view.widget.clone();
            let state = Rc::new(RefCell::new(RenderState {
                view,
                source: source.clone(),
                dirty: false,
                pending: None,
            }));
            let section = append_section_with_action(
                &self.body,
                ASSISTANT_TITLE,
                ASSISTANT_CLASS,
                &assistant_copy_button(source),
            );
            section.append(&view_widget);
            self.assistant_block = Some(AssistantBlock {
                section,
                handle: AssistantHandle(state),
            });
        }
        self.assistant_block
            .as_ref()
            .expect("assistant block created above")
            .handle
            .clone()
    }

    fn assistant_handle_existing(&self) -> Option<AssistantHandle> {
        self.assistant_block
            .as_ref()
            .map(|block| block.handle.clone())
    }

    fn add_tool_call(
        &mut self,
        name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
        on_decide: OnDecideFn,
        pending: &Rc<RefCell<PendingDecisions>>,
    ) {
        // Drop the live render handle (the section's widgets and copy button stay
        // on screen), so later deltas open a new assistant section below the tool
        // call. The caller flushes the old section first.
        self.assistant_block = None;
        self.reasoning_block = None;
        self.reasoning.clear();

        // The tool name captions the arguments card, so the section
        // needs no separate role label. Human-readable summary first
        // (when supplied), so the user reads intent before mechanics.
        let section = new_section(&self.body, TOOL_CLASS);
        if let Some(description) = description.filter(|d| !d.is_empty()) {
            append_content_label(&section, description, "scry-chat-tool-description");
        }
        section.append(&code_card(name, arguments));

        if !decisions.is_empty() {
            section.append(&decision_prompt(decisions, &on_decide, pending));
        }
    }
}

/// Role section with a `TextView`-backed plain-text body, re-rendered
/// on each streaming delta while preserving any text selection.
struct TextBlock {
    buffer: TextBuffer,
}

impl TextBlock {
    fn plain(parent: &GtkBox, title: &str, variant: &str, text: &str) -> Self {
        let block = Self::new(parent, title, variant);
        block.set_plain(text);
        block
    }

    fn new(parent: &GtkBox, title: &str, variant: &str) -> Self {
        let section = append_section(parent, title, variant);
        let buffer = TextBuffer::new(None);

        let text_view = TextView::builder()
            .buffer(&buffer)
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(WrapMode::WordChar)
            .hexpand(true)
            .focusable(true)
            .can_focus(true)
            .focus_on_click(true)
            .left_margin(0)
            .right_margin(0)
            .top_margin(0)
            .bottom_margin(0)
            .build();
        text_view.add_css_class("scry-chat-text");
        text_view.add_css_class(variant);
        section.append(&text_view);

        Self { buffer }
    }

    fn set_plain(&self, text: &str) {
        self.update_preserving_selection(|buffer| buffer.set_text(text));
    }

    fn update_preserving_selection(&self, update: impl FnOnce(&TextBuffer)) {
        let saved_selection = self
            .buffer
            .selection_bounds()
            .map(|(start, end)| (start.offset(), end.offset()));

        update(&self.buffer);

        if let Some((start, end)) = saved_selection {
            let end_offset = self.buffer.end_iter().offset();
            if start <= end_offset && end <= end_offset {
                let start_iter = self.buffer.iter_at_offset(start);
                let end_iter = self.buffer.iter_at_offset(end);
                self.buffer.select_range(&start_iter, &end_iter);
            }
        }
    }
}

/// Append a selectable, wrapping label for set-once content.
///
/// GTK can collapse selectable wrapping labels without a width hint. With
/// `width_chars(1)`, `hexpand`, and `halign(Fill)`, the parent allocates full
/// width and wrapping works inside it.
fn append_content_label(parent: &GtkBox, text: &str, variant: &str) {
    let content = Label::builder()
        .label(text)
        .xalign(0.0)
        .halign(Align::Fill)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .selectable(true)
        .hexpand(true)
        .width_chars(1)
        .build();
    content.add_css_class("scry-chat-text");
    content.add_css_class(variant);
    parent.append(&content);
}

/// A code-style card with a caption, copy button, and selectable body.
pub(super) fn code_card(caption: &str, body: &str) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .build();
    card.add_css_class("scry-code");

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.append(
        &Label::builder()
            .label(caption)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["scry-code-caption"])
            .build(),
    );
    let copy = Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy")
        .css_classes(["flat", "scry-code-copy"])
        .build();
    {
        let body = body.to_string();
        copy.connect_clicked(move |button| button.clipboard().set_text(&body));
    }
    header.append(&copy);
    card.append(&header);

    let content = Label::builder()
        .label(body)
        .xalign(0.0)
        .halign(Align::Fill)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .selectable(true)
        .width_chars(1)
        .css_classes(["scry-code-body", "scry-chat-text", "monospace"])
        .build();
    card.append(&content);

    card
}

/// A role section: a colored-left-rule box appended to `parent`.
fn new_section(parent: &GtkBox, variant: &str) -> GtkBox {
    let section = GtkBox::builder().orientation(Orientation::Vertical).build();
    section.add_css_class("scry-chat-section");
    section.add_css_class(variant);
    parent.append(&section);
    section
}

/// A role section headed by a `title` role label (You / Thinking / Scry).
fn append_section(parent: &GtkBox, title: &str, variant: &str) -> GtkBox {
    let section = new_section(parent, variant);
    let role = Label::builder().label(title).xalign(0.0).build();
    role.add_css_class("scry-chat-role");
    section.append(&role);
    section
}

/// A role section whose header row carries `title` plus a trailing `action`
/// widget (used for the assistant "Copy markdown" button). Only the assistant
/// section needs this; other roles use [`append_section`].
fn append_section_with_action(
    parent: &GtkBox,
    title: &str,
    variant: &str,
    action: &impl IsA<Widget>,
) -> GtkBox {
    let section = new_section(parent, variant);
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let role = Label::builder()
        .label(title)
        .xalign(0.0)
        .hexpand(true)
        .build();
    role.add_css_class("scry-chat-role");
    header.append(&role);
    header.append(action);
    section.append(&header);
    section
}

/// Copy button for an assistant section: copies its accumulated markdown source.
/// Holds a strong clone of `source` so it keeps working after the section's render
/// handle is dropped (e.g. when a tool call splits the turn).
fn assistant_copy_button(source: Rc<RefCell<String>>) -> Button {
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

fn current_turn<'a>(turns: &'a mut Vec<ChatTurn>, turns_box: &GtkBox) -> &'a mut ChatTurn {
    if turns.is_empty() {
        turns.push(ChatTurn::new("", turns_box));
    }
    turns.last_mut().expect("turn is inserted when missing")
}

/// First selectable `Label` or `TextView` selection in a depth-first transcript
/// walk. Skips the transient pending/status row; reasoning text remains copyable.
fn selected_text(widget: &Widget) -> Option<String> {
    if widget.has_css_class("scry-chat-pending") {
        return None;
    }
    if let Some(label) = widget.downcast_ref::<Label>() {
        if let Some((start, end)) = label.selection_bounds() {
            let (start, end) = (start as usize, end as usize);
            return Some(label.text().chars().skip(start).take(end - start).collect());
        }
    } else if let Some(view) = widget.downcast_ref::<TextView>() {
        let buffer = view.buffer();
        if let Some((start, end)) = buffer.selection_bounds() {
            return Some(buffer.text(&start, &end, false).to_string());
        }
    }

    let mut child = widget.first_child();
    while let Some(node) = child {
        if let Some(text) = selected_text(&node) {
            return Some(text);
        }
        child = node.next_sibling();
    }
    None
}
