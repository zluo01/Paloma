use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use futures::{StreamExt, channel::mpsc};
use gtk4::{
    ApplicationWindow, Overflow, PolicyType, ScrolledWindow, Stack, StackTransitionType,
    gdk::Monitor, glib, prelude::*,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::Application;
use log::{error, warn};
use tokio::sync::{broadcast, broadcast::error::RecvError};
use uuid::Uuid;

mod keys;
mod launcher;
mod model;
mod results;
mod window;

use scry_core::{
    Action, ActionOutcome, AppContext, ChatRenderEvent, ProviderId, RenderEvent, SearchRenderEvent,
};

use crate::{
    runtime,
    widgets::overlay::{
        launcher::LauncherView,
        model::{Command, Mode, Model, Msg},
        results::{ChatView, SearchView, SessionsView},
    },
};

const SEARCH_BAR_HEIGHT_PX: i32 = 94;
const OVERLAY_WIDTH_PX: i32 = 640;
const OVERLAY_CONTENT_HEIGHT_PX: i32 = 420;
const PANEL_GAP_PX: i32 = 8;

const SEARCH_VIEW_KEY: &str = "searches";
const CHAT_VIEW_KEY: &str = "chats";
const SESSION_VIEW_KEY: &str = "sessions";

const SELECTED_CLASS: &str = "selected";

const CSS: &str = include_str!("style.css");

/// Overlay stylesheet fragments loaded into the global GTK provider.
pub(crate) const CSS_PARTS: &[&str] = &[CSS, launcher::CSS, results::CSS];

pub(crate) fn new(
    app: &Application,
    app_context: Arc<AppContext>,
    hotkey: broadcast::Sender<()>,
) -> Rc<Overlay> {
    Overlay::new(app, app_context, hotkey)
}

pub(crate) struct Overlay {
    gapp: Application,
    launcher_window: ApplicationWindow,
    content_window: ApplicationWindow,
    scroller: ScrolledWindow,
    content_stack: Stack,
    launcher: LauncherView,
    search: SearchView,
    chat: ChatView,
    sessions: SessionsView,
    app_context: Arc<AppContext>,
    dispatcher: mpsc::UnboundedSender<Msg>,
    model: RefCell<Model>,
    /// Chat auto-scroll stays pinned until the user scrolls away.
    stuck_to_bottom: Cell<bool>,
    /// Bar top-left in monitor pixels; `None` until first show.
    position: Cell<Option<(i32, i32)>>,
    /// Monitor that `position` is relative to.
    monitor: RefCell<Option<Monitor>>,
}

impl Overlay {
    fn new(
        app: &Application,
        app_context: Arc<AppContext>,
        hotkey: broadcast::Sender<()>,
    ) -> Rc<Self> {
        let launcher_window = layer_window(
            app,
            "scry-launcher",
            OVERLAY_WIDTH_PX,
            KeyboardMode::Exclusive,
        );
        launcher_window.set_title(Some("Scry"));

        let (dispatcher, mut receiver) = mpsc::unbounded::<Msg>();

        let launcher = LauncherView::new(app_context.clone(), dispatcher.clone());
        launcher_window.set_child(Some(launcher.widget()));

        let content_stack = Stack::builder()
            .transition_type(StackTransitionType::None)
            .vhomogeneous(false)
            .build();

        let search = SearchView::new(dispatcher.clone());
        let chat = ChatView::new(app_context.clone());
        let sessions = SessionsView::new(app_context.clone(), dispatcher.clone());

        content_stack.add_named(search.widget(), Some(SEARCH_VIEW_KEY));
        content_stack.add_named(chat.widget(), Some(CHAT_VIEW_KEY));
        content_stack.add_named(sessions.widget(), Some(SESSION_VIEW_KEY));

        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            // Hug the content height, capped so long output scrolls.
            .propagate_natural_height(true)
            .max_content_height(OVERLAY_CONTENT_HEIGHT_PX)
            .width_request(OVERLAY_WIDTH_PX)
            // Avoid competing with TextView drag selection in chat output.
            .kinetic_scrolling(false)
            .overflow(Overflow::Hidden)
            .css_classes(["scry-surface", "scry-scroller"])
            .build();
        scroller.set_child(Some(&content_stack));

        let content_window =
            layer_window(app, "scry-content", OVERLAY_WIDTH_PX, KeyboardMode::None);
        content_window.set_child(Some(&scroller));

        let overlay = Rc::new(Self {
            gapp: app.clone(),
            launcher_window,
            content_window,
            launcher,
            scroller,
            content_stack,
            search,
            chat,
            sessions,
            app_context,
            model: RefCell::new(Model::new()),
            dispatcher: dispatcher.clone(),
            stuck_to_bottom: Cell::new(true),
            position: Cell::new(None),
            monitor: RefCell::new(None),
        });

        overlay.install_scroll_stickiness();
        overlay.install_launcher_drag();
        overlay.install_monitor_watcher();
        overlay.register_key_binding();

        let event_overlay = Rc::downgrade(&overlay);
        glib::spawn_future_local(async move {
            while let Ok(msg) = receiver.recv().await {
                let Some(overlay) = event_overlay.upgrade() else {
                    break;
                };
                let commands = overlay.model.borrow_mut().update(msg);
                for command in commands {
                    overlay.run(command);
                }
            }
        });

        let mut rx = hotkey.subscribe();
        let hotkey_dispatcher = dispatcher.clone();
        glib::spawn_future_local(async move {
            loop {
                match rx.recv().await {
                    Ok(()) => {
                        let _ = hotkey_dispatcher.unbounded_send(Msg::ToggleLauncherRequested);
                    },
                    Err(RecvError::Lagged(n)) => warn!("hotkey: lagged by {n} events"),
                    Err(RecvError::Closed) => break,
                }
            }
        });

        overlay
    }

    fn run(self: &Rc<Self>, command: Command) {
        match command {
            Command::ToggleLauncher => self.toggle_launcher(),
            Command::HideOverlay => self.hide(),
            Command::OpenSettings => self.open_settings(),
            Command::RunSearchQuery { content, query_id } => self.search(content, query_id),
            Command::RenderSearchQueryResult { event } => self.render_search_result(event),
            Command::RenderChatAction => self.render_chat_button(),
            Command::InvokeLocalQueryResultAction { handler_id, action } => {
                self.run_action(handler_id, action);
            },
            Command::FocusSearchEntry => self.launcher.focus(),
            Command::ClearQuery => self.launcher.clear(),
            Command::OpenSelectedSession => self.sessions.activate_selected(),
            Command::DeleteSelectedSession => self.sessions.delete_selected(),
            Command::RestoreSession {
                turn_id,
                session_id,
            } => {
                self.restore_session(turn_id, session_id);
            },
            Command::ReportError { error } => {
                error!("{error}");
            },
            Command::ClearSearchResults => self.search.clear(),
            Command::HideContent => self.close_content(),
            Command::SubmitChatPrompt { turn_id } => self.construct_chat_prompt(turn_id),
            Command::SendChat {
                turn_id,
                session_id,
                provider_id,
                prompt,
            } => self.send_chat(turn_id, session_id, provider_id, prompt),
            Command::CancelChatSession { session_id } => self.cancel_chat_session(session_id),
            Command::RenderChatEvent { event } => self.render_chat_event(event),
            Command::ShowChatView => self.show_chat_view(),
            Command::FilterSessions { content } => self.sessions.filter(content),
            Command::OpenSessions => self.show_session_view(),
            Command::ClearChatContent => self.clear_session(),
            Command::ExitSearch => self.exit_search(),
        }
    }
}

/// display
impl Overlay {
    fn current_mode(&self) -> Mode {
        self.model.borrow().mode
    }

    fn is_visible(&self) -> bool {
        self.launcher_window.is_visible()
    }

    fn show_search_view(&self) {
        self.launcher.set_mode(Mode::Search);
        self.content_stack.set_visible_child_name(SEARCH_VIEW_KEY);
        self.show_content();
    }

    fn show_chat_view(&self) {
        self.launcher.set_mode(Mode::Chat);
        self.content_stack.set_visible_child_name(CHAT_VIEW_KEY);
        self.show_content();
    }

    fn show_session_view(&self) {
        self.launcher.set_mode(Mode::Session);
        self.sessions.clear();
        self.sessions.refresh();
        self.content_stack.set_visible_child_name(SESSION_VIEW_KEY);
        self.show_content();
        self.scroller.vadjustment().set_value(0.0);
    }

    fn toggle_launcher(&self) {
        if self.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    fn show(&self) {
        if !self.launcher_window.is_visible() {
            match self.position.get() {
                Some(_) => self.layout(),
                None => {
                    // Provisional guess for the first frame; the monitor
                    // watcher finalizes once the compositor has placed
                    // the surface on its real output.
                    let (x, y) = window::centered_position(&self.launcher_window);
                    self.layout_at(x, y);
                },
            }
            self.launcher_window.present();
        }
        self.launcher.refresh();
        self.launcher.focus();
    }

    fn hide(&self) {
        self.launcher.clear();
        self.close_content();
        self.launcher_window.set_visible(false);
    }

    fn open_settings(&self) {
        self.gapp.activate_action("settings", None);
    }

    fn clear_session(&self) {
        self.chat.clear()
    }

    fn show_content(&self) {
        if !self.content_window.is_visible() {
            self.content_window.present();
        }
    }

    fn close_content(&self) {
        self.search.clear();
        self.clear_session();
        self.sessions.clear();

        self.launcher.set_mode(Mode::Search);
        self.content_stack.set_visible_child_name(SEARCH_VIEW_KEY);
        self.content_window.set_visible(false);
        // Mode is back to Search before content hides, so this reset does not
        // affect chat stickiness.
        self.scroller.vadjustment().set_value(0.0);
    }
}

/// Search related actions
impl Overlay {
    fn search(&self, content: String, query_id: u64) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(runtime::tokio_runtime().spawn(async move {
            let mut has_result = false;
            let mut render_stream = app_context.query(&content);
            while let Some(event) = render_stream.next().await {
                match event {
                    RenderEvent::Search(event) => {
                        let SearchRenderEvent::Append { response } = &event;
                        has_result |= response.items.iter().any(|item| !item.actions.is_empty());
                        let _ = dispatcher
                            .unbounded_send(Msg::SearchQueryRenderEvent { event, query_id });
                    },
                    RenderEvent::Done => {
                        let _ = dispatcher.unbounded_send(Msg::SearchQueryRenderFinished {
                            query_id,
                            has_result,
                        });
                    },
                    RenderEvent::Error { message } => {
                        error!("query render: {message}");
                    },
                    RenderEvent::Chat(_) | RenderEvent::Cancel => {
                        error!("Unexpected event for search query call. This indicates a bug.")
                    },
                }
            }
        }));
    }

    fn render_search_result(&self, event: SearchRenderEvent) {
        let SearchRenderEvent::Append { response } = event;
        if self
            .search
            .append_section(response.id, &response.name, response.items)
        {
            self.show_search_view();
        }
    }

    fn render_chat_button(&self) {
        self.search.append_chat_action();
        self.show_search_view();
    }

    fn run_action(&self, handler_id: &str, action: Action) {
        let Some(outcome) = self.app_context.run_query_action(handler_id, action) else {
            return;
        };

        match outcome {
            ActionOutcome::Hide => self.hide(),
            ActionOutcome::Stay => {},
            ActionOutcome::Replace { .. } => {},
        }
    }

    fn render_any(&self) -> bool {
        self.search.render_any()
    }

    fn exit_search(&self) {
        if self.render_any() {
            self.close_content();
            self.launcher.clear();
        } else {
            self.hide()
        }
    }
}

/// Chat related actions
impl Overlay {
    fn construct_chat_prompt(&self, turn_id: u64) {
        let prompt = self.launcher.query();
        self.launcher.clear();
        if prompt.is_empty() {
            let _ = self
                .dispatcher
                .unbounded_send(Msg::ChatPromptRejected { turn_id });
            return;
        }

        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(runtime::tokio_runtime().spawn(async move {
            let result = app_context.prefer_model().await;
            match result {
                Ok(Some(provider_id)) => {
                    let _ = dispatcher.unbounded_send(Msg::ChatPromptResolved {
                        turn_id,
                        prompt,
                        provider_id,
                    });
                },
                Ok(None) => {
                    error!("no preferred provider configured");
                    let _ = dispatcher.unbounded_send(Msg::ChatPromptRejected { turn_id });
                },
                Err(error) => {
                    error!("failed to load preferred provider: {error}");
                    let _ = dispatcher.unbounded_send(Msg::ChatPromptRejected { turn_id });
                },
            }
        }));
    }

    fn send_chat(
        &self,
        turn_id: u64,
        session_id: Option<Uuid>,
        provider: ProviderId,
        prompt: String,
    ) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(runtime::tokio_runtime().spawn(async move {
            let mut chat_render_stream = app_context.chat(session_id, provider, prompt).await;
            let session_id = chat_render_stream.session_id;
            let _ = dispatcher.unbounded_send(Msg::ChatSent {
                turn_id,
                session_id,
            });
            while let Some(event) = chat_render_stream.stream.next().await {
                let _ = dispatcher.unbounded_send(Msg::ChatRenderEvent { turn_id, event });
            }
        }));
    }

    fn cancel_chat_session(&self, session_id: Uuid) {
        let app_context = self.app_context.clone();
        drop(runtime::tokio_runtime().spawn(async move {
            if let Err(error) = app_context.cancel_session(session_id).await {
                error!("Fail to cancel chat. {error}");
            }
        }));
    }

    fn render_chat_event(&self, event: RenderEvent) {
        match event {
            RenderEvent::Chat(ChatRenderEvent::UserPrompt { text }) => {
                self.chat.append_user_prompt(&text);
            },
            RenderEvent::Chat(ChatRenderEvent::TextDelta { text, provider_id }) => {
                self.chat.append_text(&text, provider_id);
            },
            RenderEvent::Chat(ChatRenderEvent::ReasoningDelta { text }) => {
                self.chat.append_reasoning(&text);
            },
            RenderEvent::Chat(ChatRenderEvent::ToolCall {
                name,
                arguments,
                description,
                decisions,
            }) => {
                self.chat
                    .append_tool_call(&name, &arguments, description.as_deref(), &decisions);
            },
            RenderEvent::Done => {
                self.chat.finish();
            },
            RenderEvent::Error { message } => self.chat.fail(&message),
            RenderEvent::Cancel => {
                self.chat.cancel();
            },
            RenderEvent::Search(_) => {
                error!("unexpected render event on session channel");
            },
        }
    }
}

/// Session related actions
impl Overlay {
    fn restore_session(&self, turn_id: u64, session_id: Uuid) {
        let app_context = self.app_context.clone();
        let dispatcher = self.dispatcher.clone();
        drop(runtime::tokio_runtime().spawn(async move {
            match app_context.restore_session(session_id).await {
                Ok(mut render_stream) => {
                    while let Some(event) = render_stream.next().await {
                        let _ = dispatcher.unbounded_send(Msg::ChatRenderEvent { turn_id, event });
                    }
                },
                Err(error) => {
                    let _ = dispatcher.unbounded_send(Msg::SessionRestoreError { turn_id, error });
                },
            };
        }));
    }
}

/// A transparent, undecorated layer-shell window anchored top-left;
/// positioned via margins by [`Overlay::layout`].
fn layer_window(
    app: &Application,
    namespace: &str,
    width: i32,
    keyboard: KeyboardMode,
) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(width)
        .decorated(false)
        .resizable(false)
        .css_classes(["scry-window"])
        .build();

    window.init_layer_shell();
    window.set_namespace(Some(namespace));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(keyboard);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window
}
