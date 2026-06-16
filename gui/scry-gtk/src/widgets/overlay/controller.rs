//! App-side coordinator for the overlay.
//!
//! The overlay owns GTK widgets and local UI state; this controller owns
//! application behavior around it: subscriptions, controller calls, chat
//! session lifecycle, and settings/model actions.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use gtk4::{
    gdk::{Key, ModifierType},
    glib::{self, Propagation},
};
use libadwaita::Application;
use log::{error, warn};
use scry_core::{
    Action, ActionOutcome, AppContext, ChatRenderEvent, Connector, LocalRenderEvent, ProviderId,
    RenderEvent, SessionUpdate, TerminalState, UserDecision,
};
use tokio::sync::{broadcast::error::RecvError, mpsc};
use uuid::Uuid;

use super::{
    InvokeFn, OnDecideFn, Overlay,
    bar::ModelChoice,
    connectors::{is_preferred, is_running},
    keys::{self, KeyEvent},
};
use crate::runtime;

#[derive(Clone)]
pub(crate) struct OverlayController {
    inner: Rc<ControllerInner>,
}

struct ControllerInner {
    overlay: Overlay,
    app: Arc<AppContext>,
    open_settings: Rc<dyn Fn()>,
    chat: ChatState,
    latest_query_id: Cell<u64>,
    selected_provider: Cell<ProviderId>,
}

#[derive(Default)]
struct ChatState {
    active_session: RefCell<Option<Uuid>>,
    in_flight: Cell<bool>,
}

impl OverlayController {
    pub(crate) fn new(
        gapp: &Application,
        app: Arc<AppContext>,
        open_settings: Rc<dyn Fn()>,
    ) -> Self {
        let controller = Self {
            inner: Rc::new(ControllerInner {
                overlay: super::build(gapp),
                app,
                open_settings,
                chat: ChatState::default(),
                latest_query_id: Cell::new(0),
                selected_provider: Cell::new(ProviderId::Codex),
            }),
        };
        controller.install();
        controller
    }

    fn install(&self) {
        self.install_hotkey_watcher();
        self.install_query_handler();
        self.install_key_handler();
        self.install_bar_actions();
        self.install_model_picker();
        self.install_session_update_watcher();
        self.refresh_sessions_panel();
    }

    fn install_hotkey_watcher(&self) {
        let mut rx = self.inner.app.hotkey.subscribe();
        let controller = self.clone();
        glib::spawn_future_local(async move {
            loop {
                match rx.recv().await {
                    Ok(()) => controller.toggle_overlay(),
                    Err(RecvError::Lagged(n)) => warn!("hotkey: lagged by {n} events"),
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    fn install_query_handler(&self) {
        let controller = self.clone();
        self.inner.overlay.connect_search_changed(move |text| {
            controller.handle_query_changed(text);
        });
    }

    fn install_key_handler(&self) {
        let overlay = self.inner.overlay.clone();
        let controller = self.clone();
        self.inner.overlay.connect_key_pressed(move |key, state| {
            // Ctrl+C interrupts an in-flight chat turn, unless there's a
            // selection to copy (let it fall through to the entry's copy).
            if key == Key::c
                && state.contains(ModifierType::CONTROL_MASK)
                && overlay.is_chat_mode()
                && controller.inner.chat.in_flight.get()
                && !overlay.entry_has_selection()
            {
                controller.cancel_active_chat();
                return Propagation::Stop;
            }
            let (propagation, event) = keys::handle_key_press(&overlay, key, state);
            controller.handle_key_event(event);
            propagation
        });
    }

    fn install_bar_actions(&self) {
        let controller = self.clone();
        self.inner.overlay.connect_settings_clicked(move || {
            controller.hide_overlay();
            (controller.inner.open_settings)();
        });

        let controller = self.clone();
        self.inner.overlay.connect_sessions_clicked(move || {
            controller.inner.overlay.toggle_sessions();
        });
    }

    fn install_model_picker(&self) {
        let controller = self.clone();
        self.inner.overlay.connect_model_selected(move |choice| {
            controller.select_model(choice);
        });
    }

    fn install_session_update_watcher(&self) {
        let mut rx = self.inner.app.remote_query.subscribe();
        let controller = self.clone();
        let app = self.inner.app.clone();

        glib::spawn_future_local(async move {
            // Shared permission-decision handler for every tool-call section this
            // watcher renders.
            let on_decide: OnDecideFn = Rc::new(move |decision: UserDecision, apply| {
                let app = app.clone();
                runtime::spawn_with(
                    async move { app.remote_query.decide(decision).await },
                    move |result| match result {
                        Ok(state) => apply(state),
                        Err(err) => error!("decide: {err}"),
                    },
                );
            });

            loop {
                match rx.recv().await {
                    Ok(SessionUpdate { session_id, event }) => {
                        if Some(session_id) != *controller.inner.chat.active_session.borrow() {
                            continue;
                        }
                        controller.render_session_event(event, on_decide.clone());
                    },
                    Err(RecvError::Lagged(n)) => {
                        warn!("session update: lagged by {n} events");
                    },
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    fn handle_key_event(&self, event: KeyEvent) {
        match event {
            KeyEvent::None => {},
            KeyEvent::HideOverlay => self.hide_overlay(),
            KeyEvent::ChatBackgrounded => self.background_chat(),
            KeyEvent::StartChat(prompt) => {
                self.submit_chat(prompt, true);
            },
            KeyEvent::SubmitChat(prompt) => {
                self.submit_chat(prompt, false);
            },
            KeyEvent::SessionSelected(id) => self.restore_session(id),
            KeyEvent::SessionDeleted(id) => self.delete_session(id),
        }
    }

    fn toggle_overlay(&self) {
        if self.inner.overlay.is_visible() {
            self.hide_overlay();
        } else {
            self.show_overlay();
        }
    }

    fn show_overlay(&self) {
        if self.inner.overlay.show() {
            self.refresh_health();
            self.refresh_model_picker();
        }
    }

    fn hide_overlay(&self) {
        if self.inner.overlay.hide() {
            self.background_chat();
        }
    }

    fn background_chat(&self) {
        self.inner.chat.active_session.borrow_mut().take();
        self.inner.overlay.set_active_session(None);
        self.inner.chat.in_flight.set(false);
    }

    /// Cancel the in-flight turn on the active session. The UI updates from
    /// the resulting `RenderEvent::Cancel` broadcast, not here.
    fn cancel_active_chat(&self) {
        let Some(session_id) = *self.inner.chat.active_session.borrow() else {
            return;
        };
        let app = self.inner.app.clone();
        runtime::spawn_with(
            async move { app.remote_query.cancel(session_id).await },
            |result| {
                if let Err(err) = result {
                    error!("cancel chat: {err}");
                }
            },
        );
    }

    fn handle_query_changed(&self, text: String) {
        if self.inner.overlay.is_chat_mode() {
            return;
        }

        self.inner
            .latest_query_id
            .set(self.inner.latest_query_id.get().wrapping_add(1));
        let query_id = self.inner.latest_query_id.get();

        if text.is_empty() {
            self.inner.overlay.clear_results();
            return;
        }

        let app_for_query = self.inner.app.clone();
        let (render_tx, mut render_rx) =
            mpsc::channel::<RenderEvent>(scry_core::RENDER_CHANNEL_CAPACITY);

        self.inner.overlay.clear_results();

        runtime::spawn_with(
            async move {
                app_for_query.local_query.query(&text, render_tx).await;
            },
            |_| {},
        );

        let controller = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let on_invoke: InvokeFn = {
                let controller = controller.clone();
                Rc::new(move |handler_id, action| {
                    controller.invoke_action(handler_id, action);
                })
            };

            let mut has_results = false;
            while let Some(event) = render_rx.recv().await {
                if query_id != controller.inner.latest_query_id.get() {
                    break;
                }

                match event {
                    RenderEvent::Local(LocalRenderEvent::Append { response }) => {
                        has_results |= !response.items.is_empty();
                        controller.inner.overlay.append_section(
                            response.id,
                            &response.name,
                            response.items,
                            on_invoke.clone(),
                        );
                    },
                    RenderEvent::Done => {
                        if has_results {
                            let overlay = controller.inner.overlay.clone();
                            let chat_controller = controller.clone();
                            overlay.append_chat_action(Rc::new(move || {
                                chat_controller.start_chat_from_current_input();
                            }));
                        }
                        break;
                    },
                    RenderEvent::Error { message } => {
                        warn!("query render: {message}");
                        break;
                    },
                    RenderEvent::Chat(_) | RenderEvent::Cancel => {
                        warn!("query render: unexpected chat event on local-query channel");
                    },
                }
            }
        });
    }

    fn start_chat_from_current_input(&self) {
        self.submit_chat(self.inner.overlay.input_text(), true);
    }

    fn submit_chat(&self, prompt: String, enter_chat: bool) -> bool {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return false;
        }
        if self.inner.chat.in_flight.replace(true) {
            return false;
        }

        if enter_chat {
            self.inner.overlay.enter_chat_mode();
        }
        self.inner.overlay.clear_input();

        let prior_session = *self.inner.chat.active_session.borrow();
        let provider = self.inner.selected_provider.get();
        let controller = self.clone();

        glib::MainContext::default().spawn_local(async move {
            let init = {
                let app = controller.inner.app.clone();
                let prompt = prompt.clone();
                runtime::spawn(async move {
                    app.remote_query
                        .init_chat(prior_session, provider, prompt)
                        .await
                })
                .await
            };

            let (session_id, is_new) = match init {
                Ok((id, is_new)) => (id, is_new),
                Err(err) => {
                    controller.inner.chat.in_flight.set(false);
                    error!("chat init: {err}");
                    return;
                },
            };

            *controller.inner.chat.active_session.borrow_mut() = Some(session_id);
            controller
                .inner
                .overlay
                .set_active_session(Some(session_id));

            if is_new {
                controller.refresh_sessions_panel();
            }

            let chat_result = {
                let app = controller.inner.app.clone();
                let prompt = prompt.clone();
                runtime::spawn(
                    async move { app.remote_query.chat(session_id, provider, prompt).await },
                )
                .await
            };

            if let Err(err) = chat_result {
                error!("chat: {err}");
                controller.inner.chat.in_flight.set(false);

                if is_new {
                    let app = controller.inner.app.clone();
                    runtime::spawn(async move {
                        app.remote_query.cleanup(session_id).await;
                    })
                    .await;
                    controller.inner.chat.active_session.borrow_mut().take();
                    controller.refresh_sessions_panel();
                    controller.inner.overlay.set_active_session(None);
                }
            }
        });

        true
    }

    fn restore_session(&self, id: Uuid) {
        if *self.inner.chat.active_session.borrow() == Some(id) {
            return;
        }

        *self.inner.chat.active_session.borrow_mut() = Some(id);
        self.inner.overlay.set_active_session(Some(id));
        self.inner.overlay.enter_chat_for_restore();
        self.inner.chat.in_flight.set(false);

        let controller = self.clone();
        runtime::spawn_with(
            {
                let app = self.inner.app.clone();
                async move { app.remote_query.restore_session(id).await }
            },
            move |result| match result {
                Ok(TerminalState::Running) => {
                    controller.inner.chat.in_flight.set(true);
                },
                Ok(TerminalState::Done | TerminalState::Error | TerminalState::Cancel) => {
                    controller.inner.overlay.finish_chat_turn();
                    controller.inner.chat.in_flight.set(false);
                },
                Err(err) => error!("restore session {id}: {err}"),
            },
        );
    }

    fn render_session_event(&self, event: RenderEvent, on_decide: OnDecideFn) {
        match event {
            RenderEvent::Chat(ChatRenderEvent::UserPrompt { text }) => {
                self.inner.overlay.start_chat_turn(&text);
            },
            RenderEvent::Chat(ChatRenderEvent::TextDelta { text }) => {
                self.inner.overlay.append_chat_text(&text);
            },
            RenderEvent::Chat(ChatRenderEvent::ReasoningDelta { text }) => {
                self.inner.overlay.append_chat_reasoning(&text);
            },
            RenderEvent::Chat(ChatRenderEvent::ToolCall {
                name,
                arguments,
                description,
                decisions,
            }) => {
                self.inner.overlay.add_chat_tool_call(
                    &name,
                    &arguments,
                    description.as_deref(),
                    &decisions,
                    on_decide,
                );
            },
            RenderEvent::Done => {
                self.inner.overlay.finish_chat_turn();
                self.inner.chat.in_flight.set(false);
            },
            RenderEvent::Error { message } => {
                warn!("chat render: {message}");
                self.inner.overlay.fail_chat_turn(&message);
                self.inner.chat.in_flight.set(false);
                self.refresh_sessions_panel();
            },
            RenderEvent::Cancel => {
                self.inner.overlay.cancel_chat_turn();
                self.inner.chat.in_flight.set(false);
                self.refresh_sessions_panel();
            },
            RenderEvent::Local(_) => {
                warn!("session update: unexpected local event on session channel");
            },
        }
    }

    fn invoke_action(&self, handler_id: String, action: Action) {
        let Some(outcome) = self.inner.app.local_query.run(handler_id, action) else {
            return;
        };
        match outcome {
            ActionOutcome::Hide => self.hide_overlay(),
            ActionOutcome::Stay => {},
            ActionOutcome::Replace { input } => self.inner.overlay.set_input(&input),
        }
    }

    fn refresh_sessions_panel(&self) {
        let controller = self.clone();
        runtime::spawn_with(
            {
                let app = self.inner.app.clone();
                async move { app.remote_query.available_sessions().await }
            },
            move |result| match result {
                Ok(sessions) => {
                    let formatted: Vec<(Uuid, String, String)> = sessions
                        .into_iter()
                        .map(|listing| {
                            (
                                listing.session_id,
                                listing.provider_id.as_str().to_string(),
                                listing.title,
                            )
                        })
                        .collect();
                    // A failed first turn drops its session backend-side; if our
                    // active one is gone, reset so the next prompt opens a fresh one.
                    let active = *controller.inner.chat.active_session.borrow();
                    if let Some(active) = active
                        && !formatted.iter().any(|(id, _, _)| *id == active)
                    {
                        controller.inner.chat.active_session.borrow_mut().take();
                        controller.inner.overlay.set_active_session(None);
                    }
                    let on_selected: Rc<dyn Fn(Uuid)> = {
                        let controller = controller.clone();
                        Rc::new(move |id| controller.restore_session(id))
                    };
                    let on_delete: Rc<dyn Fn(Uuid)> = {
                        let controller = controller.clone();
                        Rc::new(move |id| controller.delete_session(id))
                    };
                    controller
                        .inner
                        .overlay
                        .set_sessions(&formatted, on_selected, on_delete);
                },
                Err(err) => warn!("refresh sessions: {err}"),
            },
        );
    }

    fn delete_session(&self, id: Uuid) {
        let controller = self.clone();
        runtime::spawn_with(
            {
                let app = self.inner.app.clone();
                async move { app.remote_query.remove_session(id).await }
            },
            move |result| {
                if let Err(err) = result {
                    error!("delete session {id}: {err}. This indicates a bug.");
                }
                controller.refresh_sessions_panel();
            },
        );
    }

    fn refresh_health(&self) {
        let overlay = self.inner.overlay.clone();
        runtime::spawn_with(
            {
                let app = self.inner.app.clone();
                async move {
                    let models = app.connect.health_level().await;
                    let plugins = app.plugin.health_level().await;
                    (models, plugins)
                }
            },
            move |(models, plugins)| overlay.set_health(models, plugins),
        );
    }

    fn refresh_model_picker(&self) {
        let controller = self.clone();
        runtime::spawn_with(
            {
                let app = self.inner.app.clone();
                async move { app.connect.available_connectors().await }
            },
            move |result| match result {
                Ok(connectors) => {
                    if let Some(provider) = preferred_provider(&connectors) {
                        controller.inner.selected_provider.set(provider);
                    }
                    controller.inner.overlay.set_model_options(&connectors);
                },
                Err(e) => warn!("model picker refresh: {e}"),
            },
        );
    }

    fn select_model(&self, choice: ModelChoice) {
        self.inner.selected_provider.set(choice.provider);

        let controller = self.clone();
        runtime::spawn_with(
            {
                let app = self.inner.app.clone();
                async move {
                    app.connect
                        .set_preferred(choice.provider, &choice.model, &choice.effort)
                        .await
                }
            },
            move |result| match result {
                Ok(()) => controller.refresh_model_picker(),
                Err(e) => warn!("set preferred model: {e}"),
            },
        );
    }
}

/// The provider to drive chat with: preferred-and-running, else the first
/// running one.
fn preferred_provider(connectors: &[Connector]) -> Option<ProviderId> {
    connectors
        .iter()
        .find(|c| is_preferred(c) && is_running(c))
        .or_else(|| connectors.iter().find(|c| is_running(c)))
        .map(|c| c.id)
}
