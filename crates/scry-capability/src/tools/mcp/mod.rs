use std::{
    collections::HashMap,
    future::Future,
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use backon::{ExponentialBuilder, Retryable};
use log::{error, warn};
use rmcp::{
    model::{CallToolRequestParams, Tool},
    service::RunningService,
    transport::{StreamableHttpClientTransport, TokioChildProcess},
    RoleClient, ServiceExt,
};
use scry_config::{MAX_STREAM_PAYLOAD_BYTES, SPILL_ROOT};
use scry_storage::{Plugin, PluginArgs};
use scry_utils::{mcp_function_name_encode, write_spill_file, Element};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::timeout,
};
use uuid::Uuid;

use crate::{entity::ToolSpec, DynTool, HealthStatus, ToolResult, ToolSchema};

/// Maximum number of retry attempts for a transient MCP transport failure.
const MAX_RETRY_TIMES: usize = 3;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

pub struct McpTool {
    name: String,
    description: String,
    timeout: i64,
    client: Option<RunningService<RoleClient, ()>>,
    status: AtomicU8,
    error: Option<String>,
}

impl McpTool {
    pub async fn new(config: &Plugin) -> (Self, Vec<ToolSpec>) {
        // make sure problematic mcp server will not hang the startup
        let init = timeout(CONNECTION_TIMEOUT, async {
            let client =
                attempt_with_retry(|| connect(&config.name, &config.args, &config.env)).await?;
            let tools = client.list_all_tools().await?;
            Ok::<_, McpToolError>((client, tools))
        })
        .await;

        match init {
            Ok(Ok((client, tools))) => {
                let schemas = tools_to_specs(&config.name, tools);
                let description = client
                    .peer_info()
                    .and_then(|info| info.server_info.description.as_deref())
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
                (Self::unhealthy(config, &error), Vec::new())
            },
            Err(_elapsed) => {
                let error = McpToolError::InitTimeout(CONNECTION_TIMEOUT.as_secs());
                error!("mcp server {}: {}", &config.name, error);
                (Self::unhealthy(config, &error), Vec::new())
            },
        }
    }

    /// unhealthy constructor
    fn unhealthy(config: &Plugin, error: &McpToolError) -> Self {
        Self {
            name: config.name.to_string(),
            description: String::new(),
            timeout: config.timeout,
            client: None,
            status: AtomicU8::new(HealthStatus::Unhealthy as u8),
            error: Some(error.to_string()),
        }
    }
}

#[async_trait::async_trait]
impl DynTool for McpTool {
    async fn specs(&self) -> std::result::Result<Vec<ToolSpec>, String> {
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
            attempt_with_retry(|| async move {
                client.list_all_tools().await.map_err(McpToolError::Service)
            }),
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

    fn health_statue(&self) -> HealthStatus {
        HealthStatus::from_u8(self.status.load(Ordering::Relaxed))
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    async fn invoke(
        &self,
        name: Option<String>, // mcp tool name
        _session_id: Uuid,
        call_id: String,
        args: Value,
    ) -> std::result::Result<ToolResult, String> {
        let tool = name.ok_or("MCP tool call requires a tool name")?;

        let Some(client) = self.client.as_ref() else {
            error!(
                "Unexpected invocation on disconnected mcp server `{}`. This indicates a bug.",
                self.name
            );
            return Err(format!(
                "internal error: mcp server `{}` is unavailable",
                self.name
            ));
        };

        let arguments = args.as_object().cloned().unwrap_or_default();

        let outcome = timeout(
            Duration::from_secs(self.timeout as u64),
            attempt_with_retry(|| {
                let params =
                    CallToolRequestParams::new(tool.to_string()).with_arguments(arguments.clone());
                async move {
                    client
                        .call_tool(params)
                        .await
                        .map_err(McpToolError::Service)
                }
            }),
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
                return Err(err.to_string());
            },
            // Timed out across all retries — ambiguous (slow tool vs hung
            // server), so surface it but leave health untouched.
            Err(_elapsed) => {
                let msg = format!("mcp tool `{tool}` timed out after {}s", self.timeout);
                error!("{msg}");
                return Err(msg);
            },
        };

        let text = match result.structured_content {
            Some(structured) => structured.to_string(),
            None => result
                .content
                .into_iter()
                .filter_map(|content| match content.raw {
                    rmcp::model::RawContent::Text(text) => Some(text.text),
                    rmcp::model::RawContent::Image(_)
                    | rmcp::model::RawContent::Resource(_)
                    | rmcp::model::RawContent::ResourceLink(_)
                    | rmcp::model::RawContent::Audio(_) => None,
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
        // Plain HTTP
        PluginArgs::Remote {
            url,
            requires_auth: false,
        } => Ok(
            ().serve(StreamableHttpClientTransport::from_uri(url.clone()))
                .await?,
        ),
        // OAuth-protected HTTP — not implemented yet.
        PluginArgs::Remote {
            requires_auth: true,
            ..
        } => Err(McpToolError::Unsupported(
            "authenticated HTTP MCP servers are not supported yet".to_string(),
        )),
    }
}

/// Retry an MCP call on transient transport failures with exponential backoff +
/// jitter. Non-transport errors fail immediately; if every attempt fails, the
/// last error is returned.
async fn attempt_with_retry<T, F>(call: impl Fn() -> F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    call.retry(
        ExponentialBuilder::default()
            .with_max_times(MAX_RETRY_TIMES)
            .with_jitter(),
    )
    .when(McpToolError::is_connection_level)
    .await
}

/// convert list of rmcp tools to tool specs
fn tools_to_specs(name: &str, tools: Vec<Tool>) -> Vec<ToolSpec> {
    tools
        .into_iter()
        .map(|tool| ToolSpec {
            name: name.to_string(),
            tool: Some(tool.name.to_string()),
            schema: ToolSchema {
                name: mcp_function_name_encode(name, &tool.name),
                description: tool.description.map(|d| d.into_owned()).unwrap_or_default(),
                parameters: Value::Object((*tool.input_schema).clone()),
            },
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum McpToolError {
    #[error("failed to spawn MCP server process: {0}")]
    Spawn(#[from] std::io::Error),

    #[error("failed to initialize MCP client: {0}")]
    Init(Box<rmcp::service::ClientInitializeError>),

    #[error("initialization timed out after {0}s")]
    InitTimeout(u64),

    #[error("MCP request failed: {0}")]
    Service(#[from] rmcp::service::ServiceError),

    #[error("unsupported: {0}")]
    Unsupported(String),
}

// `ClientInitializeError` is large (~500 bytes), so box it to keep `McpToolError`
// small. Manual `From` (instead of `#[from]`) so `?` on a bare error still boxes.
impl From<rmcp::service::ClientInitializeError> for McpToolError {
    fn from(error: rmcp::service::ClientInitializeError) -> Self {
        McpToolError::Init(Box::new(error))
    }
}

impl McpToolError {
    /// Connection-level failures: failing to establish the session (`Init`) or
    /// losing it mid-call (broken pipe / closed transport). These are the
    /// errors worth retrying — for both `connect` and live `call_tool`.
    fn is_connection_level(&self) -> bool {
        matches!(
            self,
            McpToolError::Init(_)
                | McpToolError::Service(
                    rmcp::service::ServiceError::TransportSend(_)
                        | rmcp::service::ServiceError::TransportClosed
                )
        )
    }
}

type Result<T> = std::result::Result<T, McpToolError>;
