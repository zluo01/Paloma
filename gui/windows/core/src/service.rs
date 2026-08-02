use std::sync::Arc;

use dashmap::DashMap;
use futures::{StreamExt, stream::BoxStream};
use paloma_binding_protocol::v1::{self as pb, binding_server::Binding};
use paloma_core::{AppContext, OAuthCallbackState, ProviderAuthMethod};
use paloma_extension_protocol::v1::RunActionResponse;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::convert;

pub struct BindingService {
    app: Arc<AppContext>,
    oauth_sessions: DashMap<String, OAuthCallbackState>,
}

impl BindingService {
    pub fn new(app: Arc<AppContext>) -> Self {
        Self {
            app,
            oauth_sessions: DashMap::new(),
        }
    }
}

type EventStream = BoxStream<'static, Result<pb::RenderEvent, Status>>;

#[tonic::async_trait]
impl Binding for BindingService {
    type SearchStream = EventStream;
    async fn search(
        &self,
        request: Request<pb::SearchRequest>,
    ) -> Result<Response<Self::SearchStream>, Status> {
        let input = request.into_inner().input;
        let stream: Self::SearchStream = self
            .app
            .search(&input)
            .await
            .map(|event| Ok(convert::render_event(event)))
            .boxed();
        Ok(Response::new(stream))
    }

    async fn run_search_action(
        &self,
        request: Request<pb::RunSearchActionRequest>,
    ) -> Result<Response<pb::RunSearchActionResponse>, Status> {
        let request = request.into_inner();
        let id = convert::extension_capability_id_from(request.extension_capability_id)?;
        let action = request
            .action
            .ok_or_else(|| Status::invalid_argument("missing action"))?;
        let behavior = self
            .app
            .run_search_action(id, action)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::RunSearchActionResponse {
            behavior: Some(RunActionResponse {
                behavior: Some(behavior),
            }),
        }))
    }

    type ChatStream = BoxStream<'static, Result<pb::ChatEvent, Status>>;

    async fn chat(
        &self,
        request: Request<pb::ChatRequest>,
    ) -> Result<Response<Self::ChatStream>, Status> {
        let request = request.into_inner();
        let session_id = request
            .session_id
            .as_deref()
            .map(convert::parse_uuid)
            .transpose()?;
        let backend = convert::backend_id_from(request.provider_backend_id)?;
        let chat = self.app.chat(session_id, backend, request.prompt).await;
        let started = chat.session_id.map(|id| {
            Ok(pb::ChatEvent {
                payload: Some(pb::chat_event::Payload::SessionStarted(id.to_string())),
            })
        });
        let events = chat.stream.map(|event| {
            Ok(pb::ChatEvent {
                payload: Some(pb::chat_event::Payload::Event(convert::render_event(event))),
            })
        });
        let stream: Self::ChatStream = futures::stream::iter(started).chain(events).boxed();
        Ok(Response::new(stream))
    }

    type RestoreSessionStream = EventStream;

    async fn restore_session(
        &self,
        request: Request<pb::RestoreSessionRequest>,
    ) -> Result<Response<Self::RestoreSessionStream>, Status> {
        let session_id = convert::parse_uuid(&request.into_inner().session_id)?;
        let stream = self
            .app
            .restore_session(session_id)
            .await
            .map_err(convert::internal)?;
        let stream: Self::RestoreSessionStream =
            stream.map(|event| Ok(convert::render_event(event))).boxed();
        Ok(Response::new(stream))
    }

    async fn available_sessions(
        &self,
        _request: Request<pb::AvailableSessionsRequest>,
    ) -> Result<Response<pb::AvailableSessionsResponse>, Status> {
        let sessions = self
            .app
            .available_sessions()
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::AvailableSessionsResponse {
            sessions: sessions.into_iter().map(convert::session_item).collect(),
        }))
    }

    async fn search_sessions(
        &self,
        request: Request<pb::SearchSessionsRequest>,
    ) -> Result<Response<pb::SearchSessionsResponse>, Status> {
        let ids = self
            .app
            .search_sessions(request.into_inner().needle)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::SearchSessionsResponse {
            session_ids: ids.iter().map(Uuid::to_string).collect(),
        }))
    }

    async fn remove_session(
        &self,
        request: Request<pb::RemoveSessionRequest>,
    ) -> Result<Response<pb::RemoveSessionResponse>, Status> {
        let session_id = convert::parse_uuid(&request.into_inner().session_id)?;
        self.app
            .remove_session(session_id)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::RemoveSessionResponse {}))
    }

    async fn cancel_session(
        &self,
        request: Request<pb::CancelSessionRequest>,
    ) -> Result<Response<pb::CancelSessionResponse>, Status> {
        let session_id = convert::parse_uuid(&request.into_inner().session_id)?;
        self.app
            .cancel_session(session_id)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::CancelSessionResponse {}))
    }

    async fn decide_toolcall_permissions(
        &self,
        request: Request<pb::DecideToolcallPermissionsRequest>,
    ) -> Result<Response<pb::DecideToolcallPermissionsResponse>, Status> {
        let decision = convert::user_decision_from(request.into_inner().user_decision)?;
        let state = self
            .app
            .decide_toolcall_permissions(decision)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::DecideToolcallPermissionsResponse {
            state: convert::permission_state(state) as i32,
        }))
    }

    async fn get_permissions(
        &self,
        _request: Request<pb::GetPermissionsRequest>,
    ) -> Result<Response<pb::GetPermissionsResponse>, Status> {
        let permissions = self
            .app
            .get_permissions()
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::GetPermissionsResponse {
            permissions: permissions.into_iter().map(convert::permission).collect(),
        }))
    }

    async fn delete_permission(
        &self,
        request: Request<pb::DeletePermissionRequest>,
    ) -> Result<Response<pb::DeletePermissionResponse>, Status> {
        self.app
            .delete_permission(&request.into_inner().prefix)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::DeletePermissionResponse {}))
    }

    async fn init_connection(
        &self,
        request: Request<pb::InitConnectionRequest>,
    ) -> Result<Response<pb::InitConnectionResponse>, Status> {
        let backend = convert::backend_id_from(request.into_inner().provider_backend_id)?;
        let connection = self
            .app
            .init_connection(backend)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::InitConnectionResponse {
            connection: Some(connection),
        }))
    }

    async fn finalize_connection(
        &self,
        request: Request<pb::FinalizeConnectionRequest>,
    ) -> Result<Response<pb::FinalizeConnectionResponse>, Status> {
        let request = request.into_inner();
        let method = ProviderAuthMethod::try_from(request.provider_auth_method)
            .map_err(|_| Status::invalid_argument("unknown provider auth method"))?;
        let backend = convert::backend_id_from(request.provider_backend_id)?;
        self.app
            .finalize_connection(method, backend, request.payload)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::FinalizeConnectionResponse {}))
    }

    async fn cancel_connection(
        &self,
        request: Request<pb::CancelConnectionRequest>,
    ) -> Result<Response<pb::CancelConnectionResponse>, Status> {
        let backend = convert::backend_id_from(request.into_inner().provider_backend_id)?;
        self.app
            .cancel_connection(backend)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::CancelConnectionResponse {}))
    }

    async fn disconnect_connector(
        &self,
        request: Request<pb::DisconnectConnectorRequest>,
    ) -> Result<Response<pb::DisconnectConnectorResponse>, Status> {
        let backend = convert::backend_id_from(request.into_inner().provider_backend_id)?;
        self.app
            .disconnect_connector(backend)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::DisconnectConnectorResponse {}))
    }

    async fn set_model_preference(
        &self,
        request: Request<pb::SetModelPreferenceRequest>,
    ) -> Result<Response<pb::SetModelPreferenceResponse>, Status> {
        let request = request.into_inner();
        let backend = convert::backend_id_from(request.provider_backend_id)?;
        self.app
            .set_model_preference(backend, &request.model, &request.effort, request.as_default)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::SetModelPreferenceResponse {}))
    }

    async fn prefer_model(
        &self,
        _request: Request<pb::PreferModelRequest>,
    ) -> Result<Response<pb::PreferModelResponse>, Status> {
        let backend = self.app.prefer_model().await.map_err(convert::internal)?;
        Ok(Response::new(pb::PreferModelResponse {
            provider_backend_id: backend.map(convert::backend_id),
        }))
    }

    async fn available_connectors(
        &self,
        _request: Request<pb::AvailableConnectorsRequest>,
    ) -> Result<Response<pb::AvailableConnectorsResponse>, Status> {
        let connectors = self
            .app
            .available_connectors()
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::AvailableConnectorsResponse {
            connectors: connectors.into_iter().map(convert::connector).collect(),
        }))
    }

    async fn connectors_health_level(
        &self,
        _request: Request<pb::ConnectorsHealthLevelRequest>,
    ) -> Result<Response<pb::ConnectorsHealthLevelResponse>, Status> {
        let level = self.app.connectors_health_level().await;
        Ok(Response::new(pb::ConnectorsHealthLevelResponse {
            health_level: convert::health_level(level) as i32,
        }))
    }

    async fn plugins_health_level(
        &self,
        _request: Request<pb::PluginsHealthLevelRequest>,
    ) -> Result<Response<pb::PluginsHealthLevelResponse>, Status> {
        let level = self.app.plugins_health_level().await;
        Ok(Response::new(pb::PluginsHealthLevelResponse {
            health_level: convert::health_level(level) as i32,
        }))
    }

    async fn list_extension_plugins(
        &self,
        _request: Request<pb::ListExtensionPluginsRequest>,
    ) -> Result<Response<pb::ListExtensionPluginsResponse>, Status> {
        let extensions = self
            .app
            .list_extension_plugins()
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::ListExtensionPluginsResponse {
            extensions: extensions
                .into_iter()
                .map(convert::extension_info)
                .collect(),
        }))
    }

    async fn list_provider_plugins(
        &self,
        _request: Request<pb::ListProviderPluginsRequest>,
    ) -> Result<Response<pb::ListProviderPluginsResponse>, Status> {
        let providers = self
            .app
            .list_provider_plugins()
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::ListProviderPluginsResponse {
            providers: providers.into_iter().map(convert::provider_info).collect(),
        }))
    }

    async fn list_mcps(
        &self,
        _request: Request<pb::ListMcpsRequest>,
    ) -> Result<Response<pb::ListMcpsResponse>, Status> {
        let mcps = self.app.list_mcps().await.map_err(convert::internal)?;
        Ok(Response::new(pb::ListMcpsResponse {
            mcps: mcps.into_iter().map(convert::mcp_info).collect(),
        }))
    }

    async fn add_extension_plugin(
        &self,
        request: Request<pb::AddExtensionPluginRequest>,
    ) -> Result<Response<pb::AddExtensionPluginResponse>, Status> {
        let config = convert::plugin_from(request.into_inner().config)?;
        self.app
            .add_extension_plugin(config)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::AddExtensionPluginResponse {}))
    }

    async fn add_provider_plugin(
        &self,
        request: Request<pb::AddProviderPluginRequest>,
    ) -> Result<Response<pb::AddProviderPluginResponse>, Status> {
        let config = convert::plugin_from(request.into_inner().config)?;
        self.app
            .add_provider_plugin(config)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::AddProviderPluginResponse {}))
    }

    async fn init_mcp_connection(
        &self,
        request: Request<pb::InitMcpConnectionRequest>,
    ) -> Result<Response<pb::InitMcpConnectionResponse>, Status> {
        let config = convert::plugin_from(request.into_inner().config)?;
        let state = self
            .app
            .init_mcp_connection(config)
            .await
            .map_err(convert::internal)?;
        let response = match state {
            Some(state) => {
                let id = Uuid::now_v7().to_string();
                let auth_url = state.auth_url().to_owned();
                self.oauth_sessions.insert(id.clone(), state);
                pb::InitMcpConnectionResponse {
                    oauth_session_id: Some(id),
                    auth_url: Some(auth_url),
                }
            },
            None => pb::InitMcpConnectionResponse {
                oauth_session_id: None,
                auth_url: None,
            },
        };
        Ok(Response::new(response))
    }

    async fn finalize_mcp_connection(
        &self,
        request: Request<pb::FinalizeMcpConnectionRequest>,
    ) -> Result<Response<pb::FinalizeMcpConnectionResponse>, Status> {
        let request = request.into_inner();
        let config = convert::plugin_from(request.config)?;
        let state = request
            .oauth_session_id
            .map(|id| {
                self.oauth_sessions
                    .remove(&id)
                    .map(|(_, state)| state)
                    .ok_or_else(|| Status::failed_precondition("unknown oauth session"))
            })
            .transpose()?;
        self.app
            .finalize_mcp_connection(config, state)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::FinalizeMcpConnectionResponse {}))
    }

    async fn update_plugin(
        &self,
        request: Request<pb::UpdatePluginRequest>,
    ) -> Result<Response<pb::UpdatePluginResponse>, Status> {
        let request = request.into_inner();
        let plugin_type = convert::plugin_type_from(request.plugin_type)?;
        let plugin = convert::plugin_from(request.plugin)?;
        self.app
            .update_plugin(plugin_type, plugin)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::UpdatePluginResponse {}))
    }

    async fn remove_plugin(
        &self,
        request: Request<pb::RemovePluginRequest>,
    ) -> Result<Response<pb::RemovePluginResponse>, Status> {
        let request = request.into_inner();
        let plugin_type = convert::plugin_type_from(request.plugin_type)?;
        self.app
            .remove_plugin(plugin_type, &request.name)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::RemovePluginResponse {}))
    }

    async fn toggle_plugin(
        &self,
        request: Request<pb::TogglePluginRequest>,
    ) -> Result<Response<pb::TogglePluginResponse>, Status> {
        let request = request.into_inner();
        self.app
            .toggle_plugin(&request.name, request.disabled)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::TogglePluginResponse {}))
    }

    async fn toggle_capability(
        &self,
        request: Request<pb::ToggleCapabilityRequest>,
    ) -> Result<Response<pb::ToggleCapabilityResponse>, Status> {
        let request = request.into_inner();
        let facet = convert::capability_facet_from(request.facet)?;
        self.app
            .toggle_capability(&request.name, &request.capability, facet, request.disabled)
            .await
            .map_err(convert::internal)?;
        Ok(Response::new(pb::ToggleCapabilityResponse {}))
    }

    async fn health(
        &self,
        _request: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        Ok(Response::new(pb::HealthResponse {}))
    }
}
