use std::collections::HashSet;

use scry_core::{AppError, McpServer, OAuthCallbackState, Plugin};

#[derive(Default)]
pub(super) struct State {
    pub(super) servers: Vec<McpServer>,
    pub(super) names: HashSet<String>,
}

pub(super) enum Msg {
    McpServersLoaded(Result<Vec<McpServer>, AppError>),
    AddMcpClicked,
    EditMcpClicked(String),
    McpDialogSubmitted {
        config: Plugin,
        editing: bool,
    },
    McpDialogCancelled,
    McpInitFinished {
        config: Plugin,
        // Boxed: the oauth state dwarfs every other variant.
        result: Result<Option<Box<OAuthCallbackState>>, AppError>,
    },
    McpSaveFinished(Result<(), AppError>),
    OauthDialogClosed,
    RemoveMcpClicked(String),
    RemoveMcpFinished(Result<(), AppError>),
    McpToggleChanged(String, bool),
    McpToggleFinished(Result<(), AppError>),
}

pub(super) enum Command {
    RenderMcpServers,
    LoadMcpServers,
    SaveMcpToggle(String, bool),
    RemoveMcp(String),
    OpenMcpDialog {
        config: Option<Plugin>,
        taken: HashSet<String>,
    },
    SaveMcp(Plugin),
    UpdateMcp(Plugin),
    OpenOauthDialog(String),
    FinalizeMcp {
        config: Plugin,
        state: Option<Box<OAuthCallbackState>>,
    },
    CloseOauthDialog,
    AbortMcpConnection,
    CloseMcpDialog,
    ShowMcpDialogError(String),
    ShowErrorDialog(String),
    LogWarning(String),
}

impl State {
    /// Plugin page workflow:
    ///
    /// - Page refresh: `McpServersLoaded -> RenderMcpServers`.
    /// - Add/edit/save plugin: `AddMcpClicked` / `EditMcpClicked -> OpenMcpDialog -> ReloadMcpServersRequested -> LoadMcpServers`.
    /// - Dialog submit: `McpDialogSubmitted -> SaveMcp` (add) or `UpdateMcp` (edit).
    /// - Add flow: `SaveMcp` inits the connection, then
    ///   `McpInitFinished(Ok) -> [OpenOauthDialog ->] FinalizeMcp -> McpSaveFinished`.
    /// - Successful save: `McpSaveFinished(Ok) -> CloseOauthDialog -> CloseMcpDialog -> LoadMcpServers`.
    /// - Failed init or save: `McpInitFinished(Err)` / `McpSaveFinished(Err) -> ShowMcpDialogError`.
    /// - Cancelled authorization: `OauthDialogClosed -> AbortMcpConnection -> ShowMcpDialogError`.
    /// - Dialog cancel: `McpDialogCancelled -> CloseMcpDialog`.
    /// - Remove button: `RemoveMcpClicked -> RemoveMcp -> RemoveMcpFinished`.
    /// - Enable switch: `McpToggleChanged -> SaveMcpToggle -> McpToggleFinished`.
    /// - Failed toggle: `McpToggleFinished(Err) -> LoadMcpServers -> ShowErrorDialog`.
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::McpServersLoaded(result) => match result {
                Ok(servers) => {
                    self.names = servers.iter().map(|s| s.config.name.clone()).collect();
                    self.servers = servers;
                    vec![Command::RenderMcpServers]
                },
                Err(e) => vec![Command::LogWarning(format!("list_mcps failed: {e}"))],
            },
            Msg::AddMcpClicked => vec![Command::OpenMcpDialog {
                config: None,
                taken: self.names.clone(),
            }],
            Msg::EditMcpClicked(name) => match self.config(&name) {
                Some(config) => vec![Command::OpenMcpDialog {
                    config: Some(config),
                    taken: self.names.clone(),
                }],
                None => vec![Command::LogWarning(format!(
                    "edit requested for unknown plugin: {name}"
                ))],
            },
            Msg::McpDialogSubmitted { config, editing } => {
                if editing {
                    vec![Command::UpdateMcp(config)]
                } else {
                    vec![Command::SaveMcp(config)]
                }
            },
            Msg::McpDialogCancelled => vec![Command::CloseMcpDialog],
            Msg::McpInitFinished { config, result } => match result {
                Ok(state) => {
                    let mut commands = Vec::new();
                    if let Some(state) = &state {
                        commands.push(Command::OpenOauthDialog(state.auth_url().to_string()));
                    }
                    commands.push(Command::FinalizeMcp { config, state });
                    commands
                },
                Err(e) => vec![Command::ShowMcpDialogError(format!("{e}"))],
            },
            Msg::McpSaveFinished(result) => match result {
                Ok(()) => vec![
                    Command::CloseOauthDialog,
                    Command::CloseMcpDialog,
                    Command::LoadMcpServers,
                ],
                Err(e) => vec![
                    Command::CloseOauthDialog,
                    Command::ShowMcpDialogError(format!("{e}")),
                ],
            },
            Msg::OauthDialogClosed => vec![
                Command::AbortMcpConnection,
                Command::ShowMcpDialogError("The authorization was cancelled.".into()),
            ],
            Msg::RemoveMcpClicked(name) => vec![Command::RemoveMcp(name)],
            Msg::RemoveMcpFinished(result) => match result {
                Ok(()) => vec![Command::LoadMcpServers],
                Err(e) => vec![Command::ShowErrorDialog(format!("{e}"))],
            },
            Msg::McpToggleChanged(name, enabled) => {
                self.set_enabled(&name, enabled);
                vec![Command::SaveMcpToggle(name, enabled)]
            },
            Msg::McpToggleFinished(result) => match result {
                Ok(()) => vec![],
                Err(e) => vec![
                    Command::LoadMcpServers,
                    Command::ShowErrorDialog(format!("{e}")),
                ],
            },
        }
    }

    fn config(&self, name: &str) -> Option<Plugin> {
        self.servers
            .iter()
            .find(|s| s.config.name == name)
            .map(|s| s.config.clone())
    }

    fn set_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(server) = self.servers.iter_mut().find(|s| s.config.name == name) {
            server.config.disabled = !enabled;
        }
    }
}

#[cfg(test)]
mod tests {
    use scry_core::{HealthStatus, PluginArgs, Transport};

    use super::*;

    fn error(message: &str) -> AppError {
        std::io::Error::other(message).into()
    }

    fn server(name: &str, disabled: bool) -> McpServer {
        McpServer {
            config: Plugin {
                name: name.into(),
                transport: Transport::Http,
                timeout: 300,
                disabled,
                env: Default::default(),
                args: PluginArgs::Remote {
                    url: "https://example.com".into(),
                    requires_auth: false,
                },
            },
            description: String::new(),
            status: HealthStatus::Running,
            error: None,
        }
    }

    /// Walk the model through a completed page load.
    fn loaded(state: &mut State, servers: Vec<McpServer>) {
        let cmds = state.update(Msg::McpServersLoaded(Ok(servers)));
        assert!(matches!(cmds.as_slice(), [Command::RenderMcpServers]));
    }

    /// Submit a new config through the open add dialog, returning the config
    /// the save executor would receive.
    fn submitted(state: &mut State, name: &str) -> Plugin {
        let mut cmds = state.update(Msg::McpDialogSubmitted {
            config: server(name, false).config,
            editing: false,
        });
        let Some(Command::SaveMcp(config)) = cmds.pop() else {
            panic!("submit did not request a save");
        };
        assert!(cmds.is_empty());
        config
    }

    #[test]
    fn refresh_workflow_renders_loaded_servers() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", false)]);
        assert_eq!(state.servers.len(), 1);
        assert!(state.names.contains("fs"));
    }

    #[test]
    fn refresh_workflow_failure_only_warns() {
        let mut state = State::default();
        let cmds = state.update(Msg::McpServersLoaded(Err(error("boom"))));
        assert!(matches!(cmds.as_slice(), [Command::LogWarning(_)]));
        assert!(state.servers.is_empty());
    }

    // The authorized create path is not walked end to end here:
    // `OAuthCallbackState` cannot be built outside the oauth flow.
    #[test]
    fn add_workflow_saves_finalizes_and_reloads() {
        let mut state = State::default();
        loaded(&mut state, vec![server("existing", false)]);

        // The add dialog opens knowing every listed name.
        let cmds = state.update(Msg::AddMcpClicked);
        assert!(matches!(
            cmds.as_slice(),
            [Command::OpenMcpDialog { config: None, taken }] if taken.contains("existing")
        ));

        // Submit inits the connection; no authorization step follows.
        let config = submitted(&mut state, "fs");
        let cmds = state.update(Msg::McpInitFinished {
            config,
            result: Ok(None),
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::FinalizeMcp { config, state: None }] if config.name == "fs"
        ));

        // A finished save closes both dialogs and reloads the list.
        let cmds = state.update(Msg::McpSaveFinished(Ok(())));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::CloseOauthDialog,
                Command::CloseMcpDialog,
                Command::LoadMcpServers
            ]
        ));
        loaded(
            &mut state,
            vec![server("existing", false), server("fs", false)],
        );
        assert!(state.names.contains("fs"));
    }

    #[test]
    fn add_workflow_init_failure_reports_and_allows_a_retry() {
        let mut state = State::default();
        let _ = state.update(Msg::AddMcpClicked);

        let config = submitted(&mut state, "fs");
        let cmds = state.update(Msg::McpInitFinished {
            config,
            result: Err(error("refused")),
        });
        assert!(matches!(cmds.as_slice(), [Command::ShowMcpDialogError(_)]));

        // The dialog stays open with the failure, so a resubmit works.
        submitted(&mut state, "fs");
    }

    #[test]
    fn add_workflow_finalize_failure_reports_and_closes_the_popup() {
        let mut state = State::default();
        let _ = state.update(Msg::AddMcpClicked);
        let config = submitted(&mut state, "fs");
        let _ = state.update(Msg::McpInitFinished {
            config,
            result: Ok(None),
        });

        let cmds = state.update(Msg::McpSaveFinished(Err(error("refused"))));
        assert!(matches!(
            cmds.as_slice(),
            [Command::CloseOauthDialog, Command::ShowMcpDialogError(_)]
        ));
    }

    #[test]
    fn add_workflow_cancelled_authorization_aborts_and_reports() {
        let mut state = State::default();
        let _ = state.update(Msg::AddMcpClicked);
        let _ = submitted(&mut state, "fs");

        let cmds = state.update(Msg::OauthDialogClosed);
        assert!(matches!(
            cmds.as_slice(),
            [Command::AbortMcpConnection, Command::ShowMcpDialogError(_)]
        ));

        // The dialog unlocked with the banner, so a resubmit works.
        submitted(&mut state, "fs");
    }

    #[test]
    fn edit_workflow_updates_and_reloads() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);

        let mut cmds = state.update(Msg::EditMcpClicked("fs".into()));
        let Some(Command::OpenMcpDialog {
            config: Some(config),
            taken,
        }) = cmds.pop()
        else {
            panic!("edit did not open the dialog");
        };
        assert!(taken.contains("fs"));

        let cmds = state.update(Msg::McpDialogSubmitted {
            config,
            editing: true,
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::UpdateMcp(config)] if config.name == "fs" && config.disabled
        ));

        let cmds = state.update(Msg::McpSaveFinished(Ok(())));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::CloseOauthDialog,
                Command::CloseMcpDialog,
                Command::LoadMcpServers
            ]
        ));
    }

    #[test]
    fn edit_workflow_for_an_unknown_server_warns() {
        let mut state = State::default();
        let cmds = state.update(Msg::EditMcpClicked("ghost".into()));
        assert!(matches!(cmds.as_slice(), [Command::LogWarning(_)]));
    }

    #[test]
    fn dialog_cancel_closes_the_dialog() {
        let mut state = State::default();
        let _ = state.update(Msg::AddMcpClicked);
        let cmds = state.update(Msg::McpDialogCancelled);
        assert!(matches!(cmds.as_slice(), [Command::CloseMcpDialog]));
    }

    #[test]
    fn remove_workflow_removes_and_reloads() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", false)]);

        let cmds = state.update(Msg::RemoveMcpClicked("fs".into()));
        assert!(matches!(cmds.as_slice(), [Command::RemoveMcp(name)] if name == "fs"));

        let cmds = state.update(Msg::RemoveMcpFinished(Ok(())));
        assert!(matches!(cmds.as_slice(), [Command::LoadMcpServers]));
        loaded(&mut state, vec![]);
        assert!(!state.names.contains("fs"));
    }

    #[test]
    fn remove_workflow_failure_shows_the_page_error() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", false)]);
        let _ = state.update(Msg::RemoveMcpClicked("fs".into()));

        let cmds = state.update(Msg::RemoveMcpFinished(Err(error("busy"))));
        assert!(matches!(cmds.as_slice(), [Command::ShowErrorDialog(_)]));
    }

    #[test]
    fn toggle_workflow_is_optimistic_and_persists() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);

        let cmds = state.update(Msg::McpToggleChanged("fs".into(), true));
        assert!(matches!(
            cmds.as_slice(),
            [Command::SaveMcpToggle(name, true)] if name == "fs"
        ));
        assert!(!state.servers[0].config.disabled);

        assert!(state.update(Msg::McpToggleFinished(Ok(()))).is_empty());
        assert!(!state.servers[0].config.disabled);
    }

    #[test]
    fn toggle_workflow_failure_reloads_and_reports() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);
        let _ = state.update(Msg::McpToggleChanged("fs".into(), true));

        let cmds = state.update(Msg::McpToggleFinished(Err(error("nope"))));
        assert!(matches!(
            cmds.as_slice(),
            [Command::LoadMcpServers, Command::ShowErrorDialog(_)]
        ));
    }
}
