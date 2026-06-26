use std::{cell::RefCell, rc::Rc, sync::Arc};

use gtk4::{Align, Box as GtkBox, Button, Label, Orientation, TextView, Widget, prelude::*};
use scry_core::{AppContext, ProviderId, UserDecision};

use crate::helper::Clear;

mod helper;
mod markdown;
mod parser;
mod sections;
mod status;

use self::sections::{AssistantSection, ReasoningSection, UserPromptSection};
use crate::widgets::overlay::{
    SELECTED_CLASS,
    results::{
        chat::{
            sections::{Section, ToolCallSection},
            status::StatusView,
        },
        step_index,
    },
};

pub struct ChatView {
    widget: GtkBox,
    turns: GtkBox,
    status: StatusView,
    app_context: Arc<AppContext>,
    pending_decisions: Rc<RefCell<PendingDecisions>>,
    prev_section: RefCell<Option<Section>>,
}

impl ChatView {
    pub(crate) fn new(width: i32, app_context: Arc<AppContext>) -> Self {
        let turns = GtkBox::builder().orientation(Orientation::Vertical).build();
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .valign(Align::Start)
            .width_request(width)
            .css_classes(["scry-chat-card"])
            .build();
        widget.append(&turns);

        let status = StatusView::new();
        widget.append(status.widget());

        Self {
            widget,
            turns,
            status,
            app_context,
            pending_decisions: Rc::new(RefCell::new(PendingDecisions::new())),
            prev_section: RefCell::new(None),
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.widget
    }

    pub(crate) fn clear(&self) {
        self.pending_decisions.borrow_mut().clear();
        self.turns.clear();
        self.status.finish();
        *self.prev_section.borrow_mut() = None;
    }

    pub(crate) fn is_running(&self) -> bool {
        self.status.is_running()
    }

    pub(crate) fn append_user_prompt(&self, prompt: &str) {
        if let Some(prev) = self.prev_section.borrow().as_ref() {
            prev.complete();
        }
        let user_prompt = UserPromptSection::new(prompt);
        self.turns.append(user_prompt.widget());
        *self.prev_section.borrow_mut() = Some(Section::UserPrompt(()));
        self.status.start();
    }

    pub(crate) fn append_tool_call(
        &self,
        name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
    ) {
        if let Some(prev) = self.prev_section.borrow().as_ref() {
            prev.complete();
        }
        let group = self.pending_decisions.borrow().group_count();
        let on_finish: Rc<dyn Fn()> = {
            let pending = Rc::downgrade(&self.pending_decisions);
            Rc::new(move || {
                if let Some(pending) = pending.upgrade() {
                    pending.borrow_mut().finish_group(group);
                }
            })
        };
        let tool_call = ToolCallSection::new(
            name,
            arguments,
            description,
            decisions,
            self.app_context.clone(),
            on_finish,
        );
        let (view, buttons) = tool_call.widgets();
        self.turns.append(view);
        self.pending_decisions
            .borrow_mut()
            .append_decisions(buttons);
        *self.prev_section.borrow_mut() = Some(Section::ToolCall(()));
    }

    pub(crate) fn append_text(&self, text: &str, provider_id: ProviderId) {
        let mut slot = self.prev_section.borrow_mut();
        if let Some(Section::Assistant(assistant)) = slot.as_ref() {
            assistant.append(text);
            return;
        }
        if let Some(prev) = slot.as_ref() {
            prev.complete();
        }
        let assistant = AssistantSection::new(provider_id);
        self.turns.append(assistant.widget());
        assistant.append(text);
        *slot = Some(Section::Assistant(assistant));
    }

    pub(crate) fn append_reasoning(&self, text: &str) {
        let mut slot = self.prev_section.borrow_mut();
        if let Some(Section::Reasoning(reasoning)) = slot.as_ref() {
            reasoning.append(text);
            return;
        }
        if let Some(prev) = slot.as_ref() {
            prev.complete();
        }
        let reasoning = ReasoningSection::new();
        self.turns.append(reasoning.widget());
        reasoning.append(text);
        *slot = Some(Section::Reasoning(reasoning));
    }

    pub(crate) fn finish(&self) {
        self.complete_turn();
        self.status.finish();
    }

    pub(crate) fn fail(&self, message: &str) {
        self.complete_turn();
        self.status.fail(message);
    }

    pub(crate) fn cancel(&self) {
        self.complete_turn();
        self.status.cancel();
    }

    pub(crate) fn navigate(&self, delta: i32) -> bool {
        self.pending_decisions.borrow_mut().navigate(delta)
    }

    pub(crate) fn activate(&self) -> bool {
        self.pending_decisions.borrow_mut().activate()
    }

    pub(crate) fn copy_selection(&self) -> bool {
        let Some(text) = selected_text(self.turns.upcast_ref::<Widget>()) else {
            return false;
        };
        self.turns.clipboard().set_text(&text);
        true
    }

    fn complete_turn(&self) {
        if let Some(prev) = self.prev_section.borrow().as_ref() {
            prev.complete();
        }
        *self.prev_section.borrow_mut() = None;
        self.pending_decisions.borrow_mut().clear();
    }
}

fn selected_text(root: &Widget) -> Option<String> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if let Some(label) = node.downcast_ref::<Label>() {
            if let Some((start, end)) = label.selection_bounds() {
                let (start, end) = (start as usize, end as usize);
                return Some(label.text().chars().skip(start).take(end - start).collect());
            }
        } else if let Some(view) = node.downcast_ref::<TextView>() {
            let buffer = view.buffer();
            if let Some((start, end)) = buffer.selection_bounds() {
                return Some(buffer.text(&start, &end, false).to_string());
            }
        }

        // Push children reversed so the first child is popped (visited) first,
        // preserving the original pre-order, left-to-right walk.
        let mut children = Vec::new();
        let mut next = node.first_child();
        while let Some(child) = next {
            next = child.next_sibling();
            children.push(child);
        }
        stack.extend(children.into_iter().rev());
    }
    None
}

struct Group {
    buttons: Vec<Button>,
    finished: bool,
}

struct PendingDecisions {
    groups: Vec<Group>,
    active: usize,
    current: Option<usize>,
}

impl PendingDecisions {
    fn new() -> Self {
        Self {
            groups: vec![],
            active: 0,
            current: None,
        }
    }

    fn append_decisions(&mut self, decisions: &[Button]) {
        if decisions.is_empty() {
            return;
        }
        self.active += decisions.len();
        self.groups.push(Group {
            buttons: decisions.to_vec(),
            finished: false,
        });
        if self.current.is_none() {
            self.select((self.active > 0).then_some(0));
        }
    }

    fn clear(&mut self) {
        self.select(None);
        self.groups.clear();
        self.active = 0;
    }

    fn navigate(&mut self, delta: i32) -> bool {
        if self.active == 0 {
            return false;
        }
        let current = self.current.unwrap_or(0);
        self.select(step_index(current, delta, self.active));
        true
    }

    fn activate(&mut self) -> bool {
        let Some(index) = self.current else {
            return false;
        };
        let Some((group, button)) = self.locate(index) else {
            return false;
        };
        self.groups[group].buttons[button].emit_clicked();
        true
    }

    fn group_count(&self) -> usize {
        self.groups.len()
    }

    fn finish_group(&mut self, group: usize) {
        let Some(candidate) = self.groups.get(group) else {
            return; // turn was cleared before the deferred callback ran
        };
        if candidate.finished {
            return;
        }
        self.groups[group].finished = true;
        self.active -= self.groups[group].buttons.len();
        self.select((self.active > 0).then_some(0));
    }

    fn select(&mut self, target: Option<usize>) {
        if let Some((group, button)) = self.current.and_then(|index| self.locate(index)) {
            self.groups[group].buttons[button].remove_css_class(SELECTED_CLASS);
        }
        if let Some((group, button)) = target.and_then(|index| self.locate(index)) {
            self.groups[group].buttons[button].add_css_class(SELECTED_CLASS);
        }
        self.current = target;
    }

    fn locate(&self, mut index: usize) -> Option<(usize, usize)> {
        for (group, candidate) in self.groups.iter().enumerate() {
            if candidate.finished {
                continue;
            }
            if index < candidate.buttons.len() {
                return Some((group, index));
            }
            index -= candidate.buttons.len();
        }
        None
    }
}
