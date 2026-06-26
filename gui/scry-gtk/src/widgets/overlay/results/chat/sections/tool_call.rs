use std::{rc::Rc, sync::Arc};

use gtk4::{
    Box as GtkBox, Button, Label, Orientation, Separator,
    prelude::{BoxExt, ButtonExt, WidgetExt},
};
use log::error;
use scry_core::{AppContext, PermissionState, UserDecision};

use crate::{
    helper::Clear,
    runtime,
    widgets::overlay::results::chat::helper::{append_content_label, code_card, new_section},
};

const TOOL_CLASS: &str = "scry-chat-section-tool";

pub(crate) struct ToolCallSection {
    view: GtkBox,
    decisions: Vec<Button>,
}

impl ToolCallSection {
    pub(crate) fn new(
        name: &str,
        arguments: &str,
        description: Option<&str>,
        decisions: &[UserDecision],
        app_context: Arc<AppContext>,
        on_finish: Rc<dyn Fn()>,
    ) -> Self {
        let view = new_section(None, TOOL_CLASS);
        if let Some(description) = description.filter(|d| !d.is_empty()) {
            append_content_label(&view, description, "scry-chat-tool-description");
        }
        view.append(&code_card(name, arguments));

        let decisions = if !decisions.is_empty() {
            let (decision_group, decisions) =
                decision_button_group(decisions, app_context, on_finish);
            view.append(&decision_group);
            decisions
        } else {
            vec![]
        };

        Self { view, decisions }
    }

    pub(crate) fn widgets(&self) -> (&GtkBox, &[Button]) {
        (&self.view, &self.decisions)
    }
}
fn decision_button_group(
    user_decisions: &[UserDecision],
    app_context: Arc<AppContext>,
    on_finish: Rc<dyn Fn()>,
) -> (GtkBox, Vec<Button>) {
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

    let mut buttons = Vec::with_capacity(user_decisions.len());
    for decision in allow {
        let button = decision_button(decision, &actions, app_context.clone(), on_finish.clone());
        actions.append(&button);
        buttons.push(button);
    }
    if !buttons.is_empty() && !terminal.is_empty() {
        actions.append(&Separator::new(Orientation::Horizontal));
    }
    for decision in terminal {
        let button = decision_button(decision, &actions, app_context.clone(), on_finish.clone());
        actions.append(&button);
        buttons.push(button);
    }

    (actions, buttons)
}

fn decision_button(
    user_decision: &UserDecision,
    parent: &GtkBox,
    app_context: Arc<AppContext>,
    on_finish: Rc<dyn Fn()>,
) -> Button {
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

    let decision = user_decision.clone();
    let app_context = app_context.clone();
    let actions_for_click = parent.clone();
    button.connect_clicked(move |_| {
        let actions = actions_for_click.clone();
        let app_context = app_context.clone();
        let decision = decision.clone();
        let on_finish = on_finish.clone();
        runtime::spawn_with(
            async move { app_context.decide_toolcall_permissions(decision).await },
            move |result| {
                let state = result.unwrap_or_else(|error| {
                    error!("decide: {error}");
                    PermissionState::Error
                });
                on_finish();
                resolve_decision(&actions, &state);
            },
        );
    });
    button
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
