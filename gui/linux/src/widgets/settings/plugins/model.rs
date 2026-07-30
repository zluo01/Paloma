use std::collections::HashSet;

use scry_core::{
    AppError, CapabilityFacet, ExtensionInfo, McpPluginInfo, OAuthCallbackState, Plugin,
    PluginType, ProviderInfo,
};

#[derive(Default)]
pub(super) struct State {
    pub(super) extensions: Vec<ExtensionInfo>,
    pub(super) providers: Vec<ProviderInfo>,
    pub(super) servers: Vec<McpPluginInfo>,
}

pub(super) enum Msg {
    General(GeneralPluginMsg),
    McpPlugin(McpPluginMsg),
    ExtensionLoaded(Result<Vec<ExtensionInfo>, AppError>),
    ProviderLoaded(Result<Vec<ProviderInfo>, AppError>),
    McpServersLoaded(Result<Vec<McpPluginInfo>, AppError>),
}

pub(super) enum GeneralPluginMsg {
    EditPluginClicked(PluginType, String),
    RemovePluginClicked(PluginType, String),
    RemovePluginFinished(PluginType, Result<(), AppError>),
    AddPluginClicked(PluginType),
    PluginDialogSubmitted {
        plugin_type: PluginType,
        config: Plugin,
        editing: bool,
    },
    ToggleSwitch(PluginType, String, bool),
    ToggleCapability {
        plugin_type: PluginType,
        plugin: String,
        capability: String,
        facet: CapabilityFacet,
        enabled: bool,
    },
    SwitchToggledFinish(PluginType, Result<(), AppError>),
    PluginSaveFinished(PluginType, Result<(), AppError>),
    PluginDialogCancelled,
}

pub(super) enum McpPluginMsg {
    McpInitFinished {
        config: Plugin,
        // Boxed: the oauth state dwarfs every other variant.
        result: Result<Option<Box<OAuthCallbackState>>, AppError>,
    },
    OauthDialogClosed,
}

pub(super) enum Command {
    LoadExtensions,
    LoadProviderPlugins,
    LoadMcpServers,
    RenderExtensions,
    RenderProviderPlugins,
    RenderMcpServers,
    TogglePlugin(PluginType, String, bool),
    ToggleCapability {
        plugin_type: PluginType,
        plugin: String,
        capability: String,
        facet: CapabilityFacet,
        enabled: bool,
    },
    RemovePlugin(PluginType, String),
    OpenPluginDialog {
        plugin_type: PluginType,
        config: Option<Plugin>,
        taken: HashSet<String>,
    },
    SavePlugin(PluginType, Plugin),
    UpdatePlugin(PluginType, Plugin),
    OpenOauthDialog(String),
    FinalizeMcp {
        config: Plugin,
        state: Option<Box<OAuthCallbackState>>,
    },
    CloseOauthDialog,
    AbortMcpConnection,
    ClosePluginDialog,
    ShowPluginDialogError(String),
    ShowErrorDialog(String),
    LogWarning(String),
}

impl State {
    /// Plugin page workflow:
    ///
    /// - Page refresh: `McpServersLoaded -> RenderMcpServers` and `ProviderLoaded -> RenderProviderPlugins`.
    /// - Add/edit plugin: `General(AddPluginClicked)` / `General(EditPluginClicked) -> OpenPluginDialog`.
    /// - Dialog submit: `General(PluginDialogSubmitted) -> SavePlugin` (add) or `UpdatePlugin` (edit).
    /// - Mcp add: `SavePlugin(Mcp) -> McpPlugin(McpInitFinished(Ok)) -> [OpenOauthDialog ->] FinalizeMcp -> PluginSaveFinished`.
    /// - Provider add: `SavePlugin(Provider) -> PluginSaveFinished`.
    /// - Successful save: `PluginSaveFinished(Ok) -> [CloseOauthDialog ->] ClosePluginDialog -> LoadMcpServers`/`LoadProviderPlugins`.
    /// - Failed init or save: `McpPlugin(McpInitFinished(Err))` / `PluginSaveFinished(Err) -> ShowPluginDialogError`.
    /// - Canceled authorization: `McpPlugin(OauthDialogClosed) -> AbortMcpConnection -> ShowPluginDialogError`.
    /// - Dialog cancel: `General(PluginDialogCancelled) -> ClosePluginDialog`.
    /// - Enable switch: `General(ToggleSwitch) -> TogglePlugin -> General(SwitchToggledFinish)`.
    /// - Capability chip: `General(ToggleCapability) -> ToggleCapability -> General(SwitchToggledFinish)`.
    /// - Failed toggle: `SwitchToggledFinish(Err) -> LoadMcpServers`/`LoadExtensions -> ShowErrorDialog`.
    /// - Remove button: `General(RemovePluginClicked) -> RemovePlugin -> RemovePluginFinished(Ok) -> LoadMcpServers`/`LoadProviderPlugins`.
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::General(msg) => self.handle_general(msg),
            Msg::McpPlugin(msg) => self.handle_mcp_connect(msg),
            Msg::ExtensionLoaded(result) => match result {
                Ok(extensions) => {
                    self.extensions = extensions;
                    vec![Command::RenderExtensions]
                },
                Err(e) => vec![Command::LogWarning(format!(
                    "failed to load extensions: {e}"
                ))],
            },
            Msg::ProviderLoaded(result) => match result {
                Ok(providers) => {
                    self.providers = providers;
                    vec![Command::RenderProviderPlugins]
                },
                Err(e) => vec![Command::LogWarning(format!(
                    "failed to load providers: {e}"
                ))],
            },
            Msg::McpServersLoaded(result) => match result {
                Ok(servers) => {
                    self.servers = servers;
                    vec![Command::RenderMcpServers]
                },
                Err(e) => vec![Command::LogWarning(format!("list_mcps failed: {e}"))],
            },
        }
    }

    fn handle_general(&mut self, msg: GeneralPluginMsg) -> Vec<Command> {
        match msg {
            GeneralPluginMsg::EditPluginClicked(plugin_type, name) => {
                match self.config(&plugin_type, &name) {
                    Some(config) => vec![Command::OpenPluginDialog {
                        plugin_type,
                        config: Some(config),
                        taken: self.taken_names(),
                    }],
                    None => vec![Command::LogWarning(format!(
                        "edit requested for unknown plugin: {name}"
                    ))],
                }
            },
            GeneralPluginMsg::RemovePluginClicked(plugin_type, name) => {
                vec![Command::RemovePlugin(plugin_type, name)]
            },
            GeneralPluginMsg::RemovePluginFinished(plugin_type, result) => match result {
                Ok(()) => reload_list(&plugin_type).into_iter().collect(),
                Err(e) => vec![Command::ShowErrorDialog(format!("{e}"))],
            },
            GeneralPluginMsg::AddPluginClicked(plugin_type) => vec![Command::OpenPluginDialog {
                plugin_type,
                config: None,
                taken: self.taken_names(),
            }],
            GeneralPluginMsg::PluginSaveFinished(plugin_type, result) => {
                let mut commands = Vec::new();
                // Only the MCP flow can have the OAuth popup open.
                if plugin_type == PluginType::Mcp {
                    commands.push(Command::CloseOauthDialog);
                }
                match result {
                    Ok(()) => {
                        commands.push(Command::ClosePluginDialog);
                        commands.extend(reload_list(&plugin_type));
                    },
                    Err(e) => commands.push(Command::ShowPluginDialogError(format!("{e}"))),
                }
                commands
            },
            GeneralPluginMsg::PluginDialogCancelled => vec![Command::ClosePluginDialog],
            GeneralPluginMsg::PluginDialogSubmitted {
                plugin_type,
                config,
                editing,
            } => {
                if editing {
                    vec![Command::UpdatePlugin(plugin_type, config)]
                } else {
                    vec![Command::SavePlugin(plugin_type, config)]
                }
            },
            GeneralPluginMsg::ToggleSwitch(plugin_type, name, enabled) => {
                self.set_enabled(&plugin_type, &name, enabled);
                vec![Command::TogglePlugin(plugin_type, name, enabled)]
            },
            GeneralPluginMsg::ToggleCapability {
                plugin_type,
                plugin,
                capability,
                facet,
                enabled,
            } => {
                self.set_capability_enabled(&plugin_type, &plugin, &capability, facet, enabled);
                vec![Command::ToggleCapability {
                    plugin_type,
                    plugin,
                    capability,
                    facet,
                    enabled,
                }]
            },
            GeneralPluginMsg::SwitchToggledFinish(plugin_type, result) => match result {
                Ok(()) => vec![],
                Err(e) => reload_list(&plugin_type)
                    .into_iter()
                    .chain([Command::ShowErrorDialog(format!("{e}"))])
                    .collect(),
            },
        }
    }

    fn handle_mcp_connect(&mut self, msg: McpPluginMsg) -> Vec<Command> {
        match msg {
            McpPluginMsg::McpInitFinished { config, result } => match result {
                Ok(state) => {
                    let mut commands = Vec::new();
                    if let Some(state) = &state {
                        commands.push(Command::OpenOauthDialog(state.auth_url().to_string()));
                    }
                    commands.push(Command::FinalizeMcp { config, state });
                    commands
                },
                Err(e) => vec![Command::ShowPluginDialogError(format!("{e}"))],
            },
            McpPluginMsg::OauthDialogClosed => vec![
                Command::AbortMcpConnection,
                Command::ShowPluginDialogError("The authorization was cancelled.".into()),
            ],
        }
    }

    fn taken_names(&self) -> HashSet<String> {
        self.servers
            .iter()
            .map(|s| s.config.name.clone())
            .chain(self.providers.iter().map(|p| p.name.clone()))
            .chain(self.extensions.iter().map(|e| e.name.clone()))
            .collect()
    }

    fn config(&self, plugin_type: &PluginType, name: &str) -> Option<Plugin> {
        match plugin_type {
            PluginType::Mcp => self
                .servers
                .iter()
                .find(|s| s.config.name == name)
                .map(|s| s.config.clone()),
            PluginType::Provider => self
                .providers
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.config.clone()),
            PluginType::Extension => self
                .extensions
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.config.clone()),
        }
    }

    fn set_capability_enabled(
        &mut self,
        plugin_type: &PluginType,
        plugin: &str,
        capability: &str,
        facet: CapabilityFacet,
        enabled: bool,
    ) {
        let capabilities = match plugin_type {
            PluginType::Extension => self
                .extensions
                .iter_mut()
                .find(|e| e.name == plugin)
                .map(|e| &mut e.capabilities),
            PluginType::Mcp => self
                .servers
                .iter_mut()
                .find(|s| s.config.name == plugin)
                .map(|s| &mut s.tools),
            PluginType::Provider => None,
        };
        if let Some(info) = capabilities
            .and_then(|capabilities| capabilities.iter_mut().find(|c| c.id == capability))
            && let Some(entry) = info.facets.iter_mut().find(|(f, _)| *f == facet)
        {
            entry.1 = !enabled;
        }
    }

    fn set_enabled(&mut self, plugin_type: &PluginType, name: &str, enabled: bool) {
        match plugin_type {
            PluginType::Extension => {
                if let Some(config) = self
                    .extensions
                    .iter_mut()
                    .find(|s| s.name == name)
                    .and_then(|s| s.config.as_mut())
                {
                    config.disabled = !enabled;
                }
            },
            PluginType::Provider => {},
            PluginType::Mcp => {
                if let Some(server) = self.servers.iter_mut().find(|s| s.config.name == name) {
                    server.config.disabled = !enabled;
                }
            },
        }
    }
}

fn reload_list(plugin_type: &PluginType) -> Option<Command> {
    match plugin_type {
        PluginType::Provider => Some(Command::LoadProviderPlugins),
        PluginType::Mcp => Some(Command::LoadMcpServers),
        PluginType::Extension => Some(Command::LoadExtensions),
    }
}

#[cfg(test)]
mod tests {
    use scry_core::{CapabilityInfo, HealthStatus, PluginArgs, Transport};

    use super::*;

    fn error(message: &str) -> AppError {
        std::io::Error::other(message).into()
    }

    fn server(name: &str, disabled: bool) -> McpPluginInfo {
        McpPluginInfo {
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
            tools: vec![],
        }
    }

    fn extension(name: &str, disabled: bool) -> ExtensionInfo {
        ExtensionInfo {
            name: name.into(),
            description: String::new(),
            author: None,
            homepage: None,
            capabilities: vec![],
            status: HealthStatus::Running,
            error: None,
            config: Some(Plugin {
                name: name.into(),
                transport: Transport::Local,
                timeout: 300,
                disabled,
                env: Default::default(),
                args: PluginArgs::Local {
                    command: "extension-bin".into(),
                    args: Vec::new(),
                },
            }),
        }
    }

    fn provider(name: &str) -> ProviderInfo {
        ProviderInfo {
            name: name.into(),
            description: String::new(),
            status: HealthStatus::Running,
            error: None,
            config: Some(Plugin {
                name: name.into(),
                transport: Transport::Local,
                timeout: 300,
                disabled: false,
                env: Default::default(),
                args: PluginArgs::Local {
                    command: "provider-bin".into(),
                    args: Vec::new(),
                },
            }),
        }
    }

    /// Walk the model through a completed page load.
    fn loaded(state: &mut State, servers: Vec<McpPluginInfo>) {
        let cmds = state.update(Msg::McpServersLoaded(Ok(servers)));
        assert!(matches!(cmds.as_slice(), [Command::RenderMcpServers]));
    }

    /// Submit a new config through the open add dialog, returning the config
    /// the save executor would receive.
    fn submitted(state: &mut State, name: &str) -> Plugin {
        let mut cmds = state.update(Msg::General(GeneralPluginMsg::PluginDialogSubmitted {
            plugin_type: PluginType::Mcp,
            config: server(name, false).config,
            editing: false,
        }));
        let Some(Command::SavePlugin(PluginType::Mcp, config)) = cmds.pop() else {
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
        assert!(state.taken_names().contains("fs"));
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
        let cmds = state.update(Msg::General(GeneralPluginMsg::AddPluginClicked(
            PluginType::Mcp,
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::OpenPluginDialog {
                plugin_type: PluginType::Mcp,
                config: None,
                taken,
            }] if taken.contains("existing")
        ));

        // Submit inits the connection; no authorization step follows.
        let config = submitted(&mut state, "fs");
        let cmds = state.update(Msg::McpPlugin(McpPluginMsg::McpInitFinished {
            config,
            result: Ok(None),
        }));
        assert!(matches!(
            cmds.as_slice(),
            [Command::FinalizeMcp { config, .. }] if config.name == "fs"
        ));

        // A finished save closes both dialogs and reloads the list.
        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginSaveFinished(
            PluginType::Mcp,
            Ok(()),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::CloseOauthDialog,
                Command::ClosePluginDialog,
                Command::LoadMcpServers
            ]
        ));
        loaded(
            &mut state,
            vec![server("existing", false), server("fs", false)],
        );
        assert!(state.taken_names().contains("fs"));
    }

    #[test]
    fn add_workflow_init_failure_reports_and_allows_a_retry() {
        let mut state = State::default();
        let _ = state.update(Msg::General(GeneralPluginMsg::AddPluginClicked(
            PluginType::Mcp,
        )));

        let config = submitted(&mut state, "fs");
        let cmds = state.update(Msg::McpPlugin(McpPluginMsg::McpInitFinished {
            config,
            result: Err(error("refused")),
        }));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowPluginDialogError(_)]
        ));

        // The dialog stays open with the failure, so a resubmit works.
        submitted(&mut state, "fs");
    }

    #[test]
    fn add_workflow_finalize_failure_reports_and_closes_the_popup() {
        let mut state = State::default();
        let _ = state.update(Msg::General(GeneralPluginMsg::AddPluginClicked(
            PluginType::Mcp,
        )));
        let config = submitted(&mut state, "fs");
        let _ = state.update(Msg::McpPlugin(McpPluginMsg::McpInitFinished {
            config,
            result: Ok(None),
        }));

        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginSaveFinished(
            PluginType::Mcp,
            Err(error("refused")),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::CloseOauthDialog, Command::ShowPluginDialogError(_)]
        ));
    }

    #[test]
    fn add_workflow_cancelled_authorization_aborts_and_reports() {
        let mut state = State::default();
        let _ = state.update(Msg::General(GeneralPluginMsg::AddPluginClicked(
            PluginType::Mcp,
        )));
        let _ = submitted(&mut state, "fs");

        let cmds = state.update(Msg::McpPlugin(McpPluginMsg::OauthDialogClosed));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::AbortMcpConnection,
                Command::ShowPluginDialogError(_)
            ]
        ));

        // The dialog unlocked with the banner, so a resubmit works.
        submitted(&mut state, "fs");
    }

    #[test]
    fn edit_workflow_updates_and_reloads() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);

        let mut cmds = state.update(Msg::General(GeneralPluginMsg::EditPluginClicked(
            PluginType::Mcp,
            "fs".into(),
        )));
        let Some(Command::OpenPluginDialog {
            plugin_type: PluginType::Mcp,
            config: Some(config),
            taken,
        }) = cmds.pop()
        else {
            panic!("edit did not open the dialog");
        };
        assert!(taken.contains("fs"));

        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginDialogSubmitted {
            plugin_type: PluginType::Mcp,
            config,
            editing: true,
        }));
        assert!(matches!(
            cmds.as_slice(),
            [Command::UpdatePlugin(PluginType::Mcp, config)] if config.name == "fs" && config.disabled
        ));

        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginSaveFinished(
            PluginType::Mcp,
            Ok(()),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::CloseOauthDialog,
                Command::ClosePluginDialog,
                Command::LoadMcpServers
            ]
        ));
    }

    #[test]
    fn edit_workflow_for_an_unknown_server_warns() {
        let mut state = State::default();
        let cmds = state.update(Msg::General(GeneralPluginMsg::EditPluginClicked(
            PluginType::Mcp,
            "ghost".into(),
        )));
        assert!(matches!(cmds.as_slice(), [Command::LogWarning(_)]));
    }

    #[test]
    fn dialog_cancel_closes_the_dialog() {
        let mut state = State::default();
        let _ = state.update(Msg::General(GeneralPluginMsg::AddPluginClicked(
            PluginType::Mcp,
        )));
        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginDialogCancelled));
        assert!(matches!(cmds.as_slice(), [Command::ClosePluginDialog]));
    }

    #[test]
    fn provider_add_workflow_saves_and_reloads() {
        let mut state = State::default();

        let cmds = state.update(Msg::General(GeneralPluginMsg::AddPluginClicked(
            PluginType::Provider,
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::OpenPluginDialog {
                plugin_type: PluginType::Provider,
                config: None,
                ..
            }]
        ));

        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginDialogSubmitted {
            plugin_type: PluginType::Provider,
            config: provider("openai").config.unwrap(),
            editing: false,
        }));
        assert!(matches!(
            cmds.as_slice(),
            [Command::SavePlugin(PluginType::Provider, _)]
        ));

        // A provider save is single-phase: no oauth dialog to close, and only
        // the provider list reloads.
        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginSaveFinished(
            PluginType::Provider,
            Ok(()),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ClosePluginDialog, Command::LoadProviderPlugins]
        ));
    }

    #[test]
    fn provider_save_failure_shows_the_dialog_error() {
        let mut state = State::default();
        let _ = state.update(Msg::General(GeneralPluginMsg::AddPluginClicked(
            PluginType::Provider,
        )));

        let cmds = state.update(Msg::General(GeneralPluginMsg::PluginSaveFinished(
            PluginType::Provider,
            Err(error("handshake refused")),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowPluginDialogError(_)]
        ));
    }

    #[test]
    fn provider_edit_workflow_opens_the_dialog() {
        let mut state = State::default();
        let cmds = state.update(Msg::ProviderLoaded(Ok(vec![provider("openai")])));
        assert!(matches!(cmds.as_slice(), [Command::RenderProviderPlugins]));

        let cmds = state.update(Msg::General(GeneralPluginMsg::EditPluginClicked(
            PluginType::Provider,
            "openai".into(),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::OpenPluginDialog {
                plugin_type: PluginType::Provider,
                config: Some(config),
                taken,
            }] if config.name == "openai" && taken.contains("openai")
        ));
    }

    #[test]
    fn remove_workflow_removes_and_reloads() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", false)]);

        let cmds = state.update(Msg::General(GeneralPluginMsg::RemovePluginClicked(
            PluginType::Mcp,
            "fs".into(),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::RemovePlugin(PluginType::Mcp, name)] if name == "fs"
        ));

        let cmds = state.update(Msg::General(GeneralPluginMsg::RemovePluginFinished(
            PluginType::Mcp,
            Ok(()),
        )));
        assert!(matches!(cmds.as_slice(), [Command::LoadMcpServers]));

        // A removed provider reloads only the provider list.
        let cmds = state.update(Msg::General(GeneralPluginMsg::RemovePluginFinished(
            PluginType::Provider,
            Ok(()),
        )));
        assert!(matches!(cmds.as_slice(), [Command::LoadProviderPlugins]));
        loaded(&mut state, vec![]);
        assert!(!state.taken_names().contains("fs"));
    }

    #[test]
    fn remove_workflow_failure_shows_the_page_error() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", false)]);
        let _ = state.update(Msg::General(GeneralPluginMsg::RemovePluginClicked(
            PluginType::Mcp,
            "fs".into(),
        )));

        let cmds = state.update(Msg::General(GeneralPluginMsg::RemovePluginFinished(
            PluginType::Mcp,
            Err(error("busy")),
        )));
        assert!(matches!(cmds.as_slice(), [Command::ShowErrorDialog(_)]));
    }

    #[test]
    fn toggle_workflow_is_optimistic_and_persists() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);

        let cmds = state.update(Msg::General(GeneralPluginMsg::ToggleSwitch(
            PluginType::Mcp,
            "fs".into(),
            true,
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::TogglePlugin(PluginType::Mcp, name, true)] if name == "fs"
        ));
        assert!(!state.servers[0].config.disabled);

        let cmds = state.update(Msg::General(GeneralPluginMsg::SwitchToggledFinish(
            PluginType::Mcp,
            Ok(()),
        )));
        assert!(cmds.is_empty());
        assert!(!state.servers[0].config.disabled);
    }

    #[test]
    fn toggle_workflow_failure_reloads_and_reports() {
        let mut state = State::default();
        loaded(&mut state, vec![server("fs", true)]);
        let _ = state.update(Msg::General(GeneralPluginMsg::ToggleSwitch(
            PluginType::Mcp,
            "fs".into(),
            true,
        )));

        let cmds = state.update(Msg::General(GeneralPluginMsg::SwitchToggledFinish(
            PluginType::Mcp,
            Err(error("nope")),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::LoadMcpServers, Command::ShowErrorDialog(_)]
        ));
    }

    #[test]
    fn extension_toggle_is_optimistic_and_failure_reloads_extensions() {
        let mut state = State::default();
        let cmds = state.update(Msg::ExtensionLoaded(Ok(vec![extension("files", true)])));
        assert!(matches!(cmds.as_slice(), [Command::RenderExtensions]));

        let cmds = state.update(Msg::General(GeneralPluginMsg::ToggleSwitch(
            PluginType::Extension,
            "files".into(),
            true,
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::TogglePlugin(PluginType::Extension, name, true)] if name == "files"
        ));
        assert!(!state.extensions[0].config.as_ref().unwrap().disabled);

        let cmds = state.update(Msg::General(GeneralPluginMsg::SwitchToggledFinish(
            PluginType::Extension,
            Err(error("nope")),
        )));
        assert!(matches!(
            cmds.as_slice(),
            [Command::LoadExtensions, Command::ShowErrorDialog(_)]
        ));
    }

    #[test]
    fn taken_names_include_extensions() {
        let mut state = State::default();
        let _ = state.update(Msg::ExtensionLoaded(Ok(vec![extension("files", false)])));

        assert!(state.taken_names().contains("files"));
    }

    fn capability(id: &str, facets: Vec<(CapabilityFacet, bool)>) -> CapabilityInfo {
        CapabilityInfo {
            id: id.into(),
            description: String::new(),
            facets,
        }
    }

    fn facet_flag(info: &CapabilityInfo, facet: CapabilityFacet) -> bool {
        info.facets.iter().find(|(f, _)| *f == facet).unwrap().1
    }

    #[test]
    fn capability_toggle_is_optimistic_and_flips_only_its_facet() {
        let mut state = State::default();
        let mut ext = extension("files", false);
        ext.capabilities = vec![capability(
            "Files",
            vec![
                (CapabilityFacet::Search, false),
                (CapabilityFacet::Tool, false),
            ],
        )];
        let _ = state.update(Msg::ExtensionLoaded(Ok(vec![ext])));

        let cmds = state.update(Msg::General(GeneralPluginMsg::ToggleCapability {
            plugin_type: PluginType::Extension,
            plugin: "files".into(),
            capability: "Files".into(),
            facet: CapabilityFacet::Tool,
            enabled: false,
        }));

        assert!(matches!(
            cmds.as_slice(),
            [Command::ToggleCapability {
                capability,
                facet: CapabilityFacet::Tool,
                enabled: false,
                ..
            }] if capability == "Files"
        ));
        let info = &state.extensions[0].capabilities[0];
        assert!(facet_flag(info, CapabilityFacet::Tool));
        assert!(!facet_flag(info, CapabilityFacet::Search));
    }

    #[test]
    fn mcp_tool_toggle_updates_the_server_tool() {
        let mut state = State::default();
        let mut mcp = server("fs", false);
        mcp.tools = vec![capability("read_file", vec![(CapabilityFacet::Mcp, true)])];
        loaded(&mut state, vec![mcp]);

        let _ = state.update(Msg::General(GeneralPluginMsg::ToggleCapability {
            plugin_type: PluginType::Mcp,
            plugin: "fs".into(),
            capability: "read_file".into(),
            facet: CapabilityFacet::Mcp,
            enabled: true,
        }));

        assert!(!facet_flag(
            &state.servers[0].tools[0],
            CapabilityFacet::Mcp
        ));
    }
}
