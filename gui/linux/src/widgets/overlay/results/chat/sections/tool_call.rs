use std::cmp::PartialEq;

use futures::channel::mpsc;
use gtk4::{
    Box as GtkBox, Button, Label, Orientation, Separator, pango,
    prelude::{BoxExt, ButtonExt, WidgetExt},
};
use paloma_core::{PermissionState, UserDecision};

use crate::{
    helper::{Clear, scroll_into_view},
    widgets::overlay::{
        SELECTED_CLASS,
        model::{ChatMsg, Msg},
        results::chat::helper::{append_content_label, code_card, new_section},
    },
};

const TOOL_CLASS: &str = "paloma-chat-section-tool";

#[derive(Eq, PartialEq)]
enum ToolCallState {
    Waiting,
    Decided,
    Done,
}

pub(crate) struct ToolCallDecision {
    pub action: Button,
    pub decision: UserDecision,
}

pub(crate) struct ToolCallSection {
    view: GtkBox,
    decision_group: Option<GtkBox>,
    decisions: Vec<ToolCallDecision>,
    state: ToolCallState,
}

impl ToolCallSection {
    pub(crate) fn new(
        tool_name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
        dispatcher: mpsc::UnboundedSender<Msg>,
    ) -> Self {
        let view = new_section(None, TOOL_CLASS);
        if let Some(description) = description.filter(|d| !d.is_empty()) {
            append_content_label(&view, description, "paloma-chat-tool-description");
        }
        view.append(&code_card(tool_name, arguments));

        let (decision_group, decisions, state) = if !decisions.is_empty() {
            let (decision_group, parsed) = decision_button_group(decisions, dispatcher);
            view.append(&decision_group);
            (Some(decision_group), parsed, ToolCallState::Waiting)
        } else {
            (None, vec![], ToolCallState::Done)
        };

        Self {
            view,
            decision_group,
            decisions,
            state,
        }
    }

    pub(crate) fn widgets(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn decision_count(&self) -> usize {
        self.decisions.len()
    }

    pub(crate) fn active(&mut self, index: usize) {
        if self.state != ToolCallState::Waiting {
            return;
        }
        if let Some(decision) = self.decisions.get(index)
            && decision.action.is_sensitive()
        {
            self.state = ToolCallState::Decided;
            decision.action.emit_clicked();
        }
    }

    pub(crate) fn select(&self, index: usize) {
        if let Some(decision) = self.decisions.get(index) {
            decision.action.add_css_class(SELECTED_CLASS);
            scroll_into_view(&decision.action);
        }
    }

    pub(crate) fn is_finish(&self) -> bool {
        self.state == ToolCallState::Done
    }

    pub(crate) fn deselect(&self, index: usize) {
        if let Some(decision) = self.decisions.get(index) {
            decision.action.remove_css_class(SELECTED_CLASS);
        }
    }

    pub(crate) fn contains(&self, user_decision: &UserDecision) -> bool {
        self.decisions.iter().any(|d| &d.decision == user_decision)
    }

    pub(crate) fn on_finish(&mut self, permission_state: &PermissionState) {
        if let Some(actions) = &self.decision_group {
            self.state = ToolCallState::Done;
            resolve_decision(actions, permission_state);
        }
    }
}

fn decision_button_group(
    user_decisions: &[UserDecision],
    dispatcher: mpsc::UnboundedSender<Msg>,
) -> (GtkBox, Vec<ToolCallDecision>) {
    let actions = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .build();
    actions.add_css_class("paloma-chat-tool-decisions");

    let caption = Label::builder()
        .label("Needs your permission")
        .xalign(0.0)
        .build();
    caption.add_css_class("paloma-chat-tool-permission");
    actions.append(&caption);

    // Allow options on top; Deny / "Stop asking" below a divider.
    let (allow, terminal): (Vec<&UserDecision>, Vec<&UserDecision>) =
        user_decisions.iter().partition(|d| !is_terminal(d));

    let mut tool_call_decisions: Vec<ToolCallDecision> = Vec::with_capacity(user_decisions.len());
    for decision in allow {
        let action = decision_button(decision, dispatcher.clone());
        actions.append(&action);
        tool_call_decisions.push(ToolCallDecision {
            action,
            decision: decision.clone(),
        });
    }
    if !tool_call_decisions.is_empty() && !terminal.is_empty() {
        actions.append(&Separator::new(Orientation::Horizontal));
    }
    for decision in terminal {
        let action = decision_button(decision, dispatcher.clone());
        actions.append(&action);
        tool_call_decisions.push(ToolCallDecision {
            action,
            decision: decision.clone(),
        });
    }

    (actions, tool_call_decisions)
}

fn decision_button(user_decision: &UserDecision, dispatcher: mpsc::UnboundedSender<Msg>) -> Button {
    let text = decision_label(user_decision);
    let label = Label::builder()
        .label(&text)
        .ellipsize(pango::EllipsizeMode::Middle)
        .build();
    let button = Button::builder()
        .child(&label)
        .can_focus(false)
        .focusable(false)
        .build();
    if matches!(user_decision, UserDecision::Allow { .. }) {
        button.set_tooltip_text(Some(&text));
    }
    button.add_css_class("paloma-chat-decision");
    match user_decision {
        UserDecision::AllowOnce { .. } => {
            button.add_css_class("suggested-action");
        },
        UserDecision::Deny { .. } => {
            button.add_css_class("flat");
            button.add_css_class("destructive-action");
        },
        UserDecision::IgnorePermission { .. } => {
            button.add_css_class("flat");
            button.add_css_class("paloma-chat-decision-warning");
        },
        _ => {
            button.add_css_class("flat");
        },
    }

    let decision = user_decision.clone();
    button.connect_clicked(move |button| {
        // disable all buttons
        if let Some(group) = button.parent() {
            group.set_sensitive(false);
        }
        let _ = dispatcher.unbounded_send(Msg::Chat(ChatMsg::ToolCallDecisionRequested(
            decision.clone(),
        )));
    });

    button
}

fn resolve_decision(actions: &GtkBox, state: &PermissionState) {
    actions.clear();
    let outcome = Label::builder()
        .label(decision_outcome(state))
        .xalign(0.0)
        .build();
    outcome.add_css_class("paloma-chat-tool-decision-outcome");
    actions.append(&outcome);
}

fn decision_label(decision: &UserDecision) -> String {
    match decision {
        UserDecision::AllowOnce { .. } => "Allow once".to_string(),
        UserDecision::Allow { command, glob, .. } => {
            if *glob {
                format!("Always allow {command} *")
            } else {
                format!("Always allow {command}")
            }
        },
        UserDecision::AllowSession { .. } => "Allow for this session".to_string(),
        UserDecision::IgnorePermission { .. } => "Stop asking this session".to_string(),
        UserDecision::Deny { .. } => "Deny".to_string(),
    }
}

fn is_terminal(decision: &UserDecision) -> bool {
    matches!(
        decision,
        UserDecision::Deny { .. } | UserDecision::IgnorePermission { .. }
    )
}

fn decision_outcome(state: &PermissionState) -> &'static str {
    match state {
        PermissionState::Allow => "Allowed",
        PermissionState::Deny => "Denied",
        PermissionState::Error => "Internal Error",
    }
}
