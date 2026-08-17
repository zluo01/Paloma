use std::{cell::RefCell, rc::Rc};

use futures::channel::mpsc;
use gtk4::{Align, Box as GtkBox, Label, Orientation, TextView, Widget, prelude::*};
use log::error;
use paloma_core::{PermissionState, ProviderBackendId, UserDecision};

use crate::helper::Clear;

mod helper;
mod markdown;
mod parser;
mod sections;
mod status;

use self::sections::{AssistantSection, ReasoningSection, UserPromptSection};
use crate::widgets::overlay::{
    OVERLAY_WIDTH_PX,
    model::Msg,
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
    pending_decisions: PendingDecisions,
    prev_section: RefCell<Option<Section>>,
}

impl ChatView {
    pub(crate) fn new() -> Self {
        let turns = GtkBox::builder().orientation(Orientation::Vertical).build();
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .halign(Align::Center)
            .valign(Align::Start)
            .width_request(OVERLAY_WIDTH_PX)
            .css_classes(["paloma-chat-card"])
            .build();
        widget.append(&turns);

        let status = StatusView::new();
        widget.append(status.widget());

        Self {
            widget,
            turns,
            status,
            pending_decisions: PendingDecisions::new(),
            prev_section: RefCell::new(None),
        }
    }

    pub(crate) fn widget(&self) -> &GtkBox {
        &self.widget
    }

    pub(crate) fn clear(&self) {
        self.pending_decisions.clear(PermissionState::Allow);
        self.turns.clear();
        self.status.finish();
        *self.prev_section.borrow_mut() = None;
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
        tool_name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) {
        if let Some(prev) = self.prev_section.borrow().as_ref() {
            prev.complete();
        }
        let toolcall_section =
            ToolCallSection::new(tool_name, arguments, description, decisions, dispatcher);
        self.turns.append(toolcall_section.widgets());

        self.pending_decisions.append_decisions(toolcall_section);
        *self.prev_section.borrow_mut() = Some(Section::ToolCall(()));
    }

    pub(crate) fn append_text(&self, text: &str, provider_backend_id: ProviderBackendId) {
        let mut slot = self.prev_section.borrow_mut();
        if let Some(Section::Assistant(assistant)) = slot.as_ref() {
            assistant.append(text);
            return;
        }
        if let Some(prev) = slot.as_ref() {
            prev.complete();
        }
        let assistant = AssistantSection::new(provider_backend_id);
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
        if self.complete_turn(PermissionState::Allow) {
            error!("unexpected toolcall cleanup in happy path. This indicates a bug.")
        }
        self.status.finish();
    }

    pub(crate) fn fail(&self, message: &str) {
        self.complete_turn(PermissionState::Error);
        self.status.fail(message);
    }

    pub(crate) fn cancel(&self) {
        self.complete_turn(PermissionState::Deny);
        self.status.cancel();
    }

    pub(crate) fn navigate(&self, delta: i32) -> bool {
        self.pending_decisions.navigate(delta)
    }

    pub(crate) fn activate(&self) -> bool {
        self.pending_decisions.activate()
    }

    pub(crate) fn copy_selection(&self) -> bool {
        let Some(text) = selected_text(self.turns.upcast_ref::<Widget>()) else {
            return false;
        };
        self.turns.clipboard().set_text(&text);
        true
    }

    pub(crate) fn resolve_tool_call(
        &self,
        user_decision: &UserDecision,
        permission_state: &PermissionState,
    ) {
        self.pending_decisions
            .finish(user_decision, permission_state)
    }

    fn complete_turn(&self, permission_state: PermissionState) -> bool {
        if let Some(prev) = self.prev_section.borrow().as_ref() {
            prev.complete();
        }
        *self.prev_section.borrow_mut() = None;
        self.pending_decisions.clear(permission_state)
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

struct PendingDecisionsState {
    groups: Vec<ToolCallSection>,
    // total active decision count
    active: usize,
    // current selected decision
    current: Option<usize>,
}

struct PendingDecisions {
    state: Rc<RefCell<PendingDecisionsState>>,
}

impl PendingDecisions {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(PendingDecisionsState {
                groups: vec![],
                active: 0,
                current: None,
            })),
        }
    }

    fn append_decisions(&self, tool_call_section: ToolCallSection) {
        if tool_call_section.decision_count() == 0 {
            return;
        }

        let mut state = self.state.borrow_mut();
        state.active += tool_call_section.decision_count();
        state.groups.push(tool_call_section);

        if state.current.is_none() {
            let has_active = state.active > 0;
            state.select(has_active.then_some(0));
        }
    }

    fn clear(&self, permission_state: PermissionState) -> bool {
        self.state.borrow_mut().clear(permission_state)
    }

    fn navigate(&self, delta: i32) -> bool {
        self.state.borrow_mut().navigate(delta)
    }

    fn activate(&self) -> bool {
        let mut state = self.state.borrow_mut();
        let Some(index) = state.current else {
            return false;
        };
        let Some((group, index)) = state.locate(index) else {
            return false;
        };

        state.groups[group].active(index);

        true
    }

    fn finish(&self, user_decision: &UserDecision, permission_state: &PermissionState) {
        self.state
            .borrow_mut()
            .finish_group(user_decision, permission_state);
    }
}

impl PendingDecisionsState {
    fn clear(&mut self, permission_state: PermissionState) -> bool {
        self.select(None);
        let mut clear = false;
        for group in &mut self.groups {
            if !group.is_finish() {
                group.on_finish(&permission_state);
                clear = true;
            }
        }
        self.groups.clear();
        self.active = 0;
        self.current = None;
        clear
    }

    fn navigate(&mut self, delta: i32) -> bool {
        if self.active == 0 {
            return false;
        }
        let current = self.current.unwrap_or(0);
        self.select(step_index(current, delta, self.active));
        true
    }

    fn finish_group(&mut self, user_decision: &UserDecision, permission_state: &PermissionState) {
        self.select(None);
        for group in &mut self.groups {
            if !group.is_finish() && group.contains(user_decision) {
                group.on_finish(permission_state);
                self.active -= group.decision_count();
                break;
            }
        }

        // autopopulate all the toolcall with ignore permission decision
        if matches!(user_decision, UserDecision::IgnorePermission { .. })
            && matches!(permission_state, PermissionState::Allow)
        {
            for group in &mut self.groups {
                group.active_matching(|d| matches!(d, UserDecision::IgnorePermission { .. }));
            }
        }

        self.select((self.active > 0).then_some(0));
    }

    fn select(&mut self, target: Option<usize>) {
        if let Some((group, idx)) = self.current.and_then(|index| self.locate(index)) {
            self.groups[group].deselect(idx);
        }
        if let Some((group, idx)) = target.and_then(|index| self.locate(index)) {
            self.groups[group].select(idx);
        }
        self.current = target;
    }

    fn locate(&self, mut index: usize) -> Option<(usize, usize)> {
        for (group, candidate) in self.groups.iter().enumerate() {
            if candidate.is_finish() {
                continue;
            }
            if index < candidate.decision_count() {
                return Some((group, index));
            }
            index -= candidate.decision_count();
        }
        None
    }
}
