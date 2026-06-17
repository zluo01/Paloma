//! Keyboard navigation over pending permission prompts.
//!
//! Pending tool calls contribute button groups to one flat Up/Down sequence.
//! Buttons are not GTK-focusable; selection is the shared `selected` CSS class,
//! and Enter emits `clicked` so keyboard and pointer activation share a path.

use std::{cell::RefCell, rc::Rc};

use gtk4::{Box as GtkBox, Button, Label, Orientation, Separator, prelude::*};
use scry_core::{PermissionState, UserDecision};

use super::super::{OnDecideFn, SELECTED_CLASS};
use crate::widgets::clear_children;

#[derive(Default)]
pub(super) struct PendingDecisions {
    groups: Vec<Group>,
    /// `(group, button)` of the highlighted option.
    selected: Option<(usize, usize)>,
}

/// One tool call's unresolved decision buttons; `container` is the
/// identity key used to drop the group once a decision resolves.
struct Group {
    container: GtkBox,
    buttons: Vec<Button>,
}

impl PendingDecisions {
    /// Register a tool call's decision buttons. The first prompt to
    /// arrive gets the highlight, so Enter confirms it immediately.
    pub(super) fn add_group(&mut self, container: GtkBox, buttons: Vec<Button>) {
        if buttons.is_empty() {
            return;
        }
        self.groups.push(Group { container, buttons });
        if self.selected.is_none() {
            self.set_selected(Some((self.groups.len() - 1, 0)));
        }
    }

    /// Drop a resolved group. If it held the highlight, the nearest
    /// remaining prompt inherits it.
    pub(super) fn resolve(&mut self, container: &GtkBox) {
        let Some(idx) = self.groups.iter().position(|g| &g.container == container) else {
            return;
        };
        self.groups.remove(idx);
        match self.selected {
            // Earlier group: indices unaffected, highlight stays put.
            Some((g, _)) if g < idx => {},
            // Later group: shift its index down past the removal.
            Some((g, b)) if g > idx => self.selected = Some((g - 1, b)),
            // The resolved group itself (its widgets are gone, so no
            // class cleanup): re-highlight the nearest survivor.
            Some(_) => {
                self.selected = None;
                if !self.groups.is_empty() {
                    self.set_selected(Some((idx.min(self.groups.len() - 1), 0)));
                }
            },
            None => {},
        }
    }

    /// Move the highlight by `delta` through the flattened option
    /// list, clamped at the ends. Returns false when nothing is
    /// pending; the caller should let the key fall through.
    pub(super) fn navigate(&mut self, delta: i32) -> bool {
        let flat: Vec<(usize, usize)> = self
            .groups
            .iter()
            .enumerate()
            .flat_map(|(g, group)| (0..group.buttons.len()).map(move |b| (g, b)))
            .collect();
        if flat.is_empty() {
            return false;
        }
        let current = self
            .selected
            .and_then(|sel| flat.iter().position(|&pos| pos == sel))
            .unwrap_or(0);
        let next = (current as i32 + delta).clamp(0, flat.len() as i32 - 1) as usize;
        self.set_selected(Some(flat[next]));
        true
    }

    pub(super) fn selected_button(&self) -> Option<Button> {
        let (g, b) = self.selected?;
        Some(self.groups[g].buttons[b].clone())
    }

    pub(super) fn clear(&mut self) {
        self.groups.clear();
        self.selected = None;
    }

    fn set_selected(&mut self, target: Option<(usize, usize)>) {
        if let Some((g, b)) = self.selected
            && let Some(button) = self.groups.get(g).and_then(|grp| grp.buttons.get(b))
        {
            button.remove_css_class(SELECTED_CLASS);
        }
        self.selected = target;
        if let Some((g, b)) = target {
            self.groups[g].buttons[b].add_css_class(SELECTED_CLASS);
        }
    }
}

/// A caption over a vertical stack of decision buttons, registered for
/// arrow-key navigation. A click (or Enter on the highlight) resolves the
/// prompt and swaps the buttons for the outcome label.
pub(super) fn decision_prompt(
    decisions: &[UserDecision],
    on_decide: &OnDecideFn,
    pending: &Rc<RefCell<PendingDecisions>>,
) -> GtkBox {
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

    // One emphasized primary (Allow once); every other option is flat, with
    // color carrying its meaning.
    let make_button = |decision: &UserDecision| -> Button {
        let button = Button::with_label(&decision_label(decision));
        button.add_css_class("scry-chat-decision");
        match decision {
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
        // The highlight is the shared `selected` class, never GTK focus.
        button.set_focusable(false);
        button.set_can_focus(false);

        let on_decide = on_decide.clone();
        let decision = decision.clone();
        let actions_for_click = actions.clone();
        let pending_for_click = pending.clone();
        button.connect_clicked(move |_| {
            let actions = actions_for_click.clone();
            let pending = pending_for_click.clone();
            on_decide(
                decision.clone(),
                Box::new(move |state| {
                    pending.borrow_mut().resolve(&actions);
                    resolve_decision(&actions, &state);
                }),
            );
        });
        button
    };

    // Allow options on top; Deny / "Stop asking" below a divider.
    let (allow, terminal): (Vec<&UserDecision>, Vec<&UserDecision>) =
        decisions.iter().partition(|d| !is_terminal(d));

    let mut buttons = Vec::with_capacity(decisions.len());
    for decision in allow {
        let button = make_button(decision);
        actions.append(&button);
        buttons.push(button);
    }
    if !buttons.is_empty() && !terminal.is_empty() {
        actions.append(&Separator::new(Orientation::Horizontal));
    }
    for decision in terminal {
        let button = make_button(decision);
        actions.append(&button);
        buttons.push(button);
    }

    pending.borrow_mut().add_group(actions.clone(), buttons);
    actions
}

/// Terminal choices (refuse, or stop being asked) shown below the divider,
/// apart from the routine allow-this-request options.
fn is_terminal(decision: &UserDecision) -> bool {
    matches!(
        decision,
        UserDecision::Deny { .. } | UserDecision::IgnorePermission { .. }
    )
}

/// Button text for a permission decision offered under a tool call.
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

/// Swap a tool call's decision buttons for the resolved outcome label.
fn resolve_decision(actions: &GtkBox, state: &PermissionState) {
    clear_children(actions);
    let outcome = Label::builder()
        .label(decision_outcome(state))
        .xalign(0.0)
        .build();
    outcome.add_css_class("scry-chat-tool-decision-outcome");
    actions.append(&outcome);
}

fn decision_outcome(state: &PermissionState) -> &'static str {
    match state {
        PermissionState::Allow => "Allowed",
        PermissionState::Deny => "Denied",
        PermissionState::Error => "Internal Error",
    }
}
