use std::{cell::RefCell, rc::Rc, sync::Arc};

use gtk4::{Align, Box as GtkBox, Button, Label, Orientation, TextView, Widget, prelude::*};
use log::error;
use scry_core::{AppContext, PermissionState, ProviderId, UserDecision};

use crate::{helper::Clear, runtime};

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
            sections::{Section, ToolCallDecision, ToolCallSection},
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
            pending_decisions: PendingDecisions::new(app_context),
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
        let (toolcall_section, decisions) =
            ToolCallSection::new(name, arguments, description, decisions);
        self.turns.append(toolcall_section.widgets());

        let on_finish: Rc<dyn Fn(PermissionState)> = {
            let toolcall_section = toolcall_section.clone();
            Rc::new(move |state| toolcall_section.on_finish(&state))
        };

        self.pending_decisions
            .append_decisions(&decisions, on_finish);
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

struct Group {
    buttons: Vec<Button>,
    finish: Rc<dyn Fn(PermissionState)>,
    finished: bool,
}

struct PendingDecisionsState {
    groups: Vec<Group>,
    active: usize,
    current: Option<usize>,
    app_context: Arc<AppContext>,
}

struct PendingDecisions {
    state: Rc<RefCell<PendingDecisionsState>>,
}

impl PendingDecisions {
    fn new(app_context: Arc<AppContext>) -> Self {
        Self {
            state: Rc::new(RefCell::new(PendingDecisionsState {
                groups: vec![],
                active: 0,
                current: None,
                app_context,
            })),
        }
    }

    fn append_decisions(
        &self,
        decisions: &[ToolCallDecision],
        finish: Rc<dyn Fn(PermissionState)>,
    ) {
        if decisions.is_empty() {
            return;
        }

        let state = Rc::downgrade(&self.state);
        let (group, app_context) = {
            let mut state = self.state.borrow_mut();
            let group = state.groups.len();
            let app_context = state.app_context.clone();

            state.active += decisions.len();
            state.groups.push(Group {
                buttons: decisions.iter().map(|d| d.action.clone()).collect(),
                finished: false,
                finish: finish.clone(),
            });
            if state.current.is_none() {
                let has_active = state.active > 0;
                state.select(has_active.then_some(0));
            }

            (group, app_context)
        };

        for toolcall_decision in decisions {
            let app_context = app_context.clone();
            let decision = toolcall_decision.decision.clone();
            let on_finish = finish.clone();
            let state = state.clone();
            toolcall_decision.action.connect_clicked(move |_| {
                let app_context = app_context.clone();
                let decision = decision.clone();
                let on_finish = on_finish.clone();
                let state = state.clone();
                runtime::spawn_with(
                    async move { app_context.decide_toolcall_permissions(decision).await },
                    move |result| {
                        let permission_state = result.unwrap_or_else(|error| {
                            error!("decide: {error}");
                            PermissionState::Error
                        });
                        on_finish(permission_state);
                        if let Some(state) = state.upgrade() {
                            state.borrow_mut().finish_group(group);
                        }
                    },
                );
            });
        }
    }

    fn clear(&self, permission_state: PermissionState) -> bool {
        self.state.borrow_mut().clear(permission_state)
    }

    fn navigate(&self, delta: i32) -> bool {
        self.state.borrow_mut().navigate(delta)
    }

    fn activate(&self) -> bool {
        let button = {
            let state = self.state.borrow();
            let Some(index) = state.current else {
                return false;
            };
            let Some((group, button)) = state.locate(index) else {
                return false;
            };
            state.groups[group].buttons[button].clone()
        };

        button.emit_clicked();
        true
    }
}

impl PendingDecisionsState {
    fn clear(&mut self, permission_state: PermissionState) -> bool {
        self.select(None);
        let mut clear = false;
        for group in &self.groups {
            if !group.finished {
                (group.finish)(permission_state.clone());
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

    fn finish_group(&mut self, group: usize) {
        let Some(candidate) = self.groups.get(group) else {
            return;
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
