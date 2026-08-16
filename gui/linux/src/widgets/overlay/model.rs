use paloma_core::{
    Action, AppError, ExtensionCapabilityId, PermissionState, ProviderBackendId, RenderEvent,
    SearchRenderEvent, UserDecision,
};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Search,
    Chat,
    Session,
}

#[derive(PartialEq, Eq)]
enum ChatPhase {
    Idle,
    Running,
}

struct ChatStatus {
    id: u64,
    phase: ChatPhase,
}

impl ChatStatus {
    fn new() -> Self {
        Self {
            id: 0,
            phase: ChatPhase::Idle,
        }
    }

    fn begin(&mut self) -> u64 {
        self.id = self.id.wrapping_add(1);
        self.phase = ChatPhase::Running;
        self.id
    }

    fn is_current(&self, id: u64) -> bool {
        self.id == id
    }

    fn is_running(&self) -> bool {
        self.phase == ChatPhase::Running
    }

    fn finish(&mut self) {
        self.phase = ChatPhase::Idle;
    }

    fn reset(&mut self) {
        self.id = self.id.wrapping_add(1);
        self.phase = ChatPhase::Idle;
    }
}

pub(super) struct Model {
    pub(super) mode: Mode,
    current_session: Option<Uuid>,
    chat_status: ChatStatus,
    query_id: u64,
}

pub(super) enum Msg {
    Launcher(LauncherMsg),
    Search(SearchMsg),
    Chat(ChatMsg),
    Session(SessionMsg),
    ContentCloseRequested,
}

pub(super) enum LauncherMsg {
    ToggleVisibilityRequested,
    OpenSettingsRequested,
    QueryChanged { content: String },
}

pub(super) enum SearchMsg {
    QueryEventReceived {
        query_id: u64,
        event: SearchRenderEvent,
    },
    QueryFinished {
        query_id: u64,
        has_result: bool,
    },
    ResultActionRequested {
        extension_capability_id: ExtensionCapabilityId,
        action: Action,
    },
    ExitRequested,
    ActionPanelClosed,
}

pub(super) enum ChatMsg {
    PromptSubmitRequested,
    PromptPreparationFailed {
        turn_id: u64,
    },
    PromptPrepared {
        turn_id: u64,
        prompt: String,
        provider_backend_id: ProviderBackendId,
    },
    RequestStarted {
        turn_id: u64,
        session_id: Option<Uuid>,
    },
    RenderEventReceived {
        turn_id: u64,
        event: RenderEvent,
    },
    InterruptRequested,
    ToolCallDecisionRequested(UserDecision),
    ToolCallDecisionFinished(UserDecision, PermissionState),
}

pub(super) enum SessionMsg {
    ToggleViewRequested,
    OpenSelectedRequested,
    DeleteSelectedRequested,
    RestoreRequested { session_id: Uuid },
    RestoreFailed { turn_id: u64, error: AppError },
}

pub(super) enum Command {
    ToggleLauncher,
    HideOverlay,
    OpenSettings,
    ShowChatView,
    RunSearchQuery {
        query_id: u64,
        content: String,
    },
    RenderSearchQueryResult {
        event: SearchRenderEvent,
    },
    RenderChatAction,
    ClearSearchResults,
    HideContent,
    InvokeLocalQueryResultAction {
        extension_capability_id: ExtensionCapabilityId,
        action: Action,
    },
    ExitSearch,
    SubmitChatPrompt {
        turn_id: u64,
    },
    SendChat {
        turn_id: u64,
        session_id: Option<Uuid>,
        provider_backend_id: ProviderBackendId,
        prompt: String,
    },
    RenderChatEvent {
        event: RenderEvent,
    },
    CancelChatSession {
        session_id: Uuid,
    },
    ClearChatContent,
    FocusSearchEntry,
    ClearQuery,
    FilterSessions {
        content: String,
    },
    OpenSessions,
    OpenSelectedSession,
    DeleteSelectedSession,
    RestoreSession {
        turn_id: u64,
        session_id: Uuid,
    },
    ReportError {
        error: AppError,
    },
    SendDecision(UserDecision),
    ResolveToolCallDecision(UserDecision, PermissionState),
}

impl Model {
    pub fn new() -> Self {
        Self {
            mode: Mode::Search,
            current_session: None,
            query_id: 0,
            chat_status: ChatStatus::new(),
        }
    }

    fn begin_query(&mut self) -> u64 {
        self.query_id = self.query_id.wrapping_add(1);
        self.query_id
    }

    fn reset(&mut self) {
        self.mode = Mode::Search;
        self.current_session = None;
        self.chat_status.reset();
        self.begin_query();
    }

    fn exit_to_search(&mut self) -> Vec<Command> {
        self.reset();
        vec![Command::ClearQuery, Command::HideContent]
    }

    /// Routes each message group to its workflow reducer:
    /// - `Launcher` -> `update_launcher`
    /// - `Search` -> `update_search`
    /// - `Chat` -> `update_chat`
    /// - `Session` -> `update_session`
    ///
    /// `ContentCloseRequested` is shared by Chat and Session and resets the
    /// model to Search before clearing the query and hiding content.
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::Launcher(msg) => self.update_launcher(msg),
            Msg::Search(msg) => self.update_search(msg),
            Msg::Chat(msg) => self.update_chat(msg),
            Msg::Session(msg) => self.update_session(msg),
            Msg::ContentCloseRequested => self.exit_to_search(),
        }
    }

    /// Launcher workflow:
    /// - Toggle visibility: `ToggleVisibilityRequested -> ToggleLauncher`. The
    ///   summon shortcut only shows and hides: mode, query, session, and any
    ///   live turn survive so the next summon resumes where the user left off.
    ///   Escape (`SearchMsg::ExitRequested` / `ContentCloseRequested`) remains
    ///   the reset path.
    /// - Open settings: `OpenSettingsRequested -> OpenSettings + HideOverlay`.
    /// - Query input: `QueryChanged` starts a search, filters sessions, or is ignored in Chat mode. Search follow-up messages are handled by `update_search`.
    fn update_launcher(&mut self, msg: LauncherMsg) -> Vec<Command> {
        match msg {
            LauncherMsg::ToggleVisibilityRequested => vec![Command::ToggleLauncher],
            LauncherMsg::OpenSettingsRequested => {
                self.reset();
                vec![Command::OpenSettings, Command::HideOverlay]
            },
            LauncherMsg::QueryChanged { content } => match self.mode {
                Mode::Search => {
                    let query_id = self.begin_query();
                    if content.trim().is_empty() {
                        vec![Command::ClearSearchResults, Command::HideContent]
                    } else {
                        vec![
                            Command::ClearSearchResults,
                            Command::RunSearchQuery { content, query_id },
                        ]
                    }
                },
                Mode::Chat => {
                    vec![]
                },
                Mode::Session => {
                    vec![Command::FilterSessions { content }]
                },
            },
        }
    }

    /// Chat workflow:
    /// - Submit prompt: `PromptSubmitRequested -> SubmitChatPrompt`; empty/no-provider paths return `PromptPreparationFailed`. A prepared prompt continues `-> PromptPrepared -> ShowChatView + SendChat -> RequestStarted` (session id accepted) `-> RenderEventReceived -> RenderChatEvent` until a terminal event releases the turn.
    /// - Interrupt turn: `InterruptRequested -> CancelChatSession`; the running stream then delivers `RenderEvent::Cancel` through `RenderEventReceived`.
    fn update_chat(&mut self, msg: ChatMsg) -> Vec<Command> {
        match msg {
            ChatMsg::PromptSubmitRequested => {
                if self.chat_status.is_running() {
                    return vec![];
                }
                let turn_id = self.chat_status.begin();
                vec![Command::SubmitChatPrompt { turn_id }]
            },
            ChatMsg::PromptPreparationFailed { turn_id } => {
                if self.chat_status.is_current(turn_id) {
                    self.chat_status.finish();
                }
                vec![]
            },
            ChatMsg::PromptPrepared {
                turn_id,
                prompt,
                provider_backend_id,
            } => {
                if !self.chat_status.is_current(turn_id) {
                    return vec![];
                }

                self.mode = Mode::Chat;

                vec![
                    Command::ShowChatView,
                    Command::SendChat {
                        turn_id,
                        session_id: self.current_session,
                        provider_backend_id,
                        prompt,
                    },
                ]
            },
            ChatMsg::RequestStarted {
                turn_id,
                session_id,
            } => {
                if self.chat_status.is_current(turn_id) {
                    self.current_session = session_id;
                }
                vec![]
            },
            ChatMsg::RenderEventReceived { turn_id, event } => {
                if !self.chat_status.is_current(turn_id) {
                    return vec![];
                }
                if matches!(
                    &event,
                    RenderEvent::Done | RenderEvent::Error { .. } | RenderEvent::Cancel
                ) {
                    self.chat_status.finish()
                }
                vec![Command::RenderChatEvent { event }]
            },
            ChatMsg::InterruptRequested => {
                let Some(session_id) = self.current_session else {
                    return vec![];
                };
                vec![Command::CancelChatSession { session_id }]
            },
            ChatMsg::ToolCallDecisionRequested(user_decision) => {
                vec![Command::SendDecision(user_decision)]
            },
            ChatMsg::ToolCallDecisionFinished(user_decision, permission_state) => {
                vec![Command::ResolveToolCallDecision(
                    user_decision,
                    permission_state,
                )]
            },
        }
    }

    /// Search workflow:
    /// - Query: `Launcher(QueryChanged) -> ClearSearchResults + RunSearchQuery -> QueryEventReceived -> RenderSearchQueryResult` (per result) `-> QueryFinished -> RenderChatAction`. Empty query / no result ends at `HideContent`.
    /// - Activate result: `ResultActionRequested -> InvokeLocalQueryResultAction + HideOverlay`.
    /// - Close action panel: `ActionPanelClosed -> FocusSearchEntry`.
    /// - Exit view: `ExitRequested -> ExitSearch`.
    fn update_search(&mut self, msg: SearchMsg) -> Vec<Command> {
        match msg {
            SearchMsg::QueryEventReceived { event, query_id } => {
                if query_id != self.query_id {
                    return vec![];
                }
                vec![Command::RenderSearchQueryResult { event }]
            },
            SearchMsg::QueryFinished {
                query_id,
                has_result,
            } => {
                if query_id != self.query_id {
                    return vec![];
                }
                if has_result {
                    vec![Command::RenderChatAction]
                } else {
                    vec![Command::HideContent]
                }
            },
            SearchMsg::ResultActionRequested {
                extension_capability_id,
                action,
            } => {
                self.reset();
                vec![
                    Command::InvokeLocalQueryResultAction {
                        extension_capability_id,
                        action,
                    },
                    Command::HideOverlay,
                ]
            },
            SearchMsg::ExitRequested => {
                self.reset();
                vec![Command::ExitSearch]
            },
            SearchMsg::ActionPanelClosed => vec![Command::FocusSearchEntry],
        }
    }

    /// Session workflow:
    /// - Toggle view: `ToggleViewRequested -> ClearQuery + [ClearChatContent +] OpenSessions` (enters Session mode, abandoning any live chat or pending prompt); a second toggle resets and ends at `ClearQuery + HideContent`.
    /// - Open selected: `OpenSelectedRequested -> OpenSelectedSession -> RestoreRequested -> ClearQuery + ClearChatContent + ShowChatView + RestoreSession -> Chat(RenderEventReceived) -> RenderChatEvent` (replay, until a terminal event releases the turn). A stream-open failure ends at `RestoreFailed -> ReportError`.
    /// - Delete selected: `DeleteSelectedRequested -> DeleteSelectedSession`.
    fn update_session(&mut self, msg: SessionMsg) -> Vec<Command> {
        match msg {
            SessionMsg::ToggleViewRequested => {
                if self.mode != Mode::Session {
                    // late search results or a pending prompt must not flip the visible page
                    self.begin_query();
                    self.chat_status.reset();
                    let mut commands = vec![Command::ClearQuery];
                    // a live-streaming session must not double-append on restore
                    if self.mode == Mode::Chat {
                        self.current_session = None;
                        commands.push(Command::ClearChatContent);
                    }
                    self.mode = Mode::Session;
                    commands.push(Command::OpenSessions);
                    commands
                } else {
                    self.exit_to_search()
                }
            },
            SessionMsg::OpenSelectedRequested => vec![Command::OpenSelectedSession],
            SessionMsg::DeleteSelectedRequested => vec![Command::DeleteSelectedSession],
            SessionMsg::RestoreRequested { session_id } => {
                if self.current_session == Some(session_id) {
                    return vec![];
                }
                let turn_id = self.chat_status.begin();
                self.mode = Mode::Chat;
                self.current_session = Some(session_id);
                vec![
                    Command::ClearQuery,
                    Command::ClearChatContent,
                    Command::ShowChatView,
                    Command::RestoreSession {
                        turn_id,
                        session_id,
                    },
                ]
            },
            SessionMsg::RestoreFailed { error, turn_id } => {
                if self.chat_status.is_current(turn_id) {
                    self.chat_status.finish();
                    self.current_session = None;
                    return vec![Command::ReportError { error }];
                }
                vec![]
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use paloma_core::{ChatRenderEvent, ProviderBackendId, QueryResponse, RenderEvent};

    use super::*;

    fn codex() -> ProviderBackendId {
        ProviderBackendId {
            provider_id: "openai".into(),
            backend_id: "codex".into(),
        }
    }

    fn assert_chat_running(model: &Model, turn_id: u64) {
        assert!(matches!(model.chat_status.phase, ChatPhase::Running));
        assert!(model.chat_status.is_running());
        assert!(model.chat_status.is_current(turn_id));
    }

    fn assert_chat_idle(model: &Model) {
        assert!(matches!(model.chat_status.phase, ChatPhase::Idle));
        assert!(!model.chat_status.is_running());
    }

    fn expect_submit_prompt(model: &mut Model) -> u64 {
        let commands = model.update(Msg::Chat(ChatMsg::PromptSubmitRequested));
        let [Command::SubmitChatPrompt { turn_id }] = commands.as_slice() else {
            panic!("expected chat prompt construction to start");
        };
        assert_chat_running(model, *turn_id);
        *turn_id
    }

    fn expect_running_chat(model: &mut Model, prompt: &str) -> u64 {
        let turn_id = expect_submit_prompt(model);
        let commands = model.update(Msg::Chat(ChatMsg::PromptPrepared {
            turn_id,
            prompt: prompt.into(),
            provider_backend_id: codex(),
        }));
        let [
            Command::ShowChatView,
            Command::SendChat {
                turn_id: sent_turn_id,
                session_id,
                provider_backend_id,
                prompt: sent_prompt,
            },
        ] = commands.as_slice()
        else {
            panic!("expected chat view to show and chat to start");
        };
        assert_eq!(*sent_turn_id, turn_id);
        assert_eq!(*session_id, model.current_session);
        assert_eq!(*provider_backend_id, codex());
        assert_eq!(sent_prompt, prompt);
        assert!(matches!(model.mode, Mode::Chat));
        assert_chat_running(model, turn_id);
        turn_id
    }

    fn expect_restore_session(model: &mut Model, session_id: Uuid) -> u64 {
        let commands = model.update(Msg::Session(SessionMsg::RestoreRequested { session_id }));
        let [
            Command::ClearQuery,
            Command::ClearChatContent,
            Command::ShowChatView,
            Command::RestoreSession {
                turn_id,
                session_id: restored_session_id,
            },
        ] = commands.as_slice()
        else {
            panic!("expected selected session to start restoring");
        };
        assert_eq!(*restored_session_id, session_id);
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(session_id));
        assert_chat_running(model, *turn_id);
        *turn_id
    }

    fn query_event(id: &'static str) -> SearchRenderEvent {
        SearchRenderEvent::Append {
            response: QueryResponse {
                extension_capability_id: ExtensionCapabilityId {
                    extension_id: id.into(),
                    capability_id: id.into(),
                },
                name: id.into(),
                items: vec![],
            },
        }
    }

    #[test]
    fn search_results_are_tied_to_the_latest_search_query() {
        let mut model = Model::new();

        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "RANDOM_QUERY".into(),
        }));
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery {
                query_id: stale_query_id,
                content,
            },
        ] = commands.as_slice()
        else {
            panic!("expected a search query to start");
        };
        assert_eq!(content, "RANDOM_QUERY");
        let stale_query_id = *stale_query_id;
        assert_eq!(model.query_id, stale_query_id);
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);

        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "".into(),
        }));
        let [Command::ClearSearchResults, Command::HideContent] = commands.as_slice() else {
            panic!("expected empty search to clear and hide content");
        };
        assert_eq!(model.query_id, stale_query_id.wrapping_add(1));

        let commands = model.update(Msg::Search(SearchMsg::QueryFinished {
            query_id: stale_query_id,
            has_result: true,
        }));
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);
    }

    #[test]
    fn search_query_events_accept_only_the_current_query() {
        let mut model = Model::new();
        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "first".into(),
        }));
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery { query_id, .. },
        ] = commands.as_slice()
        else {
            panic!("expected a search query to start");
        };
        let query_id = *query_id;

        let commands = model.update(Msg::Search(SearchMsg::QueryEventReceived {
            query_id,
            event: query_event("current"),
        }));
        let [
            Command::RenderSearchQueryResult {
                event: SearchRenderEvent::Append { response },
            },
        ] = commands.as_slice()
        else {
            panic!("expected the current query event to render");
        };
        assert_eq!(response.extension_capability_id.capability_id, "current");

        let _ = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "second".into(),
        }));
        let commands = model.update(Msg::Search(SearchMsg::QueryEventReceived {
            query_id,
            event: query_event("stale"),
        }));
        assert!(commands.is_empty());
    }

    #[test]
    fn current_search_completion_renders_action_or_hides_content() {
        let mut model = Model::new();
        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "with result".into(),
        }));
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery { query_id, .. },
        ] = commands.as_slice()
        else {
            panic!("expected a search query to start");
        };
        let query_id = *query_id;

        assert!(matches!(
            model
                .update(Msg::Search(SearchMsg::QueryFinished {
                    query_id,
                    has_result: true,
                }))
                .as_slice(),
            [Command::RenderChatAction]
        ));

        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "without result".into(),
        }));
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery { query_id, .. },
        ] = commands.as_slice()
        else {
            panic!("expected another search query to start");
        };
        assert!(matches!(
            model
                .update(Msg::Search(SearchMsg::QueryFinished {
                    query_id: *query_id,
                    has_result: false,
                }))
                .as_slice(),
            [Command::HideContent]
        ));
    }

    #[test]
    fn search_result_action_invokes_handler_and_resets_model() {
        let mut model = Model::new();
        let _ = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "query".into(),
        }));
        let query_id = model.query_id;
        let action = Action {
            label: "Open".into(),
            params: vec!["target".into()],
            primary: true,
        };

        let commands = model.update(Msg::Search(SearchMsg::ResultActionRequested {
            extension_capability_id: ExtensionCapabilityId {
                extension_id: "extension".into(),
                capability_id: "handler".into(),
            },
            action,
        }));
        let [
            Command::InvokeLocalQueryResultAction {
                extension_capability_id,
                action,
            },
            Command::HideOverlay,
        ] = commands.as_slice()
        else {
            panic!("expected the result action to run and hide the overlay");
        };
        assert_eq!(extension_capability_id.extension_id, "extension");
        assert_eq!(extension_capability_id.capability_id, "handler");
        assert_eq!(action.label, "Open");
        assert_eq!(action.params, ["target"]);
        assert!(action.primary);
        assert_eq!(model.query_id, query_id.wrapping_add(1));
        assert!(matches!(model.mode, Mode::Search));
        assert_chat_idle(&model);
    }

    #[test]
    fn search_exit_and_action_panel_close_emit_their_view_commands() {
        let mut model = Model::new();

        assert!(matches!(
            model
                .update(Msg::Search(SearchMsg::ActionPanelClosed))
                .as_slice(),
            [Command::FocusSearchEntry]
        ));
        assert!(matches!(
            model
                .update(Msg::Search(SearchMsg::ExitRequested))
                .as_slice(),
            [Command::ExitSearch]
        ));
        assert!(matches!(model.mode, Mode::Search));
        assert_chat_idle(&model);
    }

    #[test]
    fn prompt_submit_blocks_duplicates_until_preparation_fails() {
        let mut model = Model::new();
        assert!(matches!(model.mode, Mode::Search));
        assert_chat_idle(&model);

        let rejected_turn_id = expect_submit_prompt(&mut model);
        let duplicate_commands = model.update(Msg::Chat(ChatMsg::PromptSubmitRequested));
        assert!(duplicate_commands.is_empty());
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_running(&model, rejected_turn_id);

        let commands = model.update(Msg::Chat(ChatMsg::PromptPreparationFailed {
            turn_id: rejected_turn_id,
        }));
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);

        let next_turn_id = expect_submit_prompt(&mut model);
        assert_ne!(next_turn_id, rejected_turn_id);
        assert_chat_running(&model, next_turn_id);
    }

    #[test]
    fn accepted_prompt_enters_chat_and_uses_the_backend_session_id() {
        let mut model = Model::new();
        let turn_id = expect_submit_prompt(&mut model);
        let commands = model.update(Msg::Chat(ChatMsg::PromptPrepared {
            turn_id,
            prompt: "hello".into(),
            provider_backend_id: codex(),
        }));
        let [
            Command::ShowChatView,
            Command::SendChat {
                turn_id: sent_turn_id,
                session_id,
                provider_backend_id,
                prompt,
            },
        ] = commands.as_slice()
        else {
            panic!("expected chat view to show and chat to start");
        };
        assert_eq!(*sent_turn_id, turn_id);
        assert_eq!(*session_id, None);
        assert_eq!(*provider_backend_id, codex());
        assert_eq!(prompt, "hello");
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, None);
        assert_chat_running(&model, turn_id);

        let session_id = Uuid::now_v7();
        let commands = model.update(Msg::Chat(ChatMsg::RequestStarted {
            turn_id,
            session_id: Some(session_id),
        }));
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(session_id));
        assert_chat_running(&model, turn_id);

        let commands = model.update(Msg::Chat(ChatMsg::InterruptRequested));
        let [
            Command::CancelChatSession {
                session_id: cancel_session,
            },
        ] = commands.as_slice()
        else {
            panic!("expected interrupt to cancel the active backend session");
        };
        assert_eq!(*cancel_session, session_id);

        // Entering sessions abandons the live chat: turn invalidated,
        // session dropped, transcript cleared.
        let commands = model.update(Msg::Session(SessionMsg::ToggleViewRequested));
        let [
            Command::ClearQuery,
            Command::ClearChatContent,
            Command::OpenSessions,
        ] = commands.as_slice()
        else {
            panic!("expected toggle to abandon the chat and open the sessions view");
        };
        assert!(matches!(model.mode, Mode::Session));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);

        let commands = model.update(Msg::Session(SessionMsg::ToggleViewRequested));
        let [Command::ClearQuery, Command::HideContent] = commands.as_slice() else {
            panic!("expected a second toggle to hide the content");
        };
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);
    }

    #[test]
    fn summoning_hides_and_shows_without_disturbing_the_state() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "hello");
        let session_id = Uuid::now_v7();
        let _ = model.update(Msg::Chat(ChatMsg::RequestStarted {
            turn_id,
            session_id: Some(session_id),
        }));

        for _ in 0..2 {
            let commands = model.update(Msg::Launcher(LauncherMsg::ToggleVisibilityRequested));
            assert!(matches!(commands.as_slice(), [Command::ToggleLauncher]));
            assert!(matches!(model.mode, Mode::Chat));
            assert_eq!(model.current_session, Some(session_id));
            assert_chat_running(&model, turn_id);
        }

        // The turn survives the round trip, so its stream still renders.
        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: RenderEvent::Done,
        }));
        assert!(matches!(
            commands.as_slice(),
            [Command::RenderChatEvent { .. }]
        ));
        assert_chat_idle(&model);

        // Escape still resets everything.
        let commands = model.update(Msg::ContentCloseRequested);
        assert!(matches!(
            commands.as_slice(),
            [Command::ClearQuery, Command::HideContent]
        ));
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
    }

    #[test]
    fn summoning_keeps_an_in_flight_search_query_alive() {
        let mut model = Model::new();
        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "docker".into(),
        }));
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery { query_id, .. },
        ] = commands.as_slice()
        else {
            panic!("expected a search query to start");
        };
        let query_id = *query_id;

        let _ = model.update(Msg::Launcher(LauncherMsg::ToggleVisibilityRequested));
        let _ = model.update(Msg::Launcher(LauncherMsg::ToggleVisibilityRequested));

        // Results for the preserved query still belong to the restored view.
        let commands = model.update(Msg::Search(SearchMsg::QueryFinished {
            query_id,
            has_result: true,
        }));
        assert!(matches!(commands.as_slice(), [Command::RenderChatAction]));
    }

    #[test]
    fn opening_sessions_mid_stream_abandons_the_live_turn() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "hello");
        let session_id = Uuid::now_v7();
        let _ = model.update(Msg::Chat(ChatMsg::RequestStarted {
            turn_id,
            session_id: Some(session_id),
        }));

        let commands = model.update(Msg::Session(SessionMsg::ToggleViewRequested));
        let [
            Command::ClearQuery,
            Command::ClearChatContent,
            Command::OpenSessions,
        ] = commands.as_slice()
        else {
            panic!("expected entering sessions to abandon the live chat");
        };
        assert!(matches!(model.mode, Mode::Session));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);

        // The orphaned stream no longer renders into the hidden chat view.
        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: RenderEvent::Chat(ChatRenderEvent::TextDelta {
                provider_backend_id: codex(),
                text: "late".into(),
            }),
        }));
        assert!(commands.is_empty());

        // Restoring that same session is now an ordinary restore; core
        // replays history plus pending deltas and re-attaches the stream.
        let restore_turn_id = expect_restore_session(&mut model, session_id);
        assert_ne!(restore_turn_id, turn_id);
    }

    #[test]
    fn opening_sessions_invalidates_in_flight_search_results() {
        let mut model = Model::new();
        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "docker".into(),
        }));
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery { query_id, .. },
        ] = commands.as_slice()
        else {
            panic!("expected a search query to start");
        };
        let query_id = *query_id;

        let commands = model.update(Msg::Session(SessionMsg::ToggleViewRequested));
        assert!(matches!(
            commands.as_slice(),
            [Command::ClearQuery, Command::OpenSessions]
        ));

        // Late results from the pre-toggle search are dropped instead of
        // flipping the visible page back to search.
        let commands = model.update(Msg::Search(SearchMsg::QueryFinished {
            query_id,
            has_result: true,
        }));
        assert!(commands.is_empty());
    }

    #[test]
    fn opening_sessions_drops_a_pending_prompt() {
        let mut model = Model::new();
        let turn_id = expect_submit_prompt(&mut model);

        let commands = model.update(Msg::Session(SessionMsg::ToggleViewRequested));
        assert!(matches!(
            commands.as_slice(),
            [Command::ClearQuery, Command::OpenSessions]
        ));

        // The late prepared prompt is dropped instead of flipping the visible
        // page back to chat and sending the abandoned prompt.
        let commands = model.update(Msg::Chat(ChatMsg::PromptPrepared {
            turn_id,
            prompt: "hello".into(),
            provider_backend_id: codex(),
        }));
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Session));
        assert_chat_idle(&model);
    }

    #[test]
    fn duplicate_restore_of_the_current_session_is_ignored() {
        let mut model = Model::new();
        let session_id = Uuid::now_v7();
        let turn_id = expect_restore_session(&mut model, session_id);

        let commands = model.update(Msg::Session(SessionMsg::RestoreRequested { session_id }));
        assert!(commands.is_empty());
        assert_chat_running(&model, turn_id);
        assert_eq!(model.current_session, Some(session_id));
    }

    #[test]
    fn typing_in_session_mode_filters_sessions() {
        let mut model = Model::new();
        let _ = model.update(Msg::Session(SessionMsg::ToggleViewRequested));

        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "docker".into(),
        }));
        let [Command::FilterSessions { content }] = commands.as_slice() else {
            panic!("expected session-mode typing to filter sessions");
        };
        assert_eq!(content.as_str(), "docker");

        // an emptied query must reach the view untrimmed to reset visibility
        let commands = model.update(Msg::Launcher(LauncherMsg::QueryChanged {
            content: "".into(),
        }));
        let [Command::FilterSessions { content }] = commands.as_slice() else {
            panic!("expected an emptied query to reach the filter");
        };
        assert!(content.is_empty());
    }

    #[test]
    fn closing_sessions_clears_the_query_and_resets_to_search() {
        let mut model = Model::new();
        let _ = model.update(Msg::Session(SessionMsg::ToggleViewRequested));
        assert!(matches!(model.mode, Mode::Session));

        let commands = model.update(Msg::ContentCloseRequested);
        let [Command::ClearQuery, Command::HideContent] = commands.as_slice() else {
            panic!("expected closing sessions to clear the query and hide content");
        };
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);
    }

    #[test]
    fn stale_prepared_prompt_cannot_replace_a_restored_session() {
        let mut model = Model::new();
        let stale_turn_id = expect_submit_prompt(&mut model);
        let restored_session = Uuid::now_v7();
        let restore_turn_id = expect_restore_session(&mut model, restored_session);

        let commands = model.update(Msg::Chat(ChatMsg::PromptPrepared {
            turn_id: stale_turn_id,
            prompt: "hello".into(),
            provider_backend_id: codex(),
        }));
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(restored_session));
        assert_chat_running(&model, restore_turn_id);

        let stale_session = Uuid::now_v7();
        let commands = model.update(Msg::Chat(ChatMsg::RequestStarted {
            turn_id: stale_turn_id,
            session_id: Some(stale_session),
        }));
        assert!(commands.is_empty());
        assert_eq!(model.current_session, Some(restored_session));
        assert_chat_running(&model, restore_turn_id);

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id: stale_turn_id,
            event: RenderEvent::Chat(ChatRenderEvent::TextDelta {
                provider_backend_id: codex(),
                text: "stale".into(),
            }),
        }));
        assert!(commands.is_empty());
        assert_eq!(model.current_session, Some(restored_session));
        assert_chat_running(&model, restore_turn_id);
    }

    #[test]
    fn stale_restore_events_and_finish_do_not_touch_the_current_restore() {
        let mut model = Model::new();
        let stale_session = Uuid::now_v7();
        let current_session = Uuid::now_v7();

        let stale_turn_id = expect_restore_session(&mut model, stale_session);
        let current_turn_id = expect_restore_session(&mut model, current_session);
        assert_ne!(current_turn_id, stale_turn_id);

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id: stale_turn_id,
            event: RenderEvent::Done,
        }));
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(current_session));
        assert_chat_running(&model, current_turn_id);

        let commands = model.update(Msg::Session(SessionMsg::RestoreFailed {
            turn_id: stale_turn_id,
            error: AppError::Io(std::io::Error::other("stale restore")),
        }));
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(current_session));
        assert_chat_running(&model, current_turn_id);

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id: current_turn_id,
            event: RenderEvent::Done,
        }));
        assert!(matches!(
            commands.as_slice(),
            [Command::RenderChatEvent { .. }]
        ));
        assert_chat_idle(&model);
    }

    #[test]
    fn current_restore_failure_clears_only_the_current_session() {
        let mut model = Model::new();
        let session_id = Uuid::now_v7();
        let turn_id = expect_restore_session(&mut model, session_id);

        let commands = model.update(Msg::Session(SessionMsg::RestoreFailed {
            turn_id,
            error: AppError::Io(std::io::Error::other("restore failed")),
        }));
        let [Command::ReportError { error }] = commands.as_slice() else {
            panic!("expected current restore failure to report the error");
        };
        assert!(error.to_string().contains("restore failed"));
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);
    }

    #[test]
    fn restored_stream_blocks_prompts_until_its_terminal_event() {
        let mut model = Model::new();
        let session_id = Uuid::now_v7();
        let turn_id = expect_restore_session(&mut model, session_id);
        assert_chat_running(&model, turn_id);

        // Enter during the replayed/re-attached stream stays a guarded no-op.
        let commands = model.update(Msg::Chat(ChatMsg::PromptSubmitRequested));
        assert!(commands.is_empty());

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: RenderEvent::Chat(ChatRenderEvent::TextDelta {
                provider_backend_id: codex(),
                text: "still streaming".into(),
            }),
        }));
        assert!(matches!(
            commands.as_slice(),
            [Command::RenderChatEvent { .. }]
        ));

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: RenderEvent::Done,
        }));
        assert!(matches!(
            commands.as_slice(),
            [Command::RenderChatEvent { .. }]
        ));
        assert_chat_idle(&model);
    }

    #[test]
    fn terminal_chat_error_renders_and_releases_the_turn() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "hello");

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: RenderEvent::Error {
                message: "provider failed".into(),
            },
        }));
        let [
            Command::RenderChatEvent {
                event: RenderEvent::Error { message },
            },
        ] = commands.as_slice()
        else {
            panic!("expected terminal chat error to render");
        };
        assert_eq!(message, "provider failed");
        assert!(matches!(model.mode, Mode::Chat));
        assert_chat_idle(&model);

        let next_turn_id = expect_submit_prompt(&mut model);
        assert_ne!(next_turn_id, turn_id);
        assert_chat_running(&model, next_turn_id);
    }

    #[test]
    fn reset_invalidates_running_chat_messages_and_clears_session_state() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "hello");
        let session_id = Uuid::now_v7();

        model.update(Msg::Chat(ChatMsg::RequestStarted {
            turn_id,
            session_id: Some(session_id),
        }));
        assert_eq!(model.current_session, Some(session_id));

        let commands = model.update(Msg::ContentCloseRequested);
        let [Command::ClearQuery, Command::HideContent] = commands.as_slice() else {
            panic!("expected chat exit to hide content");
        };
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: RenderEvent::Done,
        }));
        assert!(commands.is_empty());

        let stale_session = Uuid::now_v7();
        let commands = model.update(Msg::Chat(ChatMsg::RequestStarted {
            turn_id,
            session_id: Some(stale_session),
        }));
        assert!(commands.is_empty());
        assert_eq!(model.current_session, None);
        assert!(matches!(model.mode, Mode::Search));
        assert_chat_idle(&model);
    }

    fn tool_call_event(call_id: &str) -> RenderEvent {
        RenderEvent::Chat(ChatRenderEvent::ToolCall {
            tool_name: "bash".into(),
            arguments: "rm -rf target".into(),
            description: None,
            decisions: vec![
                UserDecision::AllowOnce {
                    call_id: call_id.into(),
                },
                UserDecision::Deny {
                    call_id: call_id.into(),
                },
            ],
        })
    }

    fn expect_tool_call_rendered(model: &mut Model, turn_id: u64, call_id: &str) {
        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: tool_call_event(call_id),
        }));
        let [
            Command::RenderChatEvent {
                event: RenderEvent::Chat(ChatRenderEvent::ToolCall { .. }),
            },
        ] = commands.as_slice()
        else {
            panic!("expected the tool call to render");
        };
    }

    #[test]
    fn toolcall_decision_workflow_sends_resolves_and_keeps_the_turn_running() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "clean the build");
        expect_tool_call_rendered(&mut model, turn_id, "call-1");

        let decision = UserDecision::AllowOnce {
            call_id: "call-1".into(),
        };
        let commands = model.update(Msg::Chat(ChatMsg::ToolCallDecisionRequested(
            decision.clone(),
        )));
        let [Command::SendDecision(sent)] = commands.as_slice() else {
            panic!("expected the decision to be sent");
        };
        assert_eq!(*sent, decision);
        assert_chat_running(&model, turn_id);

        let commands = model.update(Msg::Chat(ChatMsg::ToolCallDecisionFinished(
            decision.clone(),
            PermissionState::Allow,
        )));
        let [Command::ResolveToolCallDecision(resolved, PermissionState::Allow)] =
            commands.as_slice()
        else {
            panic!("expected the decision to resolve");
        };
        assert_eq!(*resolved, decision);
        assert_chat_running(&model, turn_id);

        let commands = model.update(Msg::Chat(ChatMsg::RenderEventReceived {
            turn_id,
            event: RenderEvent::Done,
        }));
        assert!(matches!(
            commands.as_slice(),
            [Command::RenderChatEvent {
                event: RenderEvent::Done
            }]
        ));
        assert_chat_idle(&model);
    }

    #[test]
    fn toolcall_decision_denied_resolves_with_the_deny_state() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "clean the build");
        expect_tool_call_rendered(&mut model, turn_id, "call-1");

        let decision = UserDecision::Deny {
            call_id: "call-1".into(),
        };
        let commands = model.update(Msg::Chat(ChatMsg::ToolCallDecisionRequested(
            decision.clone(),
        )));
        assert!(matches!(commands.as_slice(), [Command::SendDecision(sent)] if *sent == decision));

        let commands = model.update(Msg::Chat(ChatMsg::ToolCallDecisionFinished(
            decision.clone(),
            PermissionState::Deny,
        )));
        assert!(matches!(
            commands.as_slice(),
            [Command::ResolveToolCallDecision(resolved, PermissionState::Deny)] if *resolved == decision
        ));
        assert_chat_running(&model, turn_id);
    }

    #[test]
    fn toolcall_decision_reply_after_chat_exit_is_still_forwarded_to_the_view() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "clean the build");
        expect_tool_call_rendered(&mut model, turn_id, "call-1");

        let decision = UserDecision::AllowOnce {
            call_id: "call-1".into(),
        };
        let _ = model.update(Msg::Chat(ChatMsg::ToolCallDecisionRequested(
            decision.clone(),
        )));

        let commands = model.update(Msg::ContentCloseRequested);
        let [Command::ClearQuery, Command::HideContent] = commands.as_slice() else {
            panic!("expected chat exit to hide content");
        };

        // The model has no per-call state yet: a late reply is forwarded and
        // the view is responsible for dropping decisions it no longer tracks.
        let commands = model.update(Msg::Chat(ChatMsg::ToolCallDecisionFinished(
            decision.clone(),
            PermissionState::Error,
        )));
        assert!(matches!(
            commands.as_slice(),
            [Command::ResolveToolCallDecision(resolved, PermissionState::Error)] if *resolved == decision
        ));
    }
}
