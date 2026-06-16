//! Overlay keyboard dispatch, in priority order:
//!
//! 1. Sessions popup open — arrows navigate, Enter activates,
//!    Esc/Shift+Left closes.
//! 2. Shift+Right — opens the sessions popup.
//! 3. Otherwise — Escape collapses actions / exits chat / hides the
//!    overlay; arrows move the selection (chat: the pending permission
//!    prompts); Enter confirms the highlighted permission or submits
//!    (chat), activates the selected row / starts a chat (local).
//!
//! Pure dispatch over [`Overlay`] state; nothing here owns state.

use gtk4::{
    gdk::{Key, ModifierType},
    glib::Propagation,
};
use uuid::Uuid;

use super::{Mode, Overlay};

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

    if overlay.sessions.is_open() {
        return handle_sessions_key(overlay, key, shift);
    }

    if shift && key == Key::Right {
        overlay.sessions.open();
        return (Propagation::Stop, KeyEvent::None);
    }

    match key {
        Key::Escape => {
            if overlay.selection.borrow_mut().collapse_action() {
                return (Propagation::Stop, KeyEvent::None);
            }
            if overlay.mode.get() == Mode::Chat {
                overlay.exit_chat_mode();
                return (Propagation::Stop, KeyEvent::ChatBackgrounded);
            }
            (Propagation::Stop, KeyEvent::HideOverlay)
        },
        Key::Up | Key::Down if overlay.mode.get() == Mode::Chat => {
            let delta = if key == Key::Up { -1 } else { 1 };
            if overlay.navigate_decisions(delta) {
                (Propagation::Stop, KeyEvent::None)
            } else {
                (Propagation::Proceed, KeyEvent::None)
            }
        },
        Key::Up | Key::Down if overlay.selection.borrow().is_empty() => {
            (Propagation::Proceed, KeyEvent::None)
        },
        Key::Up => {
            overlay.selection.borrow_mut().navigate(-1);
            overlay.scroll_selection_into_view();
            (Propagation::Stop, KeyEvent::None)
        },
        Key::Down => {
            overlay.selection.borrow_mut().navigate(1);
            overlay.scroll_selection_into_view();
            (Propagation::Stop, KeyEvent::None)
        },
        Key::Return | Key::KP_Enter => {
            let event = match overlay.mode.get() {
                Mode::Chat if !shift && !overlay.activate_selected_decision() => {
                    KeyEvent::SubmitChat(overlay.input_text())
                },
                Mode::Chat => KeyEvent::None,
                Mode::Local if !overlay.selection.borrow().is_empty() => {
                    overlay.activate_selection();
                    KeyEvent::None
                },
                Mode::Local => KeyEvent::StartChat(overlay.input_text()),
            };
            (Propagation::Stop, event)
        },
        _ => (Propagation::Proceed, KeyEvent::None),
    }
}

fn handle_sessions_key(overlay: &Overlay, key: Key, shift: bool) -> (Propagation, KeyEvent) {
    let event = match key {
        Key::Escape => {
            overlay.sessions.close();
            KeyEvent::None
        },
        Key::Left if shift => {
            overlay.sessions.close();
            KeyEvent::None
        },
        Key::Up => {
            overlay.sessions.navigate(-1);
            KeyEvent::None
        },
        Key::Down => {
            overlay.sessions.navigate(1);
            KeyEvent::None
        },
        Key::Delete | Key::KP_Delete => overlay
            .sessions
            .selected_session()
            .map_or(KeyEvent::None, KeyEvent::SessionDeleted),
        Key::Return | Key::KP_Enter => overlay
            .sessions
            .activate_selected()
            .map_or(KeyEvent::None, KeyEvent::SessionSelected),
        _ => KeyEvent::None,
    };
    (Propagation::Stop, event)
}
