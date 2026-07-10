use scry_core::{Action, AppError, ProviderId, RenderEvent, SearchRenderEvent};
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
    ToggleLauncherRequested,
    OpenSettingsRequested,
    ToggleSessionsRequested,
    LauncherQueryChanged {
        content: String,
    },
    SearchQueryRenderEvent {
        query_id: u64,
        event: SearchRenderEvent,
    },
    SearchQueryRenderFinished {
        query_id: u64,
        has_result: bool,
    },
    LocalQueryResultActionRequested {
        handler_id: &'static str,
        action: Action,
    },
    SearchExitRequest,
    ChatPromptSubmitted,
    ChatPromptRejected {
        turn_id: u64,
    },
    ChatPromptResolved {
        turn_id: u64,
        prompt: String,
        provider_id: ProviderId,
    },
    ChatSent {
        turn_id: u64,
        session_id: Option<Uuid>,
    },
    ChatRenderEvent {
        turn_id: u64,
        event: RenderEvent,
    },
    ChatInterruptRequested,
    ChatExitRequested,
    ActionPanelClosed,
    SessionsCloseRequested,
    SessionDeleteRequested,
    SessionOpenRequested,
    SessionRestoreRequested {
        session_id: Uuid,
    },
    SessionRestoreError {
        turn_id: u64,
        error: AppError,
    },
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
        handler_id: &'static str,
        action: Action,
    },
    ExitSearch,
    SubmitChatPrompt {
        turn_id: u64,
    },
    SendChat {
        turn_id: u64,
        session_id: Option<Uuid>,
        provider_id: ProviderId,
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

    /// Overlay workflow. Each bullet is one distinct user action traced end to
    /// end: a command that does async work loops its result back as a follow-up
    /// message (stale results dropped by the `query_id`/`turn_id` guards), so a
    /// chain runs `Msg -> Command -> Msg -> Command -> …` until a terminal command.
    ///
    /// - Toggle launcher (hotkey): `ToggleLauncherRequested -> ToggleLauncher`.
    /// - Open settings: `OpenSettingsRequested -> OpenSettings + HideOverlay`.
    /// - Toggle sessions view: `ToggleSessionsRequested -> ClearQuery + [ClearChatContent +] OpenSessions` (enters Session mode, abandoning any live chat or pending prompt); a second toggle resets and ends at `ClearQuery + HideContent`.
    /// - Search: `LauncherQueryChanged -> ClearSearchResults + RunSearchQuery -> SearchQueryRenderEvent -> RenderSearchQueryResult` (per result) `-> SearchQueryRenderFinished -> RenderChatAction`. Empty query / no result ends at `HideContent`.
    /// - Activate a result: `LocalQueryResultActionRequested -> InvokeLocalQueryResultAction + HideOverlay`.
    /// - Close action panel: `ActionPanelClosed -> FocusSearchEntry`.
    /// - Exit search: `SearchExitRequest -> ExitSearch`.
    /// - Submit a prompt `ChatPromptSubmitted -> SubmitChatPrompt`; invalid/no-provider paths return `ChatPromptRejected`. Accepted prompts continue `-> ChatPromptResolved -> ShowChatView + SendChat -> ChatSent` (session id accepted) `-> ChatRenderEvent -> RenderChatEvent` (per delta, until `RenderEvent::Done`/`Cancel`/`Error`).
    /// - Interrupt the turn: `ChatInterruptRequested -> CancelChatSession`; the running `SendChat` stream then delivers `RenderEvent::Cancel` as `ChatRenderEvent -> RenderChatEvent`.
    /// - Exit chat: `ChatExitRequested -> ClearQuery + HideContent`.
    /// - Open a session: `SessionOpenRequested -> OpenSelectedSession -> SessionRestoreRequested -> ClearQuery + ClearChatContent + ShowChatView + RestoreSession -> ChatRenderEvent -> RenderChatEvent` (replay, until `RenderEvent::Done`/`Cancel`/`Error` releases the turn). A restore whose stream fails to open ends at `SessionRestoreError -> ReportError` (turn released).
    /// - Filter sessions: `LauncherQueryChanged -> FilterSessions`.
    /// - Delete a session: `SessionDeleteRequested -> DeleteSelectedSession`.
    /// - Close sessions view: `SessionsCloseRequested -> ClearQuery + HideContent` (resets like exiting chat).
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::ToggleLauncherRequested => {
                self.reset();
                vec![Command::ToggleLauncher]
            },
            Msg::OpenSettingsRequested => {
                self.reset();
                vec![Command::OpenSettings, Command::HideOverlay]
            },
            Msg::ToggleSessionsRequested => {
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
            Msg::LauncherQueryChanged { content } => match self.mode {
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
            Msg::SearchQueryRenderEvent { event, query_id } => {
                if query_id != self.query_id {
                    return vec![];
                }
                vec![Command::RenderSearchQueryResult { event }]
            },
            Msg::SearchQueryRenderFinished {
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
            Msg::SearchExitRequest => {
                self.reset();
                vec![Command::ExitSearch]
            },
            Msg::LocalQueryResultActionRequested { handler_id, action } => {
                self.reset();
                vec![
                    Command::InvokeLocalQueryResultAction { handler_id, action },
                    Command::HideOverlay,
                ]
            },
            Msg::ChatPromptSubmitted => {
                if self.chat_status.is_running() {
                    return vec![];
                }
                let turn_id = self.chat_status.begin();
                vec![Command::SubmitChatPrompt { turn_id }]
            },
            Msg::ChatPromptRejected { turn_id } => {
                if self.chat_status.is_current(turn_id) {
                    self.chat_status.finish();
                }
                vec![]
            },
            Msg::ChatPromptResolved {
                turn_id,
                prompt,
                provider_id,
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
                        provider_id,
                        prompt,
                    },
                ]
            },
            Msg::ChatSent {
                turn_id,
                session_id,
            } => {
                if self.chat_status.is_current(turn_id) {
                    self.current_session = session_id;
                }
                vec![]
            },
            Msg::ChatRenderEvent { turn_id, event } => {
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
            Msg::ChatInterruptRequested => {
                let Some(session_id) = self.current_session else {
                    return vec![];
                };
                vec![Command::CancelChatSession { session_id }]
            },
            Msg::ChatExitRequested | Msg::SessionsCloseRequested => self.exit_to_search(),
            Msg::ActionPanelClosed => vec![Command::FocusSearchEntry],
            Msg::SessionOpenRequested => vec![Command::OpenSelectedSession],
            Msg::SessionDeleteRequested => vec![Command::DeleteSelectedSession],
            Msg::SessionRestoreRequested { session_id } => {
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
            Msg::SessionRestoreError { error, turn_id } => {
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
    use scry_core::{ChatRenderEvent, ProviderId, RenderEvent};

    use super::*;

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
        let commands = model.update(Msg::ChatPromptSubmitted);
        let [Command::SubmitChatPrompt { turn_id }] = commands.as_slice() else {
            panic!("expected chat prompt construction to start");
        };
        assert_chat_running(model, *turn_id);
        *turn_id
    }

    fn expect_running_chat(model: &mut Model, prompt: &str) -> u64 {
        let turn_id = expect_submit_prompt(model);
        let commands = model.update(Msg::ChatPromptResolved {
            turn_id,
            prompt: prompt.into(),
            provider_id: ProviderId::Codex,
        });
        let [
            Command::ShowChatView,
            Command::SendChat {
                turn_id: sent_turn_id,
                session_id,
                provider_id,
                prompt: sent_prompt,
            },
        ] = commands.as_slice()
        else {
            panic!("expected chat view to show and chat to start");
        };
        assert_eq!(*sent_turn_id, turn_id);
        assert_eq!(*session_id, model.current_session);
        assert_eq!(*provider_id, ProviderId::Codex);
        assert_eq!(sent_prompt, prompt);
        assert!(matches!(model.mode, Mode::Chat));
        assert_chat_running(model, turn_id);
        turn_id
    }

    fn expect_restore_session(model: &mut Model, session_id: Uuid) -> u64 {
        let commands = model.update(Msg::SessionRestoreRequested { session_id });
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

    #[test]
    fn search_results_are_tied_to_the_latest_search_query() {
        let mut model = Model::new();

        let commands = model.update(Msg::LauncherQueryChanged {
            content: "RANDOM_QUERY".into(),
        });
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

        let commands = model.update(Msg::LauncherQueryChanged { content: "".into() });
        let [Command::ClearSearchResults, Command::HideContent] = commands.as_slice() else {
            panic!("expected empty search to clear and hide content");
        };
        assert_eq!(model.query_id, stale_query_id.wrapping_add(1));

        let commands = model.update(Msg::SearchQueryRenderFinished {
            query_id: stale_query_id,
            has_result: true,
        });
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);
    }

    #[test]
    fn prompt_submit_blocks_duplicates_until_prompt_resolution_rejects() {
        let mut model = Model::new();
        assert!(matches!(model.mode, Mode::Search));
        assert_chat_idle(&model);

        let rejected_turn_id = expect_submit_prompt(&mut model);
        let duplicate_commands = model.update(Msg::ChatPromptSubmitted);
        assert!(duplicate_commands.is_empty());
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_running(&model, rejected_turn_id);

        let commands = model.update(Msg::ChatPromptRejected {
            turn_id: rejected_turn_id,
        });
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
        let commands = model.update(Msg::ChatPromptResolved {
            turn_id,
            prompt: "hello".into(),
            provider_id: ProviderId::Codex,
        });
        let [
            Command::ShowChatView,
            Command::SendChat {
                turn_id: sent_turn_id,
                session_id,
                provider_id,
                prompt,
            },
        ] = commands.as_slice()
        else {
            panic!("expected chat view to show and chat to start");
        };
        assert_eq!(*sent_turn_id, turn_id);
        assert_eq!(*session_id, None);
        assert_eq!(*provider_id, ProviderId::Codex);
        assert_eq!(prompt, "hello");
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, None);
        assert_chat_running(&model, turn_id);

        let session_id = Uuid::now_v7();
        let commands = model.update(Msg::ChatSent {
            turn_id,
            session_id: Some(session_id),
        });
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(session_id));
        assert_chat_running(&model, turn_id);

        let commands = model.update(Msg::ChatInterruptRequested);
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
        let commands = model.update(Msg::ToggleSessionsRequested);
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

        let commands = model.update(Msg::ToggleSessionsRequested);
        let [Command::ClearQuery, Command::HideContent] = commands.as_slice() else {
            panic!("expected a second toggle to hide the content");
        };
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);
    }

    #[test]
    fn opening_sessions_mid_stream_abandons_the_live_turn() {
        let mut model = Model::new();
        let turn_id = expect_running_chat(&mut model, "hello");
        let session_id = Uuid::now_v7();
        let _ = model.update(Msg::ChatSent {
            turn_id,
            session_id: Some(session_id),
        });

        let commands = model.update(Msg::ToggleSessionsRequested);
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
        let commands = model.update(Msg::ChatRenderEvent {
            turn_id,
            event: RenderEvent::Chat(ChatRenderEvent::TextDelta {
                provider_id: ProviderId::Codex,
                text: "late".into(),
            }),
        });
        assert!(commands.is_empty());

        // Restoring that same session is now an ordinary restore; core
        // replays history plus pending deltas and re-attaches the stream.
        let restore_turn_id = expect_restore_session(&mut model, session_id);
        assert_ne!(restore_turn_id, turn_id);
    }

    #[test]
    fn opening_sessions_invalidates_in_flight_search_results() {
        let mut model = Model::new();
        let commands = model.update(Msg::LauncherQueryChanged {
            content: "docker".into(),
        });
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery { query_id, .. },
        ] = commands.as_slice()
        else {
            panic!("expected a search query to start");
        };
        let query_id = *query_id;

        let commands = model.update(Msg::ToggleSessionsRequested);
        assert!(matches!(
            commands.as_slice(),
            [Command::ClearQuery, Command::OpenSessions]
        ));

        // Late results from the pre-toggle search are dropped instead of
        // flipping the visible page back to search.
        let commands = model.update(Msg::SearchQueryRenderFinished {
            query_id,
            has_result: true,
        });
        assert!(commands.is_empty());
    }

    #[test]
    fn opening_sessions_drops_a_pending_prompt() {
        let mut model = Model::new();
        let turn_id = expect_submit_prompt(&mut model);

        let commands = model.update(Msg::ToggleSessionsRequested);
        assert!(matches!(
            commands.as_slice(),
            [Command::ClearQuery, Command::OpenSessions]
        ));

        // The late resolution is dropped instead of flipping the visible
        // page back to chat and sending the abandoned prompt.
        let commands = model.update(Msg::ChatPromptResolved {
            turn_id,
            prompt: "hello".into(),
            provider_id: ProviderId::Codex,
        });
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Session));
        assert_chat_idle(&model);
    }

    #[test]
    fn duplicate_restore_of_the_current_session_is_ignored() {
        let mut model = Model::new();
        let session_id = Uuid::now_v7();
        let turn_id = expect_restore_session(&mut model, session_id);

        let commands = model.update(Msg::SessionRestoreRequested { session_id });
        assert!(commands.is_empty());
        assert_chat_running(&model, turn_id);
        assert_eq!(model.current_session, Some(session_id));
    }

    #[test]
    fn typing_in_session_mode_filters_sessions() {
        let mut model = Model::new();
        let _ = model.update(Msg::ToggleSessionsRequested);

        let commands = model.update(Msg::LauncherQueryChanged {
            content: "docker".into(),
        });
        let [Command::FilterSessions { content }] = commands.as_slice() else {
            panic!("expected session-mode typing to filter sessions");
        };
        assert_eq!(content.as_str(), "docker");

        // an emptied query must reach the view untrimmed to reset visibility
        let commands = model.update(Msg::LauncherQueryChanged { content: "".into() });
        let [Command::FilterSessions { content }] = commands.as_slice() else {
            panic!("expected an emptied query to reach the filter");
        };
        assert!(content.is_empty());
    }

    #[test]
    fn closing_sessions_clears_the_query_and_resets_to_search() {
        let mut model = Model::new();
        let _ = model.update(Msg::ToggleSessionsRequested);
        assert!(matches!(model.mode, Mode::Session));

        let commands = model.update(Msg::SessionsCloseRequested);
        let [Command::ClearQuery, Command::HideContent] = commands.as_slice() else {
            panic!("expected closing sessions to clear the query and hide content");
        };
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);
    }

    #[test]
    fn stale_prompt_resolution_cannot_replace_a_restored_session() {
        let mut model = Model::new();
        let stale_turn_id = expect_submit_prompt(&mut model);
        let restored_session = Uuid::now_v7();
        let restore_turn_id = expect_restore_session(&mut model, restored_session);

        let commands = model.update(Msg::ChatPromptResolved {
            turn_id: stale_turn_id,
            prompt: "hello".into(),
            provider_id: ProviderId::Codex,
        });
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(restored_session));
        assert_chat_running(&model, restore_turn_id);

        let stale_session = Uuid::now_v7();
        let commands = model.update(Msg::ChatSent {
            turn_id: stale_turn_id,
            session_id: Some(stale_session),
        });
        assert!(commands.is_empty());
        assert_eq!(model.current_session, Some(restored_session));
        assert_chat_running(&model, restore_turn_id);

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id: stale_turn_id,
            event: RenderEvent::Chat(ChatRenderEvent::TextDelta {
                provider_id: ProviderId::Codex,
                text: "stale".into(),
            }),
        });
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

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id: stale_turn_id,
            event: RenderEvent::Done,
        });
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(current_session));
        assert_chat_running(&model, current_turn_id);

        let commands = model.update(Msg::SessionRestoreError {
            turn_id: stale_turn_id,
            error: AppError::Io(std::io::Error::other("stale restore")),
        });
        assert!(commands.is_empty());
        assert!(matches!(model.mode, Mode::Chat));
        assert_eq!(model.current_session, Some(current_session));
        assert_chat_running(&model, current_turn_id);

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id: current_turn_id,
            event: RenderEvent::Done,
        });
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

        let commands = model.update(Msg::SessionRestoreError {
            turn_id,
            error: AppError::Io(std::io::Error::other("restore failed")),
        });
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
        let commands = model.update(Msg::ChatPromptSubmitted);
        assert!(commands.is_empty());

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id,
            event: RenderEvent::Chat(ChatRenderEvent::TextDelta {
                provider_id: ProviderId::Codex,
                text: "still streaming".into(),
            }),
        });
        assert!(matches!(
            commands.as_slice(),
            [Command::RenderChatEvent { .. }]
        ));

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id,
            event: RenderEvent::Done,
        });
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

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id,
            event: RenderEvent::Error {
                message: "provider failed".into(),
            },
        });
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

        model.update(Msg::ChatSent {
            turn_id,
            session_id: Some(session_id),
        });
        assert_eq!(model.current_session, Some(session_id));

        let commands = model.update(Msg::ChatExitRequested);
        let [Command::ClearQuery, Command::HideContent] = commands.as_slice() else {
            panic!("expected chat exit to hide content");
        };
        assert!(matches!(model.mode, Mode::Search));
        assert_eq!(model.current_session, None);
        assert_chat_idle(&model);

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id,
            event: RenderEvent::Done,
        });
        assert!(commands.is_empty());

        let stale_session = Uuid::now_v7();
        let commands = model.update(Msg::ChatSent {
            turn_id,
            session_id: Some(stale_session),
        });
        assert!(commands.is_empty());
        assert_eq!(model.current_session, None);
        assert!(matches!(model.mode, Mode::Search));
        assert_chat_idle(&model);
    }
}
