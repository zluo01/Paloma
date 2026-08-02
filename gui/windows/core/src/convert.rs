use paloma_binding_protocol::v1 as pb;
use paloma_core::{
    CapabilityFacet, CapabilityInfo, ChatRenderEvent, Connector, ConnectorConnection,
    ExtensionCapabilityId, ExtensionInfo, HealthLevel, HealthStatus, Icon, McpPluginInfo,
    Permission, PermissionState, Plugin, PluginArgs, PluginType, ProviderBackendId, ProviderInfo,
    ProviderStatus, RenderEvent, SearchRenderEvent, SessionListItem, Transport, UserDecision,
};
use tonic::Status;
use uuid::Uuid;

pub fn internal(e: impl std::fmt::Display) -> Status {
    Status::internal(e.to_string())
}

pub fn parse_uuid(raw: &str) -> Result<Uuid, Status> {
    Uuid::parse_str(raw).map_err(|_| Status::invalid_argument("invalid session id"))
}

pub fn render_event(event: RenderEvent) -> pb::RenderEvent {
    let payload = match event {
        RenderEvent::Search(event) => pb::render_event::Payload::Search(search_event(event)),
        RenderEvent::Chat(event) => pb::render_event::Payload::Chat(chat_event(event)),
        RenderEvent::Cancel => pb::render_event::Payload::Cancel(pb::Cancel {}),
        RenderEvent::Done => pb::render_event::Payload::Done(pb::Done {}),
        RenderEvent::Error { message } => pb::render_event::Payload::Error(message),
    };
    pb::RenderEvent {
        payload: Some(payload),
    }
}

fn search_event(event: SearchRenderEvent) -> pb::SearchRenderEvent {
    let SearchRenderEvent::Append { response } = event;
    pb::SearchRenderEvent {
        payload: Some(pb::search_render_event::Payload::Append(
            pb::QueryResponse {
                extension_capability_id: Some(extension_capability_id(
                    response.extension_capability_id,
                )),
                name: response.name,
                items: response.items,
            },
        )),
    }
}

fn chat_event(event: ChatRenderEvent) -> pb::ChatRenderEvent {
    let payload = match event {
        ChatRenderEvent::UserPrompt { text } => pb::chat_render_event::Payload::UserPrompt(text),
        ChatRenderEvent::TextDelta {
            provider_backend_id,
            text,
        } => pb::chat_render_event::Payload::TextDelta(pb::TextDelta {
            provider_backend_id: Some(backend_id(provider_backend_id)),
            text,
        }),
        ChatRenderEvent::ReasoningDelta { text } => {
            pb::chat_render_event::Payload::ReasoningDelta(text)
        },
        ChatRenderEvent::ToolCall {
            tool_name,
            arguments,
            description,
            decisions,
        } => pb::chat_render_event::Payload::ToolCall(pb::ToolCall {
            tool_name,
            arguments,
            description,
            decisions: decisions.into_iter().map(user_decision).collect(),
        }),
    };
    pb::ChatRenderEvent {
        payload: Some(payload),
    }
}

pub fn backend_id(id: ProviderBackendId) -> pb::ProviderBackendId {
    pb::ProviderBackendId {
        provider_id: id.provider_id,
        backend_id: id.backend_id,
    }
}

pub fn backend_id_from(id: Option<pb::ProviderBackendId>) -> Result<ProviderBackendId, Status> {
    let id = id.ok_or_else(|| Status::invalid_argument("missing provider_backend_id"))?;
    Ok(ProviderBackendId {
        provider_id: id.provider_id,
        backend_id: id.backend_id,
    })
}

pub fn extension_capability_id(id: ExtensionCapabilityId) -> pb::ExtensionCapabilityId {
    pb::ExtensionCapabilityId {
        extension_id: id.extension_id,
        capability_id: id.capability_id,
    }
}

pub fn extension_capability_id_from(
    id: Option<pb::ExtensionCapabilityId>,
) -> Result<ExtensionCapabilityId, Status> {
    let id = id.ok_or_else(|| Status::invalid_argument("missing extension_capability_id"))?;
    Ok(ExtensionCapabilityId {
        extension_id: id.extension_id,
        capability_id: id.capability_id,
    })
}

pub fn user_decision(decision: UserDecision) -> pb::UserDecision {
    let decision = match decision {
        UserDecision::AllowOnce { call_id } => {
            pb::user_decision::Decision::AllowOnce(pb::AllowOnce { call_id })
        },
        UserDecision::Allow {
            call_id,
            command,
            glob,
        } => pb::user_decision::Decision::Allow(pb::Allow {
            call_id,
            command,
            glob,
        }),
        UserDecision::AllowSession {
            session_id,
            call_id,
        } => pb::user_decision::Decision::AllowSession(pb::AllowSession {
            session_id: session_id.to_string(),
            call_id,
        }),
        UserDecision::IgnorePermission {
            session_id,
            call_id,
        } => pb::user_decision::Decision::IgnorePermission(pb::IgnorePermission {
            session_id: session_id.to_string(),
            call_id,
        }),
        UserDecision::Deny { call_id } => pb::user_decision::Decision::Deny(pb::Deny { call_id }),
    };
    pb::UserDecision {
        decision: Some(decision),
    }
}

pub fn user_decision_from(decision: Option<pb::UserDecision>) -> Result<UserDecision, Status> {
    let decision = decision
        .and_then(|d| d.decision)
        .ok_or_else(|| Status::invalid_argument("missing user decision"))?;
    Ok(match decision {
        pb::user_decision::Decision::AllowOnce(d) => UserDecision::AllowOnce { call_id: d.call_id },
        pb::user_decision::Decision::Allow(d) => UserDecision::Allow {
            call_id: d.call_id,
            command: d.command,
            glob: d.glob,
        },
        pb::user_decision::Decision::AllowSession(d) => UserDecision::AllowSession {
            session_id: parse_uuid(&d.session_id)?,
            call_id: d.call_id,
        },
        pb::user_decision::Decision::IgnorePermission(d) => UserDecision::IgnorePermission {
            session_id: parse_uuid(&d.session_id)?,
            call_id: d.call_id,
        },
        pb::user_decision::Decision::Deny(d) => UserDecision::Deny { call_id: d.call_id },
    })
}

pub fn permission_state(state: PermissionState) -> pb::PermissionState {
    match state {
        PermissionState::Allow => pb::PermissionState::Allow,
        PermissionState::Deny => pb::PermissionState::Deny,
        PermissionState::Error => pb::PermissionState::Error,
    }
}

pub fn permission(permission: Permission) -> pb::Permission {
    pb::Permission {
        prefix: permission.prefix,
        with_glob: permission.with_glob,
        updated_at: permission.updated_at,
    }
}

pub fn session_item(item: SessionListItem) -> pb::SessionListItem {
    pb::SessionListItem {
        session_id: item.session_id.to_string(),
        title: item.title,
        last_update: item.last_update,
    }
}

pub fn health_status(status: HealthStatus) -> pb::HealthStatus {
    match status {
        HealthStatus::Starting => pb::HealthStatus::Starting,
        HealthStatus::Running => pb::HealthStatus::Running,
        HealthStatus::Unhealthy => pb::HealthStatus::Unhealthy,
    }
}

pub fn health_level(level: HealthLevel) -> pb::HealthLevel {
    match level {
        HealthLevel::Inactive => pb::HealthLevel::Inactive,
        HealthLevel::Healthy => pb::HealthLevel::Healthy,
        HealthLevel::Down => pb::HealthLevel::Down,
        HealthLevel::Degraded => pb::HealthLevel::Degraded,
    }
}

pub fn plugin_type_from(plugin_type: i32) -> Result<PluginType, Status> {
    match pb::PluginType::try_from(plugin_type) {
        Ok(pb::PluginType::Extension) => Ok(PluginType::Extension),
        Ok(pb::PluginType::Provider) => Ok(PluginType::Provider),
        Ok(pb::PluginType::Mcp) => Ok(PluginType::Mcp),
        Err(_) => Err(Status::invalid_argument("unknown plugin type")),
    }
}

pub fn capability_facet_from(facet: i32) -> Result<CapabilityFacet, Status> {
    match pb::CapabilityFacet::try_from(facet) {
        Ok(pb::CapabilityFacet::Search) => Ok(CapabilityFacet::Search),
        Ok(pb::CapabilityFacet::Tool) => Ok(CapabilityFacet::Tool),
        Ok(pb::CapabilityFacet::Mcp) => Ok(CapabilityFacet::Mcp),
        Err(_) => Err(Status::invalid_argument("unknown capability facet")),
    }
}

fn capability_facet(facet: CapabilityFacet) -> pb::CapabilityFacet {
    match facet {
        CapabilityFacet::Search => pb::CapabilityFacet::Search,
        CapabilityFacet::Tool => pb::CapabilityFacet::Tool,
        CapabilityFacet::Mcp => pb::CapabilityFacet::Mcp,
    }
}

fn icon(icon: Icon) -> pb::Icon {
    let icon = match icon {
        Icon::Name(name) => pb::icon::Icon::Name(name),
        Icon::Path(path) => pb::icon::Icon::Path(path),
        Icon::Embedded(bytes) => pb::icon::Icon::Embedded(bytes),
    };
    pb::Icon { icon: Some(icon) }
}

pub fn connector(connector: Connector) -> pb::Connector {
    pb::Connector {
        id: Some(backend_id(connector.id)),
        description: connector.description,
        icon: connector.icon.map(icon),
        connection: connector.connection.map(connector_connection),
    }
}

fn connector_connection(connection: ConnectorConnection) -> pb::ConnectorConnection {
    pb::ConnectorConnection {
        preferred: connection.preferred,
        prefer_model: connection.prefer_model,
        prefer_effort: connection.prefer_effort,
        status: Some(provider_status(connection.status)),
    }
}

fn provider_status(status: ProviderStatus) -> pb::ProviderStatus {
    pb::ProviderStatus {
        models: status.models,
        status: health_status(status.status) as i32,
        error: status.error,
    }
}

fn capability_info(info: CapabilityInfo) -> pb::CapabilityInfo {
    pb::CapabilityInfo {
        id: info.id,
        description: info.description,
        facets: info
            .facets
            .into_iter()
            .map(|(facet, disabled)| pb::FacetState {
                facet: capability_facet(facet) as i32,
                disabled,
            })
            .collect(),
    }
}

pub fn extension_info(info: ExtensionInfo) -> pb::ExtensionInfo {
    pb::ExtensionInfo {
        name: info.name,
        description: info.description,
        author: info.author,
        homepage: info.homepage,
        capabilities: info.capabilities.into_iter().map(capability_info).collect(),
        status: health_status(info.status) as i32,
        error: info.error,
        config: info.config.map(plugin),
    }
}

pub fn provider_info(info: ProviderInfo) -> pb::ProviderInfo {
    pb::ProviderInfo {
        name: info.name,
        description: info.description,
        status: health_status(info.status) as i32,
        error: info.error,
        config: info.config.map(plugin),
    }
}

pub fn mcp_info(info: McpPluginInfo) -> pb::McpPluginInfo {
    pb::McpPluginInfo {
        description: info.description,
        status: health_status(info.status) as i32,
        error: info.error,
        tools: info.tools.into_iter().map(capability_info).collect(),
        config: Some(plugin(info.config)),
    }
}

fn plugin(plugin: Plugin) -> pb::Plugin {
    let transport = match plugin.transport {
        Transport::Local => pb::Transport::Local,
        Transport::Http => pb::Transport::Http,
    };
    let args = match plugin.args {
        PluginArgs::Local { command, args } => {
            pb::plugin::Args::Local(pb::LocalPluginArgs { command, args })
        },
        PluginArgs::Remote { url, requires_auth } => {
            pb::plugin::Args::Remote(pb::RemotePluginArgs { url, requires_auth })
        },
    };
    pb::Plugin {
        name: plugin.name,
        transport: transport as i32,
        timeout: plugin.timeout,
        disabled: plugin.disabled,
        env: plugin.env,
        args: Some(args),
    }
}

pub fn plugin_from(plugin: Option<pb::Plugin>) -> Result<Plugin, Status> {
    let plugin = plugin.ok_or_else(|| Status::invalid_argument("missing plugin config"))?;
    let transport = match pb::Transport::try_from(plugin.transport) {
        Ok(pb::Transport::Local) => Transport::Local,
        Ok(pb::Transport::Http) => Transport::Http,
        Err(_) => return Err(Status::invalid_argument("unknown transport")),
    };
    let args = match plugin
        .args
        .ok_or_else(|| Status::invalid_argument("missing plugin args"))?
    {
        pb::plugin::Args::Local(local) => PluginArgs::Local {
            command: local.command,
            args: local.args,
        },
        pb::plugin::Args::Remote(remote) => PluginArgs::Remote {
            url: remote.url,
            requires_auth: remote.requires_auth,
        },
    };
    Ok(Plugin {
        name: plugin.name,
        transport,
        timeout: plugin.timeout,
        disabled: plugin.disabled,
        env: plugin.env,
        args,
    })
}
