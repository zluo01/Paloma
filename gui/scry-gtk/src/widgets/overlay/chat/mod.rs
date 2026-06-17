//! Chat surface: one [`ChatTurn`] per user prompt, with stream events appended
//! as role sections. Assistant text renders as markdown; transcript text is
//! selectable but not editable.

use std::{cell::RefCell, rc::Rc};

use gtk4::{
    Align, Box as GtkBox, Button, Label, Orientation, TextBuffer, TextView, Widget, WrapMode,
    pango, prelude::*,
};
use libadwaita::Spinner;

mod decisions;
mod markdown;

use scry_core::UserDecision;

use self::{
    decisions::{PendingDecisions, decision_prompt},
    markdown::MarkdownView,
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
    assistant: String,
    assistant_block: Option<AssistantBlock>,
}

/// Role section wrapping a [`MarkdownView`] for assistant output.
struct AssistantBlock {
    section: GtkBox,
    view: MarkdownView,
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

    pub(super) fn append_text(&self, text: &str) {
        let mut turns = self.turns.borrow_mut();
        let turn = current_turn(&mut turns, &self.turns_box);
        turn.assistant.push_str(text);
        turn.render_assistant();
    }

    /// Push a new turn with `prompt` as its user bubble. This also completes
    /// the prior turn; history replay can deliver consecutive `UserPrompt`s
    /// without an intervening `Done`.
    pub(super) fn start_turn(&self, prompt: &str) {
        let mut turns = self.turns.borrow_mut();
        if let Some(prev) = turns.last_mut() {
            prev.mark_complete();
        }
        turns.push(ChatTurn::new(prompt, &self.turns_box));
    }

    pub(super) fn finish_turn(&self) {
        if let Some(turn) = self.turns.borrow_mut().last_mut() {
            turn.mark_complete();
        }
    }

    pub(super) fn fail_turn(&self, message: &str) {
        if let Some(turn) = self.turns.borrow().last() {
            turn.mark_ended(message, "scry-chat-error");
        }
    }

    pub(super) fn cancel_turn(&self) {
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

    /// Copy the transcript's current text selection to the clipboard, returning
    /// whether anything was copied. The transcript window has no keyboard, so
    /// Ctrl+C never reaches its text widgets; the controller calls this directly.
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
            assistant: String::new(),
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

    fn render_assistant(&mut self) {
        let block = self.assistant_block.get_or_insert_with(|| {
            let section = append_section(&self.body, ASSISTANT_TITLE, ASSISTANT_CLASS);
            let view = MarkdownView::new();
            section.append(&view.widget);
            AssistantBlock { section, view }
        });
        block.view.set_markdown(&self.assistant);
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
        // Drop the in-progress block handles, leaving their widgets on screen,
        // so later deltas open new sections below the tool call.
        self.assistant_block = None;
        self.assistant.clear();
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
