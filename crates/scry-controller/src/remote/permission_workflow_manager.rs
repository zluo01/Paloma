use futures::future::BoxFuture;
use futures::FutureExt;
use log::{debug, error};
use scry_config::{
    PERMISSION_DECISION_TIMEOUT_SECS, PERMISSION_EVICT_TTL_SECS,
    PERMISSION_WORKFLOW_CHANNEL_CAPACITY,
};
use scry_permission::{
    ArgvDecision, CommandType, PermissionController, PermissionDecision, PermissionError,
};
use scry_utils::future::CompletableFuture;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

#[derive(Default)]
struct SessionPermission {
    always: bool,
    allowlist: HashSet<String>,
}

#[derive(Clone, PartialEq)]
pub enum PermissionState {
    Allow,
    Deny,
    Timeout,
}

struct PermissionRequest {
    decision: PermissionDecision,
    tracker: CompletableFuture<PermissionState>,
    command: Vec<String>,
    timeout_at: Instant,
    evict_at: Instant,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum UserDecision {
    AllowOnce {
        call_id: String,
    },
    Allow {
        call_id: String,
        command: String,
        glob: bool,
    },
    AllowSession {
        session_id: Uuid,
        call_id: String,
    },
    IgnorePermission {
        session_id: Uuid,
        call_id: String,
    },
    Deny {
        call_id: String,
    },
}

/// Messages for the permission workflow manager.
enum PermissionWorkflowEvent {
    InitPermissionWorkflow {
        session_id: Uuid,
        call_id: String,
        command: Vec<String>,
        reply: oneshot::Sender<Result<()>>,
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
        let mut sweep = tokio::time::interval(Duration::from_secs(1));
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

    /// Handle timeout and eviction, same thread as mpsc, so no need to worry about thread-safe
    fn sweep(&mut self) {
        let now = Instant::now();
        self.permission_tracker.retain(|call_id, request| {
            if now >= request.evict_at {
                if !request.tracker.is_completed() {
                    error!("permission for {call_id} evicted while still pending; this indicates a bug");
                }
                return false;
            }
            if now >= request.timeout_at && !request.tracker.is_completed() {
                request.tracker.complete(PermissionState::Timeout);
                debug!("permission for {call_id} is timed out");
            }
            true
        });
    }

    async fn handle_event(&mut self, event: PermissionWorkflowEvent) -> Result<()> {
        match event {
            PermissionWorkflowEvent::InitPermissionWorkflow {
                session_id,
                call_id,
                command,
                reply,
            } => {
                let result = self
                    .init_permission_workflow(session_id, call_id, command)
                    .await;
                let _ = reply.send(result);
            }
            PermissionWorkflowEvent::CheckDecision {
                session_id,
                call_id,
                reply,
            } => {
                let _ = reply.send(self.handle_check_decision(session_id, call_id));
            }
            PermissionWorkflowEvent::WaitDecision { call_id, reply } => {
                let _ = reply.send(self.handle_wait_decision(call_id));
            }
            PermissionWorkflowEvent::Decide {
                user_decision,
                reply,
            } => {
                let _ = reply.send(self.handle_user_decision(user_decision).await);
            }
        };
        Ok(())
    }

    async fn init_permission_workflow(
        &mut self,
        session_id: Uuid,
        call_id: String,
        command: Vec<String>,
    ) -> Result<()> {
        let session = self.session_permission.entry(session_id).or_default();
        let session_allows =
            session.always || session.allowlist.contains(command.join(" ").as_str());

        let decision = self.permission_controller.classify(&command).await?;

        let now = Instant::now();
        let timeout_at = now + Duration::from_secs(PERMISSION_DECISION_TIMEOUT_SECS);
        let evict_at = now + Duration::from_secs(PERMISSION_EVICT_TTL_SECS);

        // only bypass permission check if the command is a composite
        if session_allows && decision.command_type() == &CommandType::Composite {
            self.permission_tracker.insert(
                call_id,
                PermissionRequest {
                    decision: PermissionDecision::new(CommandType::Composite, ArgvDecision::Allow),
                    tracker: CompletableFuture::completed(PermissionState::Allow),
                    command,
                    timeout_at,
                    evict_at,
                },
            );
        } else {
            let tracker = match decision.decision() {
                ArgvDecision::Allow => CompletableFuture::completed(PermissionState::Allow),
                ArgvDecision::NotExecutable => CompletableFuture::completed(PermissionState::Deny),
                _ => CompletableFuture::pending(),
            };
            self.permission_tracker.insert(
                call_id,
                PermissionRequest {
                    decision,
                    tracker,
                    command,
                    timeout_at,
                    evict_at,
                },
            );
        }
        Ok(())
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
                        user_options.append(&mut generate_decision_options(
                            call_id.clone(),
                            &state.command,
                        ));

                        // deny options
                        user_options.push(UserDecision::Deny { call_id });

                        Ok(user_options)
                    }
                    ArgvDecision::AskNoPersist => {
                        Ok(generate_unsafe_decision_options(call_id, session_id))
                    }
                    ArgvDecision::Allow | ArgvDecision::NotExecutable => Ok(vec![]),
                }
            }
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
                    .tracker
                    .complete(PermissionState::Allow);
                Ok(PermissionState::Allow)
            }
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
                request.tracker.complete(PermissionState::Allow);
                Ok(PermissionState::Allow)
            }
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
                        request.tracker.complete(PermissionState::Allow);
                        Ok(PermissionState::Allow)
                    }
                    None => {
                        // Decision arrived without a prior init for this
                        // session — a bug. Deny the tracker so the waiter
                        // unblocks instead of hanging until the timeout.
                        request.tracker.complete(PermissionState::Deny);
                        Err(PermissionWorkflowError::MissingSession(session_id))
                    }
                }
            }
            UserDecision::IgnorePermission {
                session_id,
                call_id,
            } => {
                let request =
                    get_permission_request_guard(&mut self.permission_tracker, &call_id).await?;

                match self.session_permission.get_mut(&session_id) {
                    Some(session) => {
                        session.always = true;
                        request.tracker.complete(PermissionState::Allow);
                        Ok(PermissionState::Allow)
                    }
                    None => {
                        request.tracker.complete(PermissionState::Deny);
                        Err(PermissionWorkflowError::MissingSession(session_id))
                    }
                }
            }
            UserDecision::Deny { call_id } => {
                get_permission_request_guard(&mut self.permission_tracker, &call_id)
                    .await?
                    .tracker
                    .complete(PermissionState::Deny);
                Ok(PermissionState::Deny)
            }
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
        return Err(match request.tracker.get().await {
            Some(PermissionState::Timeout) => {
                PermissionWorkflowError::AlreadyTimedOut(call_id.to_string())
            }
            _ => {
                error!("Unexpected attempt to complete on completed future {call_id}. This indicates a bug.");
                PermissionWorkflowError::AlreadyResolved(call_id.to_string())
            }
        });
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
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?
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

    #[error("permission for {0} already timed out")]
    AlreadyTimedOut(String),

    #[error("permission for {0} was already resolved")]
    AlreadyResolved(String),
}

pub type Result<T> = std::result::Result<T, PermissionWorkflowError>;

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
