use std::rc::Rc;

use gtk4::{
    EventControllerKey, PropagationPhase,
    gdk::{Key, ModifierType},
    glib::Propagation,
    prelude::{EventControllerExt, WidgetExt},
};
use log::error;

use super::Overlay;
use crate::widgets::{
    keymap::{self, BindingId, Context},
    overlay::model::{Mode, Msg},
};

impl Overlay {
    pub(crate) fn register_key_binding(self: &Rc<Self>) {
        let controller = EventControllerKey::new();
        controller.set_propagation_phase(PropagationPhase::Capture);
        let overlay = Rc::downgrade(self);
        controller.connect_key_pressed(move |_, key, _, state| {
            let Some(overlay) = overlay.upgrade() else {
                return Propagation::Proceed;
            };
            overlay.handle_key_press(key, state)
        });
        self.launcher_window.add_controller(controller);
    }

    fn handle_key_press(&self, key: Key, state: ModifierType) -> Propagation {
        if self.is_sessions_open() {
            return self.handle_sessions_key(key, state);
        }

        if keymap::match_binding(Context::Global, key, state) == Some(BindingId::OpenSessions) {
            let _ = self.dispatcher.unbounded_send(Msg::ToggleSessionsRequested);
            return Propagation::Stop;
        }

        match self.current_mode() {
            Mode::Search => self.handle_search_view_key(key, state),
            Mode::Chat => self.handle_chat_view_key(key, state),
        }
    }

    fn handle_chat_view_key(&self, key: Key, state: ModifierType) -> Propagation {
        match keymap::match_binding(Context::Chat, key, state) {
            Some(BindingId::ChatExit) => {
                let _ = self.dispatcher.unbounded_send(Msg::ChatExitRequested);
            },
            Some(BindingId::ChatMovePrompt) => {
                if !self.chat.navigate(move_delta(key)) {
                    return Propagation::Proceed;
                }
            },
            Some(BindingId::ChatSend) => {
                if !self.chat.activate() {
                    let _ = self.dispatcher.unbounded_send(Msg::ChatPromptSubmitted);
                }
            },
            Some(BindingId::ChatInterrupt) => {
                if self.launcher.has_selection() {
                    return Propagation::Proceed;
                }
                if self.chat.copy_selection() {
                    return Propagation::Stop;
                }
                let _ = self.dispatcher.unbounded_send(Msg::ChatInterruptRequested);
            },
            None => return Propagation::Proceed,
            Some(other) => {
                error!("unknown binding for chat view {other:?}. This indicates a bug.")
            },
        }
        Propagation::Stop
    }

    fn handle_search_view_key(&self, key: Key, state: ModifierType) -> Propagation {
        match keymap::match_binding(Context::Search, key, state) {
            Some(BindingId::SearchClose) => {
                if !self.search.close_action_panel() {
                    let _ = self.dispatcher.unbounded_send(Msg::SearchExitRequest);
                }
            },
            Some(BindingId::SearchMove) => {
                if !self.search.navigate(move_delta(key)) {
                    return Propagation::Proceed;
                }
            },
            Some(BindingId::SearchSubmit) => {
                if !self.search.activate() && !self.render_any() {
                    // this is trigger if we do not select any action and no search result exists.
                    let _ = self.dispatcher.unbounded_send(Msg::ChatPromptSubmitted);
                }
            },
            Some(BindingId::SearchShowActions) => self.search.open_action_panel(),
            None => {
                if !self.search.is_action_panel_open() {
                    return Propagation::Proceed;
                }
            },
            Some(other) => {
                error!("unknown binding for result view {other:?}. This indicates a bug.")
            },
        }
        Propagation::Stop
    }

    fn handle_sessions_key(&self, key: Key, state: ModifierType) -> Propagation {
        match keymap::match_binding(Context::Sessions, key, state) {
            Some(BindingId::SessionMove) => self.sessions.navigate(move_delta(key)),
            Some(BindingId::SessionOpen) => {
                let _ = self.dispatcher.unbounded_send(Msg::SessionOpenRequested);
            },
            Some(BindingId::SessionDelete) => {
                let _ = self.dispatcher.unbounded_send(Msg::SessionDeleteRequested);
            },
            Some(BindingId::SessionClose) => {
                let _ = self
                    .dispatcher
                    .unbounded_send(Msg::SessionWindowCloseRequested);
            },
            None => {},
            Some(other) => {
                error!("unknown binding for session window {other:?}. This indicates a bug.")
            },
        };
        Propagation::Stop
    }
}

fn move_delta(key: Key) -> i32 {
    match key {
        Key::Up => -1,
        Key::Down => 1,
        _ => unreachable!("move bindings only declare Up/Down"),
    }
}
