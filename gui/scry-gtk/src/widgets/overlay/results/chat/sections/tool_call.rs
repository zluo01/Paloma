use gtk4::{
    Box as GtkBox, Button, Label, Orientation, Separator,
    prelude::{BoxExt, WidgetExt},
};
use scry_core::{PermissionState, UserDecision};

use crate::{
    helper::Clear,
    widgets::overlay::results::chat::helper::{append_content_label, code_card, new_section},
};

const TOOL_CLASS: &str = "scry-chat-section-tool";

pub(crate) struct ToolCallDecision {
    pub action: Button,
    pub decision: UserDecision,
}

#[derive(Clone)]
pub(crate) struct ToolCallSection {
    view: GtkBox,
}

impl ToolCallSection {
    pub(crate) fn new(
        name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
    ) -> (Self, Vec<ToolCallDecision>) {
        let view = new_section(None, TOOL_CLASS);
        if let Some(description) = description.filter(|d| !d.is_empty()) {
            append_content_label(&view, description, "scry-chat-tool-description");
        }
        view.append(&code_card(name, arguments));

        let decisions = if !decisions.is_empty() {
            let (decision_group, decisions) = decision_button_group(decisions);
            view.append(&decision_group);
            decisions
        } else {
            vec![]
        };

        (Self { view }, decisions)
    }

    pub(crate) fn widgets(&self) -> &GtkBox {
        &self.view
    }

    pub(crate) fn on_finish(&self, permission_state: &PermissionState) {
        resolve_decision(&self.view, permission_state)
    }
}

fn decision_button_group(user_decisions: &[UserDecision]) -> (GtkBox, Vec<ToolCallDecision>) {
    let actions = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .build();
    actions.add_css_class("scry-chat-tool-decisions");

    let caption = Label::builder()
        .label("Needs your permission")
        .xalign(0.0)
        .build();
    caption.add_css_class("scry-chat-tool-permission");
    actions.append(&caption);

    // Allow options on top; Deny / "Stop asking" below a divider.
    let (allow, terminal): (Vec<&UserDecision>, Vec<&UserDecision>) =
        user_decisions.iter().partition(|d| !is_terminal(d));

    let mut tool_call_decisions: Vec<ToolCallDecision> = Vec::with_capacity(user_decisions.len());
    for decision in allow {
        let decision = decision_button(decision);
        actions.append(&decision.action);
        tool_call_decisions.push(decision);
    }
    if !tool_call_decisions.is_empty() && !terminal.is_empty() {
        actions.append(&Separator::new(Orientation::Horizontal));
    }
    for decision in terminal {
        let decision = decision_button(decision);
        actions.append(&decision.action);
        tool_call_decisions.push(decision);
    }

    (actions, tool_call_decisions)
}

fn decision_button(user_decision: &UserDecision) -> ToolCallDecision {
    let button = Button::builder()
        .label(decision_label(user_decision))
        .can_focus(false)
        .focusable(false)
        .build();
    button.add_css_class("scry-chat-decision");
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
            button.add_css_class("scry-chat-decision-warning");
        },
        _ => {
            button.add_css_class("flat");
        },
    }

    ToolCallDecision {
        action: button,
        decision: user_decision.clone(),
    }
}

fn resolve_decision(actions: &GtkBox, state: &PermissionState) {
    actions.clear();
    let outcome = Label::builder()
        .label(decision_outcome(state))
        .xalign(0.0)
        .build();
    outcome.add_css_class("scry-chat-tool-decision-outcome");
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
