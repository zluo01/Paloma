use std::{
    io::Error,
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use log::{error, warn};
use scry_extension_protocol::{
    Bytes, Message, PROTOCOL_VERSION,
    v1::{
        Action, CancelToolRequest, HandshakeRequest, HandshakeResponse, InvokeToolRequest, Item,
        RequestEvent, ResponseEvent, RunActionRequest, SearchRequest, ToolContent, request_event,
        response_event::Payload, run_action_response::Behavior, tool_content,
    },
};
use scry_utils::{
    Element,
    transport::{FramedRead, FramedWrite, VarintDelimitedCodec},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot},
    time,
};

use crate::{
    HealthStatus, Plugin, PluginArgs,
    constants::{MAX_STREAM_PAYLOAD_BYTES, SPILL_ROOT},
    entity::ToolResult,
    utils::{shell_path, write_spill_file},
};

const EXTENSION_REQUEST_CHANNEL_CAPACITY: usize = 16;
const DEFAULT_UNARY_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TOOL_INVOKE_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

pub struct ExtensionPlugin {
    next_event_id: AtomicU64,
    health_status: Arc<AtomicU8>,
    error: Arc<OnceLock<String>>,
    pending: Arc<DashMap<u64, oneshot::Sender<Payload>>>,
    writer: mpsc::Sender<RequestEvent>,
    child: Option<Mutex<Child>>,
}

impl ExtensionPlugin {
    pub async fn connect(plugin: &Plugin) -> Result<Arc<Self>> {
        let mut child = execute_plugin(plugin).await?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        let health_status = Arc::new(AtomicU8::new(HealthStatus::Starting as u8));
        let error: Arc<OnceLock<String>> = Arc::default();

        // request dispatch
        let (writer, mut writer_rx) =
            mpsc::channel::<RequestEvent>(EXTENSION_REQUEST_CHANNEL_CAPACITY);
        let health = Arc::clone(&health_status);
        let write_error = Arc::clone(&error);
        tokio::spawn(async move {
            let mut output = FramedWrite::new(stdin, VarintDelimitedCodec);
            while let Some(request) = writer_rx.recv().await {
                if let Err(e) = output.send(Bytes::from(request.encode_to_vec())).await {
                    // pipe closed: child is gone
                    let _ = write_error.set(format!("plugin stopped accepting requests: {e}"));
                    health.store(HealthStatus::Unhealthy as u8, Ordering::SeqCst);
                    break;
                }
            }
        });

        // handling response
        let pending: Arc<DashMap<u64, oneshot::Sender<Payload>>> = Arc::new(DashMap::new());
        let routes = Arc::clone(&pending);
        let health = Arc::clone(&health_status);
        let read_error = Arc::clone(&error);
        tokio::spawn(async move {
            let mut input = FramedRead::new(stdout, VarintDelimitedCodec);
            while let Some(Ok(frame)) = input.next().await {
                let response = match ResponseEvent::decode(frame.freeze()) {
                    Ok(response) => response,
                    Err(e) => {
                        error!("undecodable extension plugin frame: {e}");
                        continue;
                    },
                };
                let Some(payload) = response.payload else {
                    error!(
                        "response {} has no payload: indicate bugs or newer protocol version",
                        response.event_id
                    );
                    routes.remove(&response.event_id);
                    continue;
                };
                match routes.remove(&response.event_id) {
                    Some((_, tx)) => {
                        let _ = tx.send(payload);
                    },
                    None => warn!(
                        "response {} has no pending unary request",
                        response.event_id
                    ),
                }
            }
            // EOF: child died.
            let _ = read_error.set("plugin process exited".to_string());
            health.store(HealthStatus::Unhealthy as u8, Ordering::SeqCst);
            routes.clear();
        });

        Ok(Arc::new(Self {
            next_event_id: AtomicU64::default(),
            health_status,
            error,
            pending,
            writer,
            child: Some(Mutex::new(child)),
        }))
    }

    pub fn unhealthy(error: impl Into<String>) -> Arc<Self> {
        let plugin_error: Arc<OnceLock<String>> = Arc::default();
        let _ = plugin_error.set(error.into());
        let (writer, _) = mpsc::channel(1);
        Arc::new(Self {
            next_event_id: AtomicU64::default(),
            health_status: Arc::new(AtomicU8::new(HealthStatus::Unhealthy as u8)),
            error: plugin_error,
            pending: Arc::new(DashMap::new()),
            writer,
            child: None,
        })
    }

    pub fn health(&self) -> HealthStatus {
        HealthStatus::from_u8(self.health_status.load(Ordering::Relaxed))
    }

    pub fn plugin_error(&self) -> Option<String> {
        self.error.get().cloned()
    }

    // explicitly shutdown function
    // use on remove such that any arc reference call will also be killed
    pub fn shutdown(&self) {
        if let Some(child) = &self.child
            && let Ok(mut child) = child.lock()
        {
            let _ = child.start_kill();
        }
    }

    async fn request(
        &self,
        capability_id: Option<String>,
        payload: request_event::Payload,
        timeout: Duration,
    ) -> Result<Payload> {
        let event_id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.insert(event_id, tx);

        // Defensive guard on child process killed
        if self.health_status.load(Ordering::SeqCst) == HealthStatus::Unhealthy as u8 {
            self.pending.remove(&event_id);
            return Err(ExtensionConnectionError::Disconnected);
        }

        let request = RequestEvent {
            event_id,
            capability_id,
            payload: Some(payload),
        };
        if self.writer.send(request).await.is_err() {
            self.pending.remove(&event_id);
            return Err(ExtensionConnectionError::Disconnected);
        }

        let Ok(reply) = time::timeout(timeout, rx).await else {
            self.pending.remove(&event_id);
            return Err(ExtensionConnectionError::Timeout(timeout));
        };

        match reply.map_err(|_| ExtensionConnectionError::Disconnected)? {
            Payload::ExtensionError(e) => Err(ExtensionConnectionError::Extension(e.error)),
            payload => Ok(payload),
        }
    }

    pub async fn handshake(&self) -> Result<HandshakeResponse> {
        match self
            .request(
                None,
                request_event::Payload::HandshakeRequest(HandshakeRequest {}),
                DEFAULT_UNARY_REQUEST_TIMEOUT,
            )
            .await?
        {
            Payload::HandshakeResponse(handshake) => {
                if handshake.version != PROTOCOL_VERSION {
                    return Err(ExtensionConnectionError::ProtocolVersion {
                        expected: PROTOCOL_VERSION,
                        actual: handshake.version,
                    });
                }
                self.health_status
                    .store(HealthStatus::Running as u8, Ordering::SeqCst);
                Ok(handshake)
            },
            _ => Err(ExtensionConnectionError::UnexpectedResponse),
        }
    }

    pub async fn search(&self, capability_id: String, input: String) -> Result<Vec<Item>> {
        match self
            .request(
                Some(capability_id),
                request_event::Payload::SearchRequest(SearchRequest { input }),
                DEFAULT_UNARY_REQUEST_TIMEOUT,
            )
            .await?
        {
            Payload::SearchResponse(response) => Ok(response.items),
            _ => Err(ExtensionConnectionError::UnexpectedResponse),
        }
    }

    pub async fn run_search_action(
        &self,
        capability_id: String,
        action: Action,
    ) -> Result<Behavior> {
        match self
            .request(
                Some(capability_id),
                request_event::Payload::RunActionRequest(RunActionRequest {
                    action: Some(action),
                }),
                DEFAULT_UNARY_REQUEST_TIMEOUT,
            )
            .await?
        {
            Payload::RunActionResponse(response) => response
                .behavior
                .ok_or(ExtensionConnectionError::UnexpectedResponse),
            _ => Err(ExtensionConnectionError::UnexpectedResponse),
        }
    }

    pub async fn invoke_tool(
        &self,
        capability_id: String,
        session_id: String,
        call_id: String,
        arguments: String,
    ) -> Result<ToolResult> {
        let result = self
            .request(
                Some(capability_id),
                request_event::Payload::InvokeToolRequest(InvokeToolRequest {
                    session_id: session_id.clone(),
                    call_id: call_id.clone(),
                    arguments,
                }),
                TOOL_INVOKE_REQUEST_TIMEOUT,
            )
            .await;

        match result {
            Ok(Payload::InvokeToolResponse(response)) => {
                let content = response
                    .content
                    .ok_or(ExtensionConnectionError::UnexpectedResponse)?;
                Ok(augment_tool_response(content, &call_id).await)
            },
            Ok(_) => Err(ExtensionConnectionError::UnexpectedResponse),
            Err(ExtensionConnectionError::Timeout(elapsed)) => {
                if let Err(e) = self.cancel_tool(session_id).await {
                    warn!("failed to cancel timed out tool invocation {call_id}: {e}");
                }
                Err(ExtensionConnectionError::Timeout(elapsed))
            },
            Err(e) => Err(e),
        }
    }

    pub async fn cancel_tool(&self, session_id: String) -> Result<()> {
        match self
            .request(
                None,
                request_event::Payload::CancelToolRequest(CancelToolRequest { session_id }),
                DEFAULT_UNARY_REQUEST_TIMEOUT,
            )
            .await?
        {
            Payload::CancelToolResponse(_) => Ok(()),
            _ => Err(ExtensionConnectionError::UnexpectedResponse),
        }
    }
}

async fn execute_plugin(plugin: &Plugin) -> Result<Child> {
    let PluginArgs::Local { command, args } = plugin.args.clone() else {
        return Err(ExtensionConnectionError::Extension(format!(
            "extension plugin {} must be a local command",
            plugin.name
        )));
    };

    let mut cmd = Command::new(&command);
    if let Some(path) = shell_path().await {
        cmd.env("PATH", path);
    }
    let mut child = cmd
        .args(args)
        .envs(&plugin.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    if let Some(stderr) = child.stderr.take() {
        let name = plugin.name.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                warn!("extension plugin [{name}] stderr: {line}");
            }
        });
    }

    Ok(child)
}

async fn augment_tool_response(response: ToolContent, call_id: &str) -> ToolResult {
    let total = total_text_bytes(&response);
    let over_budget = total > MAX_STREAM_PAYLOAD_BYTES;

    ToolResult::Text(
        tool_content_to_element(
            &response,
            over_budget.then_some(total),
            call_id,
            &SPILL_ROOT,
        )
        .await
        .to_string(),
    )
}

fn total_text_bytes(content: &ToolContent) -> usize {
    let mut total = 0;
    let mut stack = vec![content];
    while let Some(node) = stack.pop() {
        if let Some(tool_content::Body::Text(text)) = &node.body {
            total += text.len();
        }
        stack.extend(node.children().iter());
    }
    total
}

async fn tool_content_to_element(
    content: &ToolContent,
    total: Option<usize>,
    call_id: &str,
    spill_root: &Path,
) -> Element {
    // collect all nodes in pre-order
    let mut nodes: Vec<&ToolContent> = Vec::new();
    let mut stack: Vec<&ToolContent> = vec![content];
    while let Some(node) = stack.pop() {
        nodes.push(node);
        stack.extend(node.children().iter().rev());
    }

    // construct from the bottom of the tree: in reverse pre-order a node's
    // finished children are always on top of the stack
    let spilled = AtomicUsize::new(0);
    let mut built: Vec<Element> = Vec::new();
    for node in nodes[1..].iter().rev() {
        let children = built.split_off(built.len() - node.children().len());
        built.push(build_element(node, children, total, call_id, spill_root, &spilled).await);
    }
    build_element(content, built, total, call_id, spill_root, &spilled).await
}

async fn build_element(
    node: &ToolContent,
    children: Vec<Element>,
    total: Option<usize>,
    call_id: &str,
    spill_root: &Path,
    spilled: &AtomicUsize,
) -> Element {
    let mut element = Element::new(node.tag.clone());
    for attribute in &node.attributes {
        element = element.attr(attribute.key.clone(), &attribute.value);
    }
    if let Some(tool_content::Body::Text(text)) = &node.body {
        let budget = total.map(|total| text.len() * MAX_STREAM_PAYLOAD_BYTES / total);
        element = match budget {
            Some(budget) if text.len() > budget => {
                let name = format!("part-{}", spilled.fetch_add(1, Ordering::Relaxed));
                let path = write_spill_file(spill_root, call_id, &name, text.as_bytes()).await;
                let mut end = budget;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                element
                    .attr("total_bytes", text.len())
                    .attr("truncated", "true")
                    .attr_if_some(
                        "full_output",
                        path.as_ref().map(|p| p.display().to_string()),
                    )
                    .cdata(format!("{}...", &text[..end]))
            },
            _ => element.cdata(text.clone()),
        };
    }
    for child in children.into_iter().rev() {
        element = element.child(child);
    }
    element
}

#[derive(Debug, thiserror::Error)]
pub enum ExtensionConnectionError {
    #[error(transparent)]
    Io(#[from] Error),

    #[error("extension plugin exited or closed its transport")]
    Disconnected,

    #[error("extension plugin did not respond within {0:?}")]
    Timeout(Duration),

    #[error("extension plugin speaks protocol version {actual}, host expects {expected}")]
    ProtocolVersion { expected: u64, actual: u64 },

    #[error("unexpected response payload")]
    UnexpectedResponse,

    #[error("extension error: {0}")]
    Extension(String),
}

type Result<T> = std::result::Result<T, ExtensionConnectionError>;

#[cfg(test)]
mod tests {
    use scry_extension_protocol::v1::Binary;

    use super::*;

    #[tokio::test]
    async fn renders_tree_with_attrs_and_children_in_order() {
        let content = ToolContent::new("shell_output")
            .attr("exit_code", 0)
            .child(ToolContent::new("stdout").cdata("hello"))
            .child(ToolContent::new("stderr"));

        let dir = tempfile::tempdir().unwrap();
        let rendered = tool_content_to_element(&content, None, "tc_plain", dir.path())
            .await
            .to_string();

        assert_eq!(
            rendered,
            "<shell_output\n  exit_code=\"0\"\n>\n<stdout><![CDATA[hello]]></stdout>\n<stderr></stderr>\n</shell_output>"
        );
    }

    #[tokio::test]
    async fn renders_nested_children() {
        let content = ToolContent::new("root")
            .child(ToolContent::new("a").child(ToolContent::new("b").cdata("x")))
            .child(ToolContent::new("c"));

        let dir = tempfile::tempdir().unwrap();
        let rendered = tool_content_to_element(&content, None, "tc_nested", dir.path())
            .await
            .to_string();

        assert_eq!(
            rendered,
            "<root>\n<a>\n<b><![CDATA[x]]></b>\n</a>\n<c></c>\n</root>"
        );
    }

    #[tokio::test]
    async fn over_budget_body_truncates_spills_and_annotates() {
        let dir = tempfile::tempdir().unwrap();
        let content = ToolContent::new("stdout").cdata("hello world");
        let total = "hello world".len() * MAX_STREAM_PAYLOAD_BYTES / 5;

        let rendered = tool_content_to_element(&content, Some(total), "tc_trunc", dir.path())
            .await
            .to_string();

        let spill = dir.path().join("tc_trunc").join("part-0");
        assert!(rendered.contains("total_bytes=\"11\""), "got: {rendered}");
        assert!(rendered.contains("truncated=\"true\""), "got: {rendered}");
        assert!(
            rendered.contains(&format!("full_output=\"{}\"", spill.display())),
            "got: {rendered}"
        );
        assert!(rendered.contains("<![CDATA[hello...]]>"), "got: {rendered}");
        assert_eq!(std::fs::read_to_string(&spill).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn truncation_respects_char_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let content = ToolContent::new("stdout").cdata("résumé.txt not found");
        let total = "résumé.txt not found".len() * MAX_STREAM_PAYLOAD_BYTES / 2;

        let rendered = tool_content_to_element(&content, Some(total), "tc_boundary", dir.path())
            .await
            .to_string();

        let spill = dir.path().join("tc_boundary").join("part-0");
        assert!(rendered.contains("total_bytes=\"22\""), "got: {rendered}");
        assert!(rendered.contains("truncated=\"true\""), "got: {rendered}");
        assert!(
            rendered.contains(&format!("full_output=\"{}\"", spill.display())),
            "got: {rendered}"
        );
        assert!(rendered.contains("<![CDATA[r...]]>"), "got: {rendered}");
        assert_eq!(
            std::fs::read_to_string(&spill).unwrap(),
            "résumé.txt not found"
        );
    }

    #[tokio::test]
    async fn same_tag_siblings_spill_to_separate_files() {
        let dir = tempfile::tempdir().unwrap();
        let content = ToolContent::new("results")
            .child(ToolContent::new("match").cdata("first payload"))
            .child(ToolContent::new("match").cdata("second payload"));

        let total = ("first payload".len() + "second payload".len()) * MAX_STREAM_PAYLOAD_BYTES / 4;

        tool_content_to_element(&content, Some(total), "tc_siblings", dir.path()).await;

        let call_dir = dir.path().join("tc_siblings");
        let mut spilled: Vec<String> = std::fs::read_dir(&call_dir)
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        spilled.sort();

        assert_eq!(
            spilled,
            vec!["first payload".to_string(), "second payload".to_string()]
        );
    }

    #[tokio::test]
    async fn same_tag_siblings_both_count_toward_total() {
        let content = ToolContent::new("results")
            .child(ToolContent::new("match").cdata("aaaa"))
            .child(ToolContent::new("match").cdata("bbbbbb"));

        assert_eq!(total_text_bytes(&content), 10);
    }

    #[tokio::test]
    async fn binary_body_renders_as_empty_element() {
        let content = ToolContent {
            tag: "blob".to_string(),
            body: Some(tool_content::Body::Binary(Binary {
                mime_type: "image/png".to_string(),
                data: vec![1, 2, 3],
            })),
            ..Default::default()
        };

        let dir = tempfile::tempdir().unwrap();
        let rendered = tool_content_to_element(&content, None, "tc_binary", dir.path())
            .await
            .to_string();

        assert_eq!(rendered, "<blob></blob>");
    }
}
