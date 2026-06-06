use futures::future::BoxFuture;
use futures::FutureExt;
use log::error;
use scry_config::PERMISSION_WORKFLOW_CHANNEL_CAPACITY;
use scry_permission::{
    ArgvDecision, CommandType, PermissionController, PermissionDecision, PermissionError,
};
use scry_utils::future::CompletableFuture;
use std::collections::{HashMap, HashSet};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

#[derive(Default)]
struct SessionPermission {
    always: bool,
    allowlist: HashSet<String>,
}

struct PermissionState {
    decision: PermissionDecision,
    tracker: CompletableFuture<bool>,
    command: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum UserDecision {
    AllowOnce {
        caller_id: String,
    },
    Allow {
        caller_id: String,
        command: String,
        glob: bool,
    },
    AllowSession {
        session_id: Uuid,
        caller_id: String,
    },
    IgnorePermission {
        session_id: Uuid,
        caller_id: String,
    },
    Deny {
        caller_id: String,
    },
}

/// Messages for the permission workflow manager.
enum PermissionWorkflowEvent {
    InitPermissionWorkflow {
        session_id: Uuid,
        caller_id: String,
        command: Vec<String>,
        reply: oneshot::Sender<Result<()>>,
    },
    CheckDecision {
        session_id: Uuid,
        caller_id: String,
        reply: oneshot::Sender<Result<Vec<UserDecision>>>,
    },
    WaitDecision {
        caller_id: String,
        reply: oneshot::Sender<Result<BoxFuture<'static, Option<bool>>>>,
    },
    Decide {
        user_decision: UserDecision,
        reply: oneshot::Sender<Result<()>>,
    },
}

pub struct PermissionWorkflowManager {
    permission_controller: PermissionController,
    event_rx: mpsc::Receiver<PermissionWorkflowEvent>,
    // key is the session id.
    session_permission: HashMap<Uuid, SessionPermission>,
    // key is the caller_id from llm function call payload
    permission_tracker: HashMap<String, PermissionState>,
}

#[derive(Clone)]
pub struct PermissionWorkflowManagerClient {
    event_tx: mpsc::Sender<PermissionWorkflowEvent>,
}

impl PermissionWorkflowManagerClient {
    /// Register a tool call and classify it, populating the pending tracker.
    pub async fn init_permission_workflow(
        &self,
        session_id: Uuid,
        caller_id: String,
        command: Vec<String>,
    ) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::InitPermissionWorkflow {
                session_id,
                caller_id,
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
        caller_id: String,
    ) -> Result<Vec<UserDecision>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::CheckDecision {
                session_id,
                caller_id,
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
        caller_id: String,
    ) -> Result<BoxFuture<'static, Option<bool>>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(PermissionWorkflowEvent::WaitDecision {
                caller_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| PermissionWorkflowError::ChannelClosed)?
    }

    /// Submit the user's decision, completing the pending tracker.
    pub async fn decide(&self, user_decision: UserDecision) -> Result<()> {
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
        while let Some(event) = self.event_rx.recv().await {
            if let Err(err) = self.handle_event(event).await {
                error!("permission workflow manager error: {err}");
            }
        }
    }

    async fn handle_event(&mut self, event: PermissionWorkflowEvent) -> Result<()> {
        match event {
            PermissionWorkflowEvent::InitPermissionWorkflow {
                session_id,
                caller_id,
                command,
                reply,
            } => {
                let result = self
                    .init_permission_workflow(session_id, caller_id, command)
                    .await;
                let _ = reply.send(result);
            }
            PermissionWorkflowEvent::CheckDecision {
                session_id,
                caller_id,
                reply,
            } => {
                let _ = reply.send(self.handle_check_decision(session_id, caller_id));
            }
            PermissionWorkflowEvent::WaitDecision { caller_id, reply } => {
                let _ = reply.send(self.handle_wait_decision(caller_id));
            }
            PermissionWorkflowEvent::Decide {
                user_decision,
                reply,
            } => {
                let result = self.handle_user_decision(user_decision).await;
                let _ = reply.send(result);
            }
        };
        Ok(())
    }

    async fn init_permission_workflow(
        &mut self,
        session_id: Uuid,
        caller_id: String,
        command: Vec<String>,
    ) -> Result<()> {
        let session = self.session_permission.entry(session_id).or_default();
        let session_allows =
            session.always || session.allowlist.contains(command.join(" ").as_str());

        let decision = self.permission_controller.classify(&command).await?;

        // only bypass permission check if the command is a composite
        if session_allows && decision.command_type() == &CommandType::Composite {
            self.permission_tracker.insert(
                caller_id,
                PermissionState {
                    decision: PermissionDecision::new(CommandType::Composite, ArgvDecision::Allow),
                    tracker: CompletableFuture::completed(true),
                    command,
                },
            );
        } else {
            let tracker = match decision.decision() {
                ArgvDecision::Allow => CompletableFuture::completed(true),
                ArgvDecision::NotExecutable => CompletableFuture::completed(false),
                _ => CompletableFuture::pending(),
            };
            self.permission_tracker.insert(
                caller_id,
                PermissionState {
                    decision,
                    tracker,
                    command,
                },
            );
        }
        Ok(())
    }

    fn handle_check_decision(
        &mut self,
        session_id: Uuid,
        caller_id: String,
    ) -> Result<Vec<UserDecision>> {
        let state = self
            .permission_tracker
            .get_mut(&caller_id)
            .ok_or_else(|| PermissionWorkflowError::MissingCallerId(caller_id.clone()))?;

        match state.decision.command_type() {
            // should have options allow once, allow session, allow always, deny
            CommandType::Composite => Ok(generate_unsafe_decision_options(caller_id, session_id)),
            CommandType::Simple => {
                match state.decision.decision() {
                    ArgvDecision::Unknown => {
                        let mut user_options: Vec<UserDecision> = Vec::new();

                        // allow once on exact
                        user_options.push(UserDecision::AllowOnce {
                            caller_id: caller_id.clone(),
                        });

                        // allow global for generated options
                        user_options.append(&mut generate_decision_options(
                            caller_id.clone(),
                            &state.command,
                        ));

                        // deny options
                        user_options.push(UserDecision::Deny { caller_id });

                        Ok(user_options)
                    }
                    ArgvDecision::AskNoPersist => {
                        Ok(generate_unsafe_decision_options(caller_id, session_id))
                    }
                    ArgvDecision::Allow | ArgvDecision::NotExecutable => Ok(vec![]),
                }
            }
        }
    }

    fn handle_wait_decision(&self, caller_id: String) -> Result<BoxFuture<'static, Option<bool>>> {
        let handle = self
            .permission_tracker
            .get(&caller_id)
            .ok_or_else(|| PermissionWorkflowError::MissingCallerId(caller_id.clone()))?
            .tracker
            .get()
            .boxed();

        Ok(handle)
    }

    async fn handle_user_decision(&mut self, user_decision: UserDecision) -> Result<()> {
        match user_decision {
            UserDecision::AllowOnce { caller_id } => {
                self.permission_tracker
                    .get_mut(&caller_id)
                    .ok_or_else(|| PermissionWorkflowError::MissingCallerId(caller_id.clone()))?
                    .tracker
                    .complete(true);
                Ok(())
            }
            // for normal allow, it can either be exact allow or wildcard allow(glob enabled)
            // we will need to save the decision to database for consensus check
            UserDecision::Allow {
                caller_id,
                command,
                glob,
            } => {
                self.permission_controller
                    .add_permission(command, glob)
                    .await?;

                self.permission_tracker
                    .get_mut(&caller_id)
                    .ok_or_else(|| PermissionWorkflowError::MissingCallerId(caller_id.clone()))?
                    .tracker
                    .complete(true);
                Ok(())
            }
            // AllowSession only happens to composite, which can either be
            // allowed command in this session or allow all composite in this session
            // for first case, we do not need to update always flag but simply add to allowlist
            // for second case, we explicitly update the always flag along with the allowlist
            UserDecision::AllowSession {
                session_id,
                caller_id,
            } => {
                let state = self
                    .permission_tracker
                    .get_mut(&caller_id)
                    .ok_or_else(|| PermissionWorkflowError::MissingCallerId(caller_id.clone()))?;
                let command = state.command.join(" ");

                match self.session_permission.get_mut(&session_id) {
                    Some(session) => {
                        session.allowlist.insert(command);
                        state.tracker.complete(true);
                        Ok(())
                    }
                    None => {
                        // Decision arrived without a prior init for this
                        // session — a bug. Deny the tracker so the waiter
                        // unblocks instead of hanging until the timeout.
                        state.tracker.complete(false);
                        Err(PermissionWorkflowError::MissingSession(session_id))
                    }
                }
            }
            UserDecision::IgnorePermission {
                session_id,
                caller_id,
            } => {
                let state = self
                    .permission_tracker
                    .get_mut(&caller_id)
                    .ok_or_else(|| PermissionWorkflowError::MissingCallerId(caller_id.clone()))?;

                match self.session_permission.get_mut(&session_id) {
                    Some(session) => {
                        session.always = true;
                        state.tracker.complete(true);
                        Ok(())
                    }
                    None => {
                        state.tracker.complete(false);
                        Err(PermissionWorkflowError::MissingSession(session_id))
                    }
                }
            }
            UserDecision::Deny { caller_id } => {
                self.permission_tracker
                    .get_mut(&caller_id)
                    .ok_or_else(|| PermissionWorkflowError::MissingCallerId(caller_id.clone()))?
                    .tracker
                    .complete(false);
                Ok(())
            }
        }
    }
}

/// options for AskNoPersist
fn generate_unsafe_decision_options(caller_id: String, session_id: Uuid) -> Vec<UserDecision> {
    vec![
        UserDecision::AllowOnce {
            caller_id: caller_id.clone(),
        },
        UserDecision::AllowSession {
            session_id,
            caller_id: caller_id.clone(),
        },
        UserDecision::IgnorePermission {
            session_id,
            caller_id: caller_id.clone(),
        },
        UserDecision::Deny { caller_id },
    ]
}

fn generate_decision_options(caller_id: String, command_vec: &[String]) -> Vec<UserDecision> {
    let Some(program) = command_vec.first() else {
        return Vec::new();
    };

    // Broadest: the program alone, e.g. "cargo:*".
    let mut options = vec![UserDecision::Allow {
        caller_id: caller_id.clone(),
        command: program.clone(),
        glob: true,
    }];

    // program + subcommand, e.g. "cargo build:*". Capped at two tokens, and
    // only when the 2nd arg is a real subcommand — not an option/flag like
    // "-f" or "--amend".
    if command_vec.get(1).is_some_and(|arg| !arg.starts_with('-')) {
        options.push(UserDecision::Allow {
            caller_id,
            command: command_vec[..2].join(" "),
            glob: true,
        });
    }

    options
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
}

pub type Result<T> = std::result::Result<T, PermissionWorkflowError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    /// A glob `Allow` with caller_id "c" — asserting against it checks all
    /// three fields (caller_id, command, glob) at once.
    fn allow(command: &str) -> UserDecision {
        UserDecision::Allow {
            caller_id: "c".into(),
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
                    caller_id: "c".into(),
                },
                UserDecision::AllowSession {
                    session_id,
                    caller_id: "c".into(),
                },
                UserDecision::IgnorePermission {
                    session_id,
                    caller_id: "c".into(),
                },
                UserDecision::Deny {
                    caller_id: "c".into(),
                },
            ]
        );
    }
}
