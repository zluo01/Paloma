use std::collections::HashSet;

use scry_core::{AppError, McpServer, Plugin};

#[derive(Default)]
pub(super) struct State {
    pub(super) servers: Vec<McpServer>,
    pub(super) names: HashSet<String>,
}

pub(super) enum Msg {
    ReloadMcpServersRequested,
    McpServersLoaded(Result<Vec<McpServer>, AppError>),
    AddMcpClicked,
    EditMcpClicked(String),
    RemoveMcpClicked(String),
    RemoveMcpFinished(Result<(), AppError>),
    McpToggleChanged(String, bool),
    McpToggleFinished(Result<(), AppError>),
}

#[derive(Debug, PartialEq)]
pub(super) enum Command {
    RenderMcpServers,
    LoadMcpServers,
    SaveMcpToggle(String, bool),
    RemoveMcp(String),
    OpenAddMcpDialog,
    OpenEditMcpDialog(Plugin),
    ShowErrorDialog(String),
    LogWarning(String),
}

impl State {
    /// Plugin page workflow:
    ///
    /// - Page refresh: `McpServersLoaded -> RenderMcpServers`.
    /// - Add/edit/save plugin: `AddMcpClicked` / `EditMcpClicked -> Open...McpDialog -> ReloadMcpServersRequested -> LoadMcpServers`.
    /// - Remove button: `RemoveMcpClicked -> RemoveMcp -> RemoveMcpFinished`.
    /// - Enable switch: `McpToggleChanged -> SaveMcpToggle -> McpToggleFinished`.
    /// - Failed toggle: `McpToggleFinished(Err) -> LoadMcpServers -> ShowErrorDialog`.
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::ReloadMcpServersRequested => vec![Command::LoadMcpServers],
            Msg::McpServersLoaded(result) => match result {
                Ok(servers) => {
                    self.names = servers.iter().map(|s| s.config.name.clone()).collect();
                    self.servers = servers;
                    vec![Command::RenderMcpServers]
                },
                Err(e) => vec![Command::LogWarning(format!("list_mcps failed: {e}"))],
            },
            Msg::AddMcpClicked => vec![Command::OpenAddMcpDialog],
            Msg::EditMcpClicked(name) => match self.config(&name) {
                Some(config) => vec![Command::OpenEditMcpDialog(config)],
                None => vec![Command::LogWarning(format!(
                    "edit requested for unknown plugin: {name}"
                ))],
            },
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

    fn loaded(state: &mut State, servers: Vec<McpServer>) {
        state.update(Msg::McpServersLoaded(Ok(servers)));
    }

    #[test]
    fn reload_requests_a_load() {
        let mut state = State::default();
        assert_eq!(
            state.update(Msg::ReloadMcpServersRequested),
            vec![Command::LoadMcpServers]
        );
    }

    #[test]
    fn loaded_populates_servers_and_names() {
        let mut state = State::default();
        let cmds = state.update(Msg::McpServersLoaded(Ok(vec![server("fs", false)])));

        assert_eq!(cmds, vec![Command::RenderMcpServers]);
        assert_eq!(state.servers.len(), 1);
        assert!(state.names.contains("fs"));
    }

    #[test]
    fn load_failure_only_warns() {
        let mut state = State::default();
        let cmds = state.update(Msg::McpServersLoaded(Err(error("boom"))));
        assert!(matches!(cmds.as_slice(), [Command::LogWarning(_)]));
    }

    #[test]
    fn add_click_opens_the_add_dialog() {
        let mut state = State::default();
        assert_eq!(
            state.update(Msg::AddMcpClicked),
            vec![Command::OpenAddMcpDialog]
        );
    }

    #[test]
    fn edit_click_opens_the_edit_dialog_with_config() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", false)]);
        let cmds = state.update(Msg::EditMcpClicked("fs".into()));
        assert!(matches!(cmds.as_slice(), [Command::OpenEditMcpDialog(p)] if p.name == "fs"));
    }

    #[test]
    fn edit_click_for_unknown_server_warns() {
        let mut state = State::default();
        let cmds = state.update(Msg::EditMcpClicked("ghost".into()));
        assert!(matches!(cmds.as_slice(), [Command::LogWarning(_)]));
    }

    #[test]
    fn remove_click_calls_backend() {
        let mut state = State::default();
        assert_eq!(
            state.update(Msg::RemoveMcpClicked("fs".into())),
            vec![Command::RemoveMcp("fs".into())]
        );
    }

    #[test]
    fn finished_remove_reloads() {
        let mut state = State::default();
        assert_eq!(
            state.update(Msg::RemoveMcpFinished(Ok(()))),
            vec![Command::LoadMcpServers]
        );
    }

    #[test]
    fn failed_remove_shows_error() {
        let mut state = State::default();
        let cmds = state.update(Msg::RemoveMcpFinished(Err(error("busy"))));
        assert!(matches!(cmds.as_slice(), [Command::ShowErrorDialog(_)]));
    }

    #[test]
    fn toggle_request_is_optimistic() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);

        let cmds = state.update(Msg::McpToggleChanged("fs".into(), true));

        assert_eq!(cmds, vec![Command::SaveMcpToggle("fs".into(), true)]);
        assert!(!state.servers[0].config.disabled);
    }

    #[test]
    fn successful_toggle_keeps_optimistic_state() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);
        state.update(Msg::McpToggleChanged("fs".into(), true));

        let cmds = state.update(Msg::McpToggleFinished(Ok(())));

        assert!(cmds.is_empty());
        assert!(!state.servers[0].config.disabled);
    }

    #[test]
    fn failed_toggle_reloads_and_shows_error() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);
        state.update(Msg::McpToggleChanged("fs".into(), true));

        let cmds = state.update(Msg::McpToggleFinished(Err(error("nope"))));

        assert!(matches!(
            cmds.as_slice(),
            [Command::LoadMcpServers, Command::ShowErrorDialog(_)]
        ));
    }
}
