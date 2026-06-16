use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use futures::{FutureExt, future::BoxFuture};
use log::error;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::{
    config::{PERMISSION_EVICT_TTL_SECS, PERMISSION_WORKFLOW_CHANNEL_CAPACITY},
    permission::{
        ArgvDecision, CommandType, PermissionController, PermissionDecision, PermissionError,
        PermissionState, UserDecision,
    },
    utils::CompletableFuture,
};

#[derive(Default)]
struct SessionPermission {
    always: bool,
    allowlist: HashSet<String>,
}

struct PermissionRequest {
    decision: PermissionDecision,
    tracker: CompletableFuture<PermissionState>,
    command: Vec<String>,
    /// `None` while pending (kept forever). Set when the request resolves,
    /// scheduling eviction `PERMISSION_EVICT_TTL_SECS` later.
    evict_at: Option<Instant>,
}

impl PermissionRequest {
    /// The eviction deadline for a request that resolves now.
    fn evict_deadline() -> Instant {
        Instant::now() + Duration::from_secs(PERMISSION_EVICT_TTL_SECS)
    }

    /// Resolve the request and schedule its eviction.
    fn resolve(&mut self, state: PermissionState) {
        self.tracker.complete(state);
        self.evict_at = Some(Self::evict_deadline());
    }
}

/// Messages for the permission workflow manager.
enum PermissionWorkflowEvent {
    InitPermissionWorkflow {
        session_id: Uuid,
        call_id: String,
        command: Vec<String>,
        reply: oneshot::Sender<()>,
    },
    CheckDecision {
        session_id: Uuid,
        call_id: String,
        reply: oneshot::Sender<Result<Vec<UserDecision>>>,
    },
    WaitDecision {
        call_id: String,
        reply: oneshot::Sender<Result<BoxFuture<'static, Option<PermissionState>>>>,
    },
    Decide {
        user_decision: UserDecision,
        reply: oneshot::Sender<Result<PermissionState>>,
    },
    RemovePermission {
        session_id: Uuid,
        reply: oneshot::Sender<()>,
    },
}

pub struct PermissionWorkflowManager {
    permission_controller: PermissionController,
    event_rx: mpsc::Receiver<PermissionWorkflowEvent>,
    // key is the session id.
    session_permission: HashMap<Uuid, SessionPermission>,
    // key is the call_id from llm function call payload
    permission_tracker: HashMap<String, PermissionRequest>,
}

#[derive(Clone)]
pub struct PermissionWorkflowManagerClient {
    event_tx: mpsc::Sender<PermissionWorkflowEvent>,
}

impl PermissionWorkflowManager {
    pub fn new(
        permission_controller: PermissionController,
    ) -> (Self, PermissionWorkflowManagerClient) {
        let (event_tx, event_rx) = mpsc::channel(PERMISSION_WORKFLOW_CHANNEL_CAPACITY);
        (
            Self {
                permission_controller,
                event_rx,
                session_permission: HashMap::new(),
                permission_tracker: HashMap::new(),
            },
            PermissionWorkflowManagerClient { event_tx },
        )
    }

    pub async fn run(&mut self) {
        let mut sweep = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                maybe_event = self.event_rx.recv() => {
                    let Some(event) = maybe_event else { break };
                    if let Err(err) = self.handle_event(event).await {
                        error!("permission workflow manager error: {err}");
                    }
                }
                _ = sweep.tick() => self.sweep(),
            }
        }
    }

    /// Handle complete jobs eviction, same thread as mpsc, so no need to worry about thread-safe
    fn sweep(&mut self) {
        let now = Instant::now();
        self.permission_tracker
            .retain(|_, request| request.evict_at.is_none_or(|deadline| now < deadline));
    }

    async fn handle_event(&mut self, event: PermissionWorkflowEvent) -> Result<()> {
        match event {
            PermissionWorkflowEvent::InitPermissionWorkflow {
                session_id,
                call_id,
                command,
                reply,
            } => {
                self.init_permission_workflow(session_id, call_id, command)
                    .await;
                let _ = reply.send(());
            },
            PermissionWorkflowEvent::CheckDecision {
                session_id,
                call_id,
                reply,
            } => {
                let _ = reply.send(self.handle_check_decision(session_id, call_id));
            },
            PermissionWorkflowEvent::WaitDecision { call_id, reply } => {
                let _ = reply.send(self.handle_wait_decision(call_id));
            },
            PermissionWorkflowEvent::Decide {
                user_decision,
                reply,
            } => {
                let _ = reply.send(self.handle_user_decision(user_decision).await);
            },
            PermissionWorkflowEvent::RemovePermission { session_id, reply } => {
                self.session_permission.remove(&session_id);
                let _ = reply.send(());
            },
        };
        Ok(())
    }

    async fn init_permission_workflow(
        &mut self,
        session_id: Uuid,
        call_id: String,
        command: Vec<String>,
    ) {
        let session = self.session_permission.entry(session_id).or_default();
        let session_allows =
            session.always || session.allowlist.contains(command.join(" ").as_str());

        match self.permission_controller.classify(&command).await {
            Ok(decision) => {
                // only bypass permission check if the command is a composite
                if session_allows && decision.command_type() == &CommandType::Composite {
                    self.permission_tracker.insert(
                        call_id,
                        PermissionRequest {
                            decision: PermissionDecision::new(
                                CommandType::Composite,
                                vec![], // should not be used.
                                ArgvDecision::Allow,
                            ),
                            tracker: CompletableFuture::completed(PermissionState::Allow),
                            command,
                            evict_at: Some(PermissionRequest::evict_deadline()),
                        },
                    );
                } else {
                    let (tracker, evict_at) = match decision.decision() {
                        ArgvDecision::Allow => (
                            CompletableFuture::completed(PermissionState::Allow),
                            Some(PermissionRequest::evict_deadline()),
                        ),
                        ArgvDecision::NotExecutable => (
                            CompletableFuture::completed(PermissionState::Deny),
                            Some(PermissionRequest::evict_deadline()),
                        ),
                        _ => (CompletableFuture::pending(), None),
                    };
                    self.permission_tracker.insert(
                        call_id,
                        PermissionRequest {
                            decision,
                            tracker,
                            command,
                            evict_at,
                        },
                    );
                }
            },
            Err(e) => {
                error!(
                    "Error happen when try to classify permission for command {:?}. {}",
                    &command, e
                );
                self.permission_tracker.insert(
                    call_id,
                    PermissionRequest {
                        decision: PermissionDecision::new(
                            CommandType::Composite,
                            vec![], // should not be used.
                            ArgvDecision::NotExecutable,
                        ),
                        tracker: CompletableFuture::completed(PermissionState::Error),
                        command,
                        evict_at: Some(PermissionRequest::evict_deadline()),
                    },
                );
            },
        };
    }

    fn handle_check_decision(
        &mut self,
        session_id: Uuid,
        call_id: String,
    ) -> Result<Vec<UserDecision>> {
        let state = self
            .permission_tracker
            .get_mut(&call_id)
            .ok_or_else(|| PermissionWorkflowError::MissingCallerId(call_id.clone()))?;

        // if future is already completed, then meaning no more user action required.
        if state.tracker.is_completed() {
            return Ok(vec![]);
        }

        match state.decision.command_type() {
            // should have options allow once, allow session, allow always, deny
            CommandType::Composite => Ok(generate_unsafe_decision_options(call_id, session_id)),
            CommandType::Simple => {
                match state.decision.decision() {
                    ArgvDecision::Unknown => {
                        let mut user_options: Vec<UserDecision> = Vec::new();

                        // allow once on exact
                        user_options.push(UserDecision::AllowOnce {
                            call_id: call_id.clone(),
                        });

                        // allow global for generated options
                        // we should generate the option from the stripped inner command.
                        user_options.append(&mut generate_decision_options(
                            call_id.clone(),
                            &state.decision.parsed_commands()[0],
                        ));

                        // deny options
                        user_options.push(UserDecision::Deny { call_id });

                        Ok(user_options)
                    },
                    ArgvDecision::AskNoPersist => {
                        Ok(generate_unsafe_decision_options(call_id, session_id))
                    },
                    ArgvDecision::Allow | ArgvDecision::NotExecutable => Ok(vec![]),
                }
            },
        }
    }

    fn handle_wait_decision(
        &self,
        call_id: String,
    ) -> Result<BoxFuture<'static, Option<PermissionState>>> {
        let handle = self
            .permission_tracker
            .get(&call_id)
            .ok_or_else(|| PermissionWorkflowError::MissingCallerId(call_id.clone()))?
            .tracker
            .get()
            .boxed();

        Ok(handle)
    }

    async fn handle_user_decision(
        &mut self,
        user_decision: UserDecision,
    ) -> Result<PermissionState> {
        match user_decision {
            UserDecision::AllowOnce { call_id } => {
                get_permission_request_guard(&mut self.permission_tracker, &call_id)
                    .await?
                    .resolve(PermissionState::Allow);
                Ok(PermissionState::Allow)
            },
            // for normal allow, it can either be exact allow or wildcard allow(glob enabled)
            // we will need to save the decision to database for consensus check
            UserDecision::Allow {
                call_id,
                command,
                glob,
            } => {
                let request =
                    get_permission_request_guard(&mut self.permission_tracker, &call_id).await?;
                self.permission_controller
                    .add_permission(command, glob)
                    .await?;
                request.resolve(PermissionState::Allow);
                Ok(PermissionState::Allow)
            },
            // AllowSession only happens to composite, which can either be
            // allowed command in this session or allow all composite in this session
            // for first case, we do not need to update always flag but simply add to allowlist
            // for second case, we explicitly update the always flag along with the allowlist
            UserDecision::AllowSession {
                session_id,
                call_id,
            } => {
                let request =
                    get_permission_request_guard(&mut self.permission_tracker, &call_id).await?;
                let command = request.command.join(" ");

                match self.session_permission.get_mut(&session_id) {
                    Some(session) => {
                        session.allowlist.insert(command);
                        request.resolve(PermissionState::Allow);
                        Ok(PermissionState::Allow)
                    },
                    None => {
                        // Decision arrived without a prior init for this
                        // session — a bug. Deny the tracker so the waiter
                        // unblocks instead of hanging forever.
                        request.resolve(PermissionState::Deny);
                        Err(PermissionWorkflowError::MissingSession(session_id))
                    },
                }
            },
            UserDecision::IgnorePermission {
                session_id,
                call_id,
            } => {
                let request =
                    get_permission_request_guard(&mut self.permission_tracker, &call_id).await?;

                match self.session_permission.get_mut(&session_id) {
                    Some(session) => {
                        session.always = true;
                        request.resolve(PermissionState::Allow);
                        Ok(PermissionState::Allow)
                    },
                    None => {
                        request.resolve(PermissionState::Deny);
                        Err(PermissionWorkflowError::MissingSession(session_id))
                    },
                }
            },
            UserDecision::Deny { call_id } => {
                get_permission_request_guard(&mut self.permission_tracker, &call_id)
                    .await?
                    .resolve(PermissionState::Deny);
                Ok(PermissionState::Deny)
            },
        }
    }
}

async fn get_permission_request_guard<'a>(
    permission_tracker: &'a mut HashMap<String, PermissionRequest>,
    call_id: &str,
) -> Result<&'a mut PermissionRequest> {
    let request = permission_tracker
        .get_mut(call_id)
        .ok_or_else(|| PermissionWorkflowError::MissingCallerId(call_id.to_string()))?;

    if request.tracker.is_completed() {
        error!(
            "Unexpected attempt to complete on completed future {call_id}. This indicates a bug."
        );
        return Err(PermissionWorkflowError::AlreadyResolved(
            call_id.to_string(),
        ));
    }

    Ok(request)
}

/// options for AskNoPersist
fn generate_unsafe_decision_options(call_id: String, session_id: Uuid) -> Vec<UserDecision> {
    vec![
        UserDecision::AllowOnce {
            call_id: call_id.clone(),
        },
        UserDecision::AllowSession {
            session_id,
            call_id: call_id.clone(),
        },
        UserDecision::IgnorePermission {
            session_id,
            call_id: call_id.clone(),
        },
        UserDecision::Deny { call_id },
    ]
}

fn generate_decision_options(call_id: String, command_vec: &[String]) -> Vec<UserDecision> {
    let Some(program) = command_vec.first() else {
        return Vec::new();
    };

    // Broadest: the program alone, e.g. "cargo:*".
    let mut options = vec![UserDecision::Allow {
        call_id: call_id.clone(),
        command: program.clone(),
        glob: true,
    }];

    // program + subcommand, e.g. "cargo build:*". Capped at two tokens, and
    // only when the 2nd arg is a real subcommand — not an option/flag like
    // "-f" or "--amend".
    if command_vec.get(1).is_some_and(|arg| !arg.starts_with('-')) {
        options.push(UserDecision::Allow {
            call_id,
            command: command_vec[..2].join(" "),
            glob: true,
        });
    }

    options
}

impl PermissionWorkflowManagerClient {
    /// Register a tool call and classify it, populating the pending tracker.
    pub async fn init_permission_workflow(
        &self,
        session_id: Uuid,
        call_id: String,
        command: Vec<String>,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::InitPermissionWorkflow {
                session_id,
                call_id,
                command,
                reply: reply_tx,
            })
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)
    }

    /// The option menu to present for a pending request.
    pub async fn check_decision(
        &self,
        session_id: Uuid,
        call_id: String,
    ) -> Result<Vec<UserDecision>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::CheckDecision {
                session_id,
                call_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?
    }

    /// Hand back the awaitable that resolves once the decision is made. Await
    /// the returned future (in your own task) for `Some(true)`/`Some(false)`.
    pub async fn wait_decision(
        &self,
        call_id: String,
    ) -> Result<BoxFuture<'static, Option<PermissionState>>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::WaitDecision {
                call_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?
    }

    pub async fn decide(&self, user_decision: UserDecision) -> Result<PermissionState> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::Decide {
                user_decision,
                reply: reply_tx,
            })
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?
    }

    pub async fn remove_permission(&self, session_id: Uuid) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::RemovePermission {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PermissionWorkflowError {
    #[error(transparent)]
    Permission(#[from] PermissionError),

    #[error("no permission entry for caller id {0}")]
    MissingCallerId(String),

    // The session entry is created by `init_permission_workflow`;
    // its absence afterward means the decision arrived without a prior
    // init for this session, which is a bug.
    #[error("no permission entry for session id {0}")]
    MissingSession(Uuid),

    #[error("permission workflow channel closed")]
    ChannelClosed,

    #[error("permission for {0} was already resolved")]
    AlreadyResolved(String),
}

type Result<T> = std::result::Result<T, PermissionWorkflowError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    /// A glob `Allow` with call_id "c" — asserting against it checks all
    /// three fields (call_id, command, glob) at once.
    fn allow(command: &str) -> UserDecision {
        UserDecision::Allow {
            call_id: "c".into(),
            command: command.into(),
            glob: true,
        }
    }

    #[test]
    fn single_token_yields_program_only() {
        let opts = generate_decision_options("c".into(), &argv(&["ls"]));
        assert_eq!(opts, vec![allow("ls")]);
    }

    #[test]
    fn exact_two_argvs_which_second_one_is_subcommand() {
        let opts = generate_decision_options("c".into(), &argv(&["cargo", "build"]));
        assert_eq!(opts, vec![allow("cargo"), allow("cargo build")]);
    }

    #[test]
    fn subcommand_yields_program_and_subcommand() {
        let opts = generate_decision_options("c".into(), &argv(&["cargo", "build", "-j", "8"]));
        assert_eq!(opts, vec![allow("cargo"), allow("cargo build")]);
    }

    #[test]
    fn caps_at_two_tokens() {
        let opts =
            generate_decision_options("c".into(), &argv(&["git", "commit", "-m", "x", "--amend"]));
        assert_eq!(opts, vec![allow("git"), allow("git commit")]);
    }

    #[test]
    fn short_flag_second_arg_yields_program_only() {
        let opts = generate_decision_options("c".into(), &argv(&["rm", "-rf", "dir"]));
        assert_eq!(opts, vec![allow("rm")]);
    }

    #[test]
    fn long_flag_second_arg_yields_program_only() {
        let opts = generate_decision_options("c".into(), &argv(&["cargo", "--version"]));
        assert_eq!(opts, vec![allow("cargo")]);
    }

    #[test]
    fn empty_argv_yields_no_options() {
        let opts = generate_decision_options("c".into(), &[]);
        assert!(opts.is_empty());
    }

    #[test]
    fn unsafe_options_are_the_four_session_scoped_choices() {
        let session_id = Uuid::from_u128(42);
        let opts = generate_unsafe_decision_options("c".into(), session_id);
        assert_eq!(
            opts,
            vec![
                UserDecision::AllowOnce {
                    call_id: "c".into(),
                },
                UserDecision::AllowSession {
                    session_id,
                    call_id: "c".into(),
                },
                UserDecision::IgnorePermission {
                    session_id,
                    call_id: "c".into(),
                },
                UserDecision::Deny {
                    call_id: "c".into(),
                },
            ]
        );
    }
}
