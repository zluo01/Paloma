mod credentials;
mod helper;

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use log::{error, warn};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequest, CallToolRequestParams, ClientRequest, ServerResult, Tool},
    service::{PeerRequestOptions, RunningService, ServiceError},
    transport::{
        AuthClient, AuthError, AuthorizationManager, StreamableHttpClientTransport,
        TokioChildProcess, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use scry_utils::{Element, attempt_with_retry};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

use crate::{
    capability::{ToolResult, ToolSchema, ToolSpec},
    constants::{MAX_STREAM_PAYLOAD_BYTES, SPILL_ROOT},
    db::Storage,
    entity::{HealthStatus, Plugin, PluginArgs},
    mcp::{credentials::CredentialStorage, helper::mcp_function_name_encode},
    utils::write_spill_file,
};

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct McpPluginInfo {
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
    pub config: Plugin,
}

pub struct McpPlugin {
    name: String,
    description: String,
    timeout: u32,
    client: Option<RunningService<RoleClient, ()>>,
    status: AtomicU8,
    error: Option<String>,
}

impl McpPlugin {
    pub async fn new(
        config: &Plugin,
        request_client: reqwest::Client,
        storage: Storage,
    ) -> (Self, HashMap<String, ToolSpec>) {
        // make sure problematic mcp server will not hang the startup
        let init = timeout(CONNECTION_TIMEOUT, async {
            let client = attempt_with_retry(
                || {
                    connect(
                        request_client.clone(),
                        storage.clone(),
                        &config.name,
                        &config.args,
                        &config.env,
                    )
                },
                McpPluginError::is_connection_level,
            )
            .await?;
            let tools = client.list_all_tools().await?;
            Ok::<_, McpPluginError>((client, tools))
        })
        .await;

        match init {
            Ok(Ok((client, tools))) => {
                let schemas = tools_to_specs(&config.name, tools);
                let description = client
                    .peer_info()
                    .and_then(|info| info.server_info.description.clone())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let tool = Self {
                    name: config.name.to_string(),
                    description,
                    timeout: config.timeout,
                    client: Some(client),
                    status: AtomicU8::new(HealthStatus::Running as u8),
                    error: None,
                };
                (tool, schemas)
            },
            Ok(Err(error)) => {
                error!(
                    "failed to connect to mcp server {}: {}",
                    &config.name, error
                );
                (Self::unhealthy(config, &error), HashMap::new())
            },
            Err(_elapsed) => {
                let error = McpPluginError::InitTimeout(CONNECTION_TIMEOUT.as_secs());
                error!("mcp server {}: {}", &config.name, error);
                (Self::unhealthy(config, &error), HashMap::new())
            },
        }
    }

    /// unhealthy constructor
    fn unhealthy(config: &Plugin, error: &McpPluginError) -> Self {
        Self {
            name: config.name.to_string(),
            description: String::new(),
            timeout: config.timeout,
            client: None,
            status: AtomicU8::new(HealthStatus::Unhealthy as u8),
            error: Some(error.to_string()),
        }
    }

    async fn specs(&self) -> std::result::Result<HashMap<String, ToolSpec>, String> {
        let Some(client) = self.client.as_ref() else {
            error!(
                "Unexpected invocation on disconnected mcp server `{}` for getting schema. This indicates a bug.",
                self.name
            );
            return Err(format!(
                "internal error: mcp server `{}` is unavailable",
                self.name
            ));
        };

        let outcome = timeout(
            Duration::from_secs(self.timeout as u64),
            attempt_with_retry(
                || async move {
                    client
                        .list_all_tools()
                        .await
                        .map_err(McpPluginError::Service)
                },
                McpPluginError::is_connection_level,
            ),
        )
        .await;

        match outcome {
            Ok(Ok(tools)) => Ok(tools_to_specs(&self.name, tools)),
            Ok(Err(err)) => {
                if err.is_connection_level() {
                    self.status
                        .store(HealthStatus::Unhealthy as u8, Ordering::Relaxed);
                }
                error!("fail to list tools: {err}");
                Err(err.to_string())
            },
            Err(_elapsed) => {
                let msg = format!("fail to list tools, timed out after {}s", self.timeout);
                error!("{msg}");
                Err(msg)
            },
        }
    }

    pub fn health_status(&self) -> HealthStatus {
        HealthStatus::from_u8(self.status.load(Ordering::Relaxed))
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub async fn call(
        &self,
        name: Option<String>, // mcp tool name
        cancel: CancellationToken,
        call_id: String,
        args: Value,
    ) -> Result<ToolResult> {
        let tool = name.ok_or(McpPluginError::MissingToolName)?;

        let Some(client) = self.client.as_ref() else {
            error!(
                "Unexpected invocation on disconnected mcp server `{}`. This indicates a bug.",
                self.name
            );
            return Err(McpPluginError::Disconnected(self.name.clone()));
        };

        let arguments = args.as_object().cloned().unwrap_or_default();

        let outcome = timeout(
            Duration::from_secs(self.timeout as u64),
            attempt_with_retry(
                || {
                    let params = CallToolRequestParams::new(tool.to_string())
                        .with_arguments(arguments.clone());
                    let cancel = cancel.clone();
                    async move {
                        let mut handle = client
                            .send_cancellable_request(
                                ClientRequest::CallToolRequest(CallToolRequest::new(params)),
                                PeerRequestOptions::no_options(),
                            )
                            .await
                            .map_err(McpPluginError::Service)?;

                        tokio::select! {
                            result = &mut handle.rx => {
                                let response = result
                                    .unwrap_or(Err(ServiceError::TransportClosed))
                                    .map_err(McpPluginError::Service)?;
                                match response {
                                    ServerResult::CallToolResult(result) => Ok(result),
                                    _ => Err(McpPluginError::Service(
                                        ServiceError::UnexpectedResponse,
                                    )),
                                }
                            },
                            _ = cancel.cancelled() => {
                                let _ = handle.cancel(Some("session cancelled".into())).await;
                                Err(McpPluginError::Cancelled)
                            },
                        }
                    }
                },
                McpPluginError::is_connection_level,
            ),
        )
        .await;

        let result = match outcome {
            Ok(Ok(result)) => {
                self.status
                    .store(HealthStatus::Running as u8, Ordering::Relaxed);
                result
            },
            // Completed but errored. Only a dead connection counts against
            // health; a logical/protocol error means the server is up.
            Ok(Err(err)) => {
                if err.is_connection_level() {
                    self.status
                        .store(HealthStatus::Unhealthy as u8, Ordering::Relaxed);
                }
                error!("mcp tool `{tool}` failed: {err}");
                return Err(err);
            },
            // Timed out across all retries — ambiguous (slow tool vs hung
            // server), so surface it but leave health untouched.
            Err(_elapsed) => {
                let err = McpPluginError::CallTimeout {
                    tool: tool.clone(),
                    seconds: self.timeout,
                };
                error!("{err}");
                return Err(err);
            },
        };

        let text = match result.structured_content {
            Some(structured) => structured.to_string(),
            None => result
                .content
                .into_iter()
                .filter_map(|content| match content {
                    rmcp::model::ContentBlock::Text(text) => Some(text.text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };

        Ok(ToolResult::Text(
            truncate_payload(&tool, &call_id, text).await,
        ))
    }
}

/// Truncate payload if exceed [`MAX_STREAM_PAYLOAD_BYTES`] to prevent
/// `Your input exceeds the context window of this model. Please adjust your input and try again.`
async fn truncate_payload(tool: &str, call_id: &str, text: String) -> String {
    if text.len() <= MAX_STREAM_PAYLOAD_BYTES {
        return text;
    }

    let mut end = MAX_STREAM_PAYLOAD_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }

    let spill_path = write_spill_file(&SPILL_ROOT, call_id, "out", text.as_bytes()).await;

    Element::new("mcp_output")
        .attr("tool", tool)
        .attr("total_bytes", text.len())
        .attr("truncated", "true")
        .attr_if_some(
            "full_output",
            spill_path.as_ref().map(|p| p.display().to_string()),
        )
        .cdata(format!("{}...", &text[..end]))
        .to_string()
}

async fn connect(
    request_client: reqwest::Client,
    storage: Storage,
    name: &str,
    cfg: &PluginArgs,
    env: &HashMap<String, String>,
) -> Result<RunningService<RoleClient, ()>> {
    match cfg {
        // Local process over stdio
        PluginArgs::Local { command, args } => {
            let mut cmd = Command::new(command);

            for (key, value) in env {
                cmd.env(key, value);
            }

            cmd.args(args).kill_on_drop(true);

            // Use builder pattern to capture stderr
            let (transport, stderr) = TokioChildProcess::builder(cmd)
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            // Spawn a task to drain stderr to prevent buffer overflow
            // If stderr fills up, the child process will block
            if let Some(stderr) = stderr {
                let name = name.to_string();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        warn!("MCP server [{}] stderr: {}", name, line);
                    }
                });
            }

            Ok(().serve(transport).await?)
        },
        PluginArgs::Remote {
            url,
            requires_auth: false,
        } => Ok(create_http_connection(request_client.clone(), url).await?),
        PluginArgs::Remote {
            url,
            requires_auth: true,
        } => Ok(
            create_auth_http_connection(request_client.clone(), storage.clone(), url, name).await?,
        ),
    }
}

async fn create_http_connection(
    request_client: reqwest::Client,
    url: &str,
) -> Result<RunningService<RoleClient, ()>> {
    let config = StreamableHttpClientTransportConfig::with_uri(url);
    let transport = StreamableHttpClientTransport::with_client(request_client, config);
    Ok(().serve(transport).await?)
}

async fn create_auth_http_connection(
    request_client: reqwest::Client,
    storage: Storage,
    url: &str,
    name: &str,
) -> Result<RunningService<RoleClient, ()>> {
    let credential_store = CredentialStorage::new(storage, name.to_string());
    let mut auth_manager = AuthorizationManager::new(url).await?;
    auth_manager.set_credential_store(credential_store);

    // Connecting never starts an interactive flow: stored credentials are
    // required, and authorization happens through the settings action.
    if !auth_manager.initialize_from_store().await? {
        return Err(McpPluginError::Auth(AuthError::AuthorizationRequired));
    }
    // eagerly validate the access token
    auth_manager.get_access_token().await?;

    let auth_client = AuthClient::new(request_client, auth_manager);
    let config = StreamableHttpClientTransportConfig::with_uri(url);
    let transport = StreamableHttpClientTransport::with_client(auth_client, config);
    Ok(().serve(transport).await?)
}

/// convert list of rmcp tools to tool specs
fn tools_to_specs(name: &str, tools: Vec<Tool>) -> HashMap<String, ToolSpec> {
    tools
        .into_iter()
        .map(|tool| {
            let spec = ToolSpec {
                name: name.to_string(),
                tool: Some(tool.name.to_string()),
                schema: ToolSchema {
                    name: mcp_function_name_encode(name, &tool.name),
                    description: tool.description.map(|d| d.into_owned()).unwrap_or_default(),
                    parameters: Value::Object((*tool.input_schema).clone()),
                },
            };
            (spec.schema.name.clone(), spec)
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum McpPluginError {
    #[error("failed to spawn MCP server process: {0}")]
    Spawn(#[from] std::io::Error),

    #[error("failed to initialize MCP client: {0}")]
    Init(Box<rmcp::service::ClientInitializeError>),

    #[error("initialization timed out after {0}s")]
    InitTimeout(u64),

    #[error("MCP request failed: {0}")]
    Service(#[from] rmcp::service::ServiceError),

    #[error("MCP authorization failed: {0}")]
    Auth(#[from] AuthError),

    #[error("MCP tool call requires a tool name")]
    MissingToolName,

    #[error("internal error: mcp server `{0}` is unavailable")]
    Disconnected(String),

    #[error("mcp tool `{tool}` timed out after {seconds}s")]
    CallTimeout { tool: String, seconds: u32 },

    #[error("mcp tool call was cancelled")]
    Cancelled,
}

// `ClientInitializeError` is large (~500 bytes), so box it to keep `McpToolError`
// small. Manual `From` (instead of `#[from]`) so `?` on a bare error still boxes.
impl From<rmcp::service::ClientInitializeError> for McpPluginError {
    fn from(error: rmcp::service::ClientInitializeError) -> Self {
        McpPluginError::Init(Box::new(error))
    }
}

impl McpPluginError {
    /// Connection-level failures: failing to establish the session (`Init`) or
    /// losing it mid-call (broken pipe / closed transport). These are the
    /// errors worth retrying — for both `connect` and live `call_tool`.
    fn is_connection_level(&self) -> bool {
        matches!(
            self,
            McpPluginError::Init(_)
                | McpPluginError::Service(
                    rmcp::service::ServiceError::TransportSend(_)
                        | rmcp::service::ServiceError::TransportClosed
                )
                | McpPluginError::Auth(AuthError::HttpError(_))
        )
    }
}

type Result<T> = std::result::Result<T, McpPluginError>;
