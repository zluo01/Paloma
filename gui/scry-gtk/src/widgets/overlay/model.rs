use scry_core::{Action, AppError, ProviderId, RenderEvent, SearchRenderEvent};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Search,
    Chat,
}

pub(super) struct Model {
    pub(super) mode: Mode,
    current_session: Option<Uuid>,
    query_id: u64,
    turn_id: u64,
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
    ChatPromptResolved {
        prompt: String,
        provider_id: ProviderId,
    },
    ChatInitialized {
        turn_id: u64,
        provider_id: ProviderId,
        prompt: String,
        result: Result<(Uuid, bool), AppError>,
    },
    ChatSent {
        turn_id: u64,
        session_id: Uuid,
        is_new: bool,
        result: Result<(), AppError>,
    },
    ChatRenderEvent {
        turn_id: u64,
        event: RenderEvent,
    },
    ChatInterruptRequested,
    ChatExitRequested,
    ActionPanelClosed,
    SessionWindowCloseRequested,
    SessionDeleteRequested,
    SessionOpenRequested,
    SessionRestoreRequested {
        session_id: Uuid,
    },
    SessionRestoreFinished {
        result: Result<(), AppError>,
    },
}

pub(super) enum Command {
    ToggleLauncher,
    HideOverlay,
    OpenSettings,
    ToggleSessions {
        session_id: Option<Uuid>,
    },
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
    SubmitChatPrompt,
    InitChat {
        turn_id: u64,
        prior_session: Option<Uuid>,
        provider_id: ProviderId,
        prompt: String,
    },
    SendChat {
        turn_id: u64,
        session_id: Uuid,
        provider_id: ProviderId,
        prompt: String,
        is_new: bool,
    },
    CleanupChatSession {
        session_id: Uuid,
    },
    RenderChatEvent {
        event: RenderEvent,
    },
    CancelChatSession {
        session_id: Uuid,
    },
    ClearChatContent,
    FocusSearchEntry,
    CloseSessions,
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
            turn_id: 0,
        }
    }

    fn begin_query(&mut self) -> u64 {
        self.query_id = self.query_id.wrapping_add(1);
        self.query_id
    }

    fn begin_turn(&mut self) -> u64 {
        self.turn_id = self.turn_id.wrapping_add(1);
        self.turn_id
    }

    fn reset(&mut self) {
        self.mode = Mode::Search;
        self.current_session = None;
        self.begin_query();
        self.begin_turn();
    }

    /// Overlay workflow. Each bullet is one distinct user action traced end to
    /// end: a command that does async work loops its result back as a follow-up
    /// message (stale results dropped by the `query_id`/`turn_id` guards), so a
    /// chain runs `Msg -> Command -> Msg -> Command -> …` until a terminal command.
    ///
    /// - Toggle launcher (hotkey): `ToggleLauncherRequested -> ToggleLauncher`.
    /// - Open settings: `OpenSettingsRequested -> OpenSettings + HideOverlay`.
    /// - Toggle sessions window: `ToggleSessionsRequested -> ToggleSessions`.
    /// - Search: `LauncherQueryChanged -> ClearSearchResults + RunSearchQuery -> SearchQueryRenderEvent -> RenderSearchQueryResult` (per result) `-> SearchQueryRenderFinished -> RenderChatAction`. Empty query / no result ends at `HideContent`.
    /// - Activate a result: `LocalQueryResultActionRequested -> InvokeLocalQueryResultAction + HideOverlay`.
    /// - Close action panel: `ActionPanelClosed -> FocusSearchEntry`.
    /// - Exit search: `SearchExitRequest -> ExitSearch`.
    /// - Submit a prompt `ChatPromptSubmitted` `-> SubmitChatPrompt -> ChatPromptResolved -> InitChat -> ChatInitialized -> ShowChatView + SendChat -> ChatRenderEvent -> RenderChatEvent` (per delta, until `RenderEvent::Done`) `-> ChatSent`. `ChatInitialized(Err)` / `ChatSent(Err) -> ReportError` (a new session also `CleanupChatSession`).
    /// - Interrupt the turn: `ChatInterruptRequested -> CancelChatSession`; the running `SendChat` stream then delivers `RenderEvent::Cancel` as `ChatRenderEvent -> RenderChatEvent` and ends with `ChatSent`.
    /// - Exit chat: `ChatExitRequested -> HideContent`.
    /// - Open a session: `SessionOpenRequested -> OpenSelectedSession -> SessionRestoreRequested -> ClearChatContent + ShowChatView + RestoreSession -> ChatRenderEvent -> RenderChatEvent` (replay) `-> SessionRestoreFinished -> CloseSessions`. `SessionRestoreFinished(Err) -> ReportError`.
    /// - Delete a session: `SessionDeleteRequested -> DeleteSelectedSession`.
    /// - Close sessions window: `SessionWindowCloseRequested -> CloseSessions`.
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
                vec![Command::ToggleSessions {
                    session_id: self.current_session,
                }]
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
            Msg::ChatPromptSubmitted => vec![Command::SubmitChatPrompt],
            Msg::ChatPromptResolved {
                prompt,
                provider_id,
            } => {
                let prompt = prompt.trim().to_string();
                if prompt.is_empty() {
                    return vec![];
                }
                let turn_id = self.begin_turn();
                vec![Command::InitChat {
                    turn_id,
                    prior_session: self.current_session,
                    provider_id,
                    prompt,
                }]
            },
            Msg::ChatInitialized {
                turn_id,
                provider_id,
                prompt,
                result,
            } => {
                if turn_id != self.turn_id {
                    return vec![];
                }
                match result {
                    Ok((session_id, is_new)) => {
                        self.mode = Mode::Chat;
                        self.current_session = Some(session_id);
                        vec![
                            Command::ShowChatView,
                            Command::SendChat {
                                turn_id,
                                session_id,
                                provider_id,
                                prompt,
                                is_new,
                            },
                        ]
                    },
                    Err(error) => vec![Command::ReportError { error }],
                }
            },
            Msg::ChatSent {
                turn_id,
                session_id,
                is_new,
                result,
            } => {
                if turn_id != self.turn_id {
                    return vec![];
                }
                match result {
                    Ok(()) => vec![],
                    Err(error) => {
                        let mut commands = vec![Command::ReportError { error }];
                        if is_new {
                            self.current_session = None;
                            commands.push(Command::CleanupChatSession { session_id });
                        }
                        commands
                    },
                }
            },
            Msg::ChatRenderEvent { turn_id, event } => {
                if turn_id != self.turn_id {
                    return vec![];
                }
                vec![Command::RenderChatEvent { event }]
            },
            Msg::ChatInterruptRequested => {
                let Some(session_id) = self.current_session else {
                    return vec![];
                };
                vec![Command::CancelChatSession { session_id }]
            },
            Msg::ChatExitRequested => {
                self.reset();
                vec![Command::HideContent]
            },
            Msg::ActionPanelClosed => vec![Command::FocusSearchEntry],
            Msg::SessionWindowCloseRequested => vec![Command::CloseSessions],
            Msg::SessionOpenRequested => vec![Command::OpenSelectedSession],
            Msg::SessionDeleteRequested => vec![Command::DeleteSelectedSession],
            Msg::SessionRestoreRequested { session_id } => {
                let turn_id = self.begin_turn();
                self.mode = Mode::Chat;
                self.current_session = Some(session_id);
                vec![
                    Command::ClearChatContent,
                    Command::ShowChatView,
                    Command::RestoreSession {
                        turn_id,
                        session_id,
                    },
                ]
            },
            Msg::SessionRestoreFinished { result } => match result {
                Ok(()) => {
                    vec![Command::CloseSessions]
                },
                Err(error) => {
                    self.current_session = None;
                    vec![Command::ReportError { error }]
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use scry_core::{ProviderId, RenderEvent};

    use super::*;

    fn running_search_id(model: &mut Model) -> u64 {
        let commands = model.update(Msg::LauncherQueryChanged {
            content: "codex".into(),
        });
        let [
            Command::ClearSearchResults,
            Command::RunSearchQuery { query_id, .. },
        ] = commands.as_slice()
        else {
            panic!("expected a search query to start");
        };
        *query_id
    }

    fn running_turn_id(model: &mut Model) -> u64 {
        let commands = model.update(Msg::ChatPromptResolved {
            prompt: "hello".into(),
            provider_id: ProviderId::Codex,
        });
        let [Command::InitChat { turn_id, .. }] = commands.as_slice() else {
            panic!("expected a chat turn to initialize");
        };
        *turn_id
    }

    #[test]
    fn reset_invalidates_in_flight_search_results() {
        let mut model = Model::new();
        let query_id = running_search_id(&mut model);

        model.update(Msg::SearchExitRequest);

        let commands = model.update(Msg::SearchQueryRenderFinished {
            query_id,
            has_result: true,
        });
        assert!(commands.is_empty());
    }

    #[test]
    fn reset_invalidates_in_flight_chat_events() {
        let mut model = Model::new();
        let turn_id = running_turn_id(&mut model);

        model.update(Msg::ChatExitRequested);

        let commands = model.update(Msg::ChatRenderEvent {
            turn_id,
            event: RenderEvent::Done,
        });
        assert!(commands.is_empty());
    }
}
