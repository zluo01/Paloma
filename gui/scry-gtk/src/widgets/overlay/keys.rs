//! Overlay keyboard dispatch.
//!
//! Transient UI wins first (action panel, then sessions popup); normal local
//! and chat handling follows. This module only dispatches over [`Overlay`]
//! state; it does not own any state itself.

use gtk4::{
    gdk::{Key, ModifierType},
    glib::Propagation,
};
use uuid::Uuid;

use super::{Mode, Overlay};
use crate::widgets::keymap::{self, BindingId, Context};

pub(super) enum KeyEvent {
    None,
    HideOverlay,
    ChatBackgrounded,
    StartChat(String),
    SubmitChat(String),
    SessionSelected(Uuid),
    SessionDeleted(Uuid),
}

pub(super) fn handle_key_press(
    overlay: &Overlay,
    key: Key,
    state: ModifierType,
) -> (Propagation, KeyEvent) {
    let shift = state.contains(ModifierType::SHIFT_MASK);

    if overlay.is_action_panel_open() {
        return handle_action_panel_key(overlay, key, state);
    }

    if overlay.is_sessions_open() {
        return handle_sessions_key(overlay, key, state);
    }

    // Shift+Right opens sessions from either mode, ahead of the mode split.
    if keymap::match_binding(Context::Global, key, state) == Some(BindingId::OpenSessions) {
        overlay.open_sessions();
        return (Propagation::Stop, KeyEvent::None);
    }

    // Ctrl+K opens the action panel, local mode only.
    if overlay.mode.get() == Mode::Local
        && keymap::match_binding(Context::Local, key, state) == Some(BindingId::SearchShowActions)
    {
        overlay.open_action_panel();
        return (Propagation::Stop, KeyEvent::None);
    }

    let ctx = match overlay.mode.get() {
        Mode::Local => Context::Local,
        Mode::Chat => Context::Chat,
    };
    match keymap::match_binding(ctx, key, state) {
        Some(BindingId::SearchClose) => (Propagation::Stop, KeyEvent::HideOverlay),
        Some(BindingId::ChatExit) => {
            overlay.exit_chat_mode();
            (Propagation::Stop, KeyEvent::ChatBackgrounded)
        },
        // Falls through (Proceed) when no pending decision consumes the arrow.
        Some(BindingId::ChatMovePrompt) => {
            if overlay.navigate_decisions(move_delta(key)) {
                (Propagation::Stop, KeyEvent::None)
            } else {
                (Propagation::Proceed, KeyEvent::None)
            }
        },
        // Falls through (Proceed) when there's no selection to move.
        Some(BindingId::SearchMove) => {
            if overlay.selection.borrow().is_empty() {
                (Propagation::Proceed, KeyEvent::None)
            } else {
                overlay.selection.borrow_mut().navigate(move_delta(key));
                overlay.scroll_selection_into_view();
                (Propagation::Stop, KeyEvent::None)
            }
        },
        // Shift+Return (or a consumed decision) is swallowed, not submitted.
        Some(BindingId::ChatSend) => {
            if !shift && !overlay.activate_selected_decision() {
                (
                    Propagation::Stop,
                    KeyEvent::SubmitChat(overlay.input_text()),
                )
            } else {
                (Propagation::Stop, KeyEvent::None)
            }
        },
        Some(BindingId::SearchOpen) => {
            if overlay.selection.borrow().is_empty() {
                (Propagation::Stop, KeyEvent::StartChat(overlay.input_text()))
            } else {
                overlay.activate_selection();
                (Propagation::Stop, KeyEvent::None)
            }
        },
        // `ChatInterrupt` effects (copy selection / interrupt stream) live in the
        // controller; when it doesn't consume the event, this path falls through.
        None | Some(BindingId::ChatInterrupt) => (Propagation::Proceed, KeyEvent::None),
        Some(other) => unreachable!("unhandled keymap binding in mode dispatch: {other:?}"),
    }
}

/// Navigation step for a move binding: `Up` goes up the list, `Down` down.
fn move_delta(key: Key) -> i32 {
    match key {
        Key::Up => -1,
        Key::Down => 1,
        _ => unreachable!("move bindings only declare Up/Down"),
    }
}

fn handle_action_panel_key(
    overlay: &Overlay,
    key: Key,
    state: ModifierType,
) -> (Propagation, KeyEvent) {
    match keymap::match_binding(Context::Panel, key, state) {
        Some(BindingId::PanelMove) => overlay.navigate_action_panel(move_delta(key)),
        Some(BindingId::PanelRun) => overlay.activate_action_panel(),
        Some(BindingId::PanelClose) => overlay.close_action_panel(),
        // Modal: swallow unbound keys so typing doesn't leak to the entry.
        None => {},
        Some(other) => unreachable!("match_binding(Context::Panel) returned {other:?}"),
    }
    (Propagation::Stop, KeyEvent::None)
}

fn handle_sessions_key(
    overlay: &Overlay,
    key: Key,
    state: ModifierType,
) -> (Propagation, KeyEvent) {
    let event = match keymap::match_binding(Context::Sessions, key, state) {
        Some(BindingId::SessionMove) => {
            overlay.sessions.navigate(move_delta(key));
            KeyEvent::None
        },
        Some(BindingId::SessionOpen) => overlay
            .sessions
            .activate_selected()
            .map_or(KeyEvent::None, KeyEvent::SessionSelected),
        Some(BindingId::SessionDelete) => overlay
            .sessions
            .selected_session()
            .map_or(KeyEvent::None, KeyEvent::SessionDeleted),
        Some(BindingId::SessionClose) => {
            overlay.close_sessions();
            KeyEvent::None
        },
        None => KeyEvent::None,
        Some(other) => unreachable!("match_binding(Context::Sessions) returned {other:?}"),
    };
    (Propagation::Stop, event)
}
