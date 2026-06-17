use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use dashmap::DashMap;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::{mpsc, oneshot},
    time::timeout,
};
use uuid::Uuid;

use crate::{
    capability::ToolResult,
    constants::{MAX_STREAM_PAYLOAD_BYTES, SPILL_ROOT},
    utils::Element,
};

/// Bounded mpsc capacity for the actor's incoming event queue.
const PROCESS_MANAGER_CHANNEL_CAPACITY: usize = 128;
/// Wall-clock budget for a single command before it is force-killed.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
/// Grace period to flush buffered pipe bytes after the process group is killed.
const IO_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
/// Per-syscall buffer size for the read loop; pure performance knob, unrelated to caps.
const READ_BUFFER_BYTES: usize = 8 * 1024;

#[derive(Debug)]
pub struct ProcessExecRequest {
    pub session_id: Uuid,
    pub call_id: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Debug)]
pub enum ProcessManagerEvent {
    Exec {
        request: ProcessExecRequest,
        reply: oneshot::Sender<Result<ToolResult>>,
    },
    CancelSession {
        session_id: Uuid,
        reply: oneshot::Sender<Result<()>>,
    },
}

pub struct ProcessManager {
    sessions: Arc<DashMap<Uuid, Vec<i32>>>,
    event_rx: mpsc::Receiver<ProcessManagerEvent>,
}

#[derive(Clone)]
pub struct ProcessManagerClient {
    event_tx: mpsc::Sender<ProcessManagerEvent>,
}

impl ProcessManager {
    pub fn new() -> (Self, ProcessManagerClient) {
        let (event_tx, event_rx) = mpsc::channel(PROCESS_MANAGER_CHANNEL_CAPACITY);
        let manager = Self {
            sessions: Arc::new(DashMap::new()),
            event_rx,
        };
        let client = ProcessManagerClient { event_tx };
        (manager, client)
    }

    pub async fn run(&mut self) {
        while let Some(event) = self.event_rx.recv().await {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: ProcessManagerEvent) {
        match event {
            ProcessManagerEvent::Exec { request, reply } => {
                self.handle_exec(request, reply);
            },
            ProcessManagerEvent::CancelSession { session_id, reply } => {
                self.cancel_session(session_id);
                let _ = reply.send(Ok(()));
            },
        }
    }

    fn handle_exec(
        &mut self,
        request: ProcessExecRequest,
        reply: oneshot::Sender<Result<ToolResult>>,
    ) {
        let mut cmd = Command::new(&request.command[0]);
        cmd.args(&request.command[1..])
            .current_dir(&request.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // make this command process its own process-group leader
        #[cfg(unix)]
        cmd.process_group(0);

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = reply.send(Err(ProcessManagerError::Spawn(e.to_string())));
                return;
            },
        };

        let pgid = child.id().map(|id| id as i32);
        match pgid {
            Some(pgid) => {
                self.sessions
                    .entry(request.session_id)
                    .or_default()
                    .push(pgid);
            },
            None => {
                log::error!(
                    "Spawned child for session {} has no pid. This indicate a code bug.",
                    request.session_id
                );
            },
        }

        // Per-invocation spill directory keyed by call_id. If we cannot
        // create it, spill is disabled for this exec and overflow bytes fall
        // back to discard.
        let spill_dir = SPILL_ROOT.join(&request.call_id);
        let spill_paths = match std::fs::create_dir_all(&spill_dir) {
            Ok(()) => Some((spill_dir.join("out"), spill_dir.join("err"))),
            Err(e) => {
                log::error!(
                    "could not create spill dir {:?}: {e}; overflow will be discarded",
                    spill_dir
                );
                None
            },
        };

        let sessions = self.sessions.clone();
        let session_id = request.session_id;
        tokio::spawn(async move {
            let result = run_to_completion(child, request, spill_paths).await;
            if let Some(pgid) = pgid {
                remove_pid(&sessions, session_id, pgid);
            }
            let _ = reply.send(Ok(result));
        });
    }

    fn cancel_session(&mut self, session_id: Uuid) {
        if let Some((_, pids)) = self.sessions.remove(&session_id) {
            for pid in pids {
                kill_process_group(pid);
            }
        }
    }
}

impl ProcessManagerClient {
    pub async fn exec(&self, request: ProcessExecRequest) -> Result<ToolResult> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(ProcessManagerEvent::Exec {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| ProcessManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| ProcessManagerError::ChannelClosed)?
    }

    pub async fn cancel_session(&self, session_id: Uuid) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.event_tx
            .send(ProcessManagerEvent::CancelSession {
                session_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| ProcessManagerError::ChannelClosed)?;
        reply_rx
            .await
            .map_err(|_| ProcessManagerError::ChannelClosed)?
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessManagerError {
    #[error("process manager channel closed")]
    ChannelClosed,

    #[error("failed to spawn process: {0}")]
    Spawn(String),
}

type Result<T> = std::result::Result<T, ProcessManagerError>;

fn remove_pid(sessions: &DashMap<Uuid, Vec<i32>>, session_id: Uuid, pid: i32) {
    let became_empty = match sessions.get_mut(&session_id) {
        Some(mut vec) => {
            vec.retain(|p| *p != pid);
            vec.is_empty()
        },
        None => return,
    };
    if became_empty {
        // Re-check under shard lock in case another exec inserted between
        // releasing the get_mut guard and this call.
        sessions.remove_if(&session_id, |_, v| v.is_empty());
    }
}

#[cfg(unix)]
fn kill_process_group(pgid: i32) {
    use nix::{
        sys::signal::{Signal, killpg},
        unistd::Pid,
    };
    let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL);
}

async fn run_to_completion(
    mut child: Child,
    request: ProcessExecRequest,
    spill_paths: Option<(PathBuf, PathBuf)>,
) -> ToolResult {
    let started_at = std::time::Instant::now();
    let (stdout_path, stderr_path) = match spill_paths {
        Some((o, e)) => (Some(o), Some(e)),
        None => (None, None),
    };
    let stdout_task = child
        .stdout
        .take()
        .map(|s| tokio::spawn(read_capped(s, stdout_path)));
    let stderr_task = child
        .stderr
        .take()
        .map(|s| tokio::spawn(read_capped(s, stderr_path)));

    let wait_result = timeout(COMMAND_TIMEOUT, child.wait()).await;

    let (timed_out, status_text) = match wait_result {
        Ok(Ok(status)) => {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "terminated_by_signal".to_string());
            (false, code)
        },
        Ok(Err(e)) => (false, format!("wait_error: {e}")),
        Err(_) => {
            // Timeout. Kill the whole process group on unix, then drain.
            // kill_on_drop also fires when the Child is dropped at the end
            // of this function as a final safety net.
            if let Some(pid) = child.id() {
                kill_process_group(pid as i32);
            }
            let _ = child.start_kill();
            let _ = child.wait().await;
            (true, "timed_out".to_string())
        },
    };

    // Drain reader tasks with a bounded timeout. If a grandchild kept the
    // pipe open past the kill, give up rather than hang forever.
    let stdout = drain_task(stdout_task).await;
    let stderr = drain_task(stderr_task).await;

    let duration = started_at.elapsed();
    let formatted = format_output(
        &request,
        &status_text,
        timed_out,
        duration,
        &stdout,
        &stderr,
    );
    ToolResult::Text(formatted)
}

async fn drain_task(task: Option<tokio::task::JoinHandle<CapturedStream>>) -> CapturedStream {
    match task {
        Some(t) => match timeout(IO_DRAIN_TIMEOUT, t).await {
            Ok(Ok(out)) => out,
            _ => CapturedStream::default(),
        },
        None => CapturedStream::default(),
    }
}

#[derive(Default)]
struct CapturedStream {
    /// ANSI-stripped, ≤ MAX_STREAM_PAYLOAD_BYTES bytes of the actual prefix.
    text: String,
    /// Total bytes the process emitted, even past the cap (counted regardless of spill success).
    total_bytes: u64,
    /// True iff at least one byte was sent only to the spill file (or would have been, if spill had been available).
    truncated: bool,
    /// Path of the spill file if it was created and at least one byte was successfully written.
    spill_path: Option<PathBuf>,
}

/// Read the entire stream, keeping the first `MAX_STREAM_PAYLOAD_BYTES` in
/// memory. Once that cap is crossed, the in-memory prefix is dumped to
/// `spill_path` and every subsequent byte goes only to disk. If `spill_path`
/// is `None` or the file cannot be created/written, overflow bytes are read
/// and discarded so the writer does not block on a full pipe buffer.
async fn read_capped<R>(mut reader: R, spill_path: Option<PathBuf>) -> CapturedStream
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    let mut buf: Vec<u8> = Vec::with_capacity(MAX_STREAM_PAYLOAD_BYTES);
    let mut tmp = [0u8; READ_BUFFER_BYTES];
    let mut total_bytes: u64 = 0;
    let mut truncated = false;
    let mut spill_file: Option<tokio::fs::File> = None;
    let mut spill_path_used: Option<PathBuf> = None;

    loop {
        let n = match reader.read(&mut tmp).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        total_bytes += n as u64;
        let chunk = &tmp[..n];

        if !truncated {
            let remaining_buf = MAX_STREAM_PAYLOAD_BYTES.saturating_sub(buf.len());
            let take = n.min(remaining_buf);
            buf.extend_from_slice(&chunk[..take]);
            if take < n {
                // Crossed the cap with this chunk. Open the spill file,
                // dump the full prefix we already kept, then the rest of
                // this chunk. Any failure degrades to discard.
                truncated = true;
                if let Some(p) = spill_path.as_ref() {
                    match tokio::fs::File::create(p).await {
                        Ok(mut f) => {
                            if f.write_all(&buf).await.is_ok()
                                && f.write_all(&chunk[take..]).await.is_ok()
                            {
                                spill_file = Some(f);
                                spill_path_used = Some(p.clone());
                            } else {
                                log::error!(
                                    "spill file write failed for {}; remaining bytes will be discarded",
                                    p.display()
                                );
                            }
                        },
                        Err(e) => {
                            log::error!(
                                "could not create spill file {}: {e}; overflow will be discarded",
                                p.display()
                            );
                        },
                    }
                }
            }
        } else if let Some(file) = spill_file.as_mut() {
            // Truncated: every subsequent byte goes to disk only.
            if let Err(e) = file.write_all(chunk).await {
                log::error!(
                    "spill file write failed mid-stream: {e}; remaining bytes will be discarded"
                );
                // reset state to prevent any further writing to the file.
                spill_file = None;
            }
        }
    }

    if let Some(mut file) = spill_file.take() {
        let _ = file.flush().await;
    }

    let text = strip_ansi(&String::from_utf8_lossy(&buf));
    CapturedStream {
        text,
        total_bytes,
        truncated,
        spill_path: spill_path_used,
    }
}

/// Strip CSI escape sequences (ESC `[` ... final-byte). Covers the bulk of
/// what shell tools emit (colors, cursor moves). OSC and other lesser-used
/// sequences are passed through unchanged.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            while let Some(&next) = chars.peek() {
                chars.next();
                let n = next as u32;
                if (0x40..=0x7E).contains(&n) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn format_output(
    request: &ProcessExecRequest,
    status_text: &str,
    timed_out: bool,
    duration: Duration,
    stdout: &CapturedStream,
    stderr: &CapturedStream,
) -> String {
    Element::new("shell_output")
        .attr("command", request.command.join(" "))
        .attr("workdir", request.cwd.display())
        .attr("exec_id", &request.call_id)
        .attr_if(timed_out, "timed_out", "true")
        .attr("exit_code", status_text)
        .attr("duration_ms", duration.as_millis())
        .child(format_stream("stdout", stdout))
        .child(format_stream("stderr", stderr))
        .to_string()
}

fn format_stream(name: &'static str, s: &CapturedStream) -> Element {
    let mut elem = Element::new(name)
        .attr("total_bytes", s.total_bytes)
        .attr_if(s.truncated, "truncated", "true")
        .attr_if_some(
            "full_output",
            s.spill_path.as_ref().map(|p| p.display().to_string()),
        );
    if !s.text.is_empty() || s.truncated {
        let body = if s.truncated {
            format!("{}...", s.text)
        } else {
            s.text.clone()
        };
        elem = elem.cdata(body);
    }
    elem
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_pm() -> ProcessManagerClient {
        let (mut pm, client) = ProcessManager::new();
        tokio::spawn(async move { pm.run().await });
        client
    }

    #[tokio::test]
    async fn exec_returns_stdout_and_exit_code() {
        let client = spawn_pm();
        let result = client
            .exec(ProcessExecRequest {
                session_id: Uuid::now_v7(),
                call_id: "call_stdout".into(),
                command: vec!["printf".into(), "hello".into()],
                cwd: std::env::current_dir().unwrap(),
            })
            .await
            .unwrap();

        let ToolResult::Text(text) = result else {
            panic!("expected text result");
        };
        assert!(text.contains("exit_code=\"0\""), "got:\n{text}");
        assert!(text.contains("exec_id=\"call_stdout\""), "got:\n{text}");
        assert!(text.contains("duration_ms=\""), "got:\n{text}");
        assert!(text.contains("<stdout"), "got:\n{text}");
        assert!(text.contains("<![CDATA[hello]]>"), "got:\n{text}");
        assert!(!text.contains("truncated=\"true\""), "got:\n{text}");
    }

    #[tokio::test]
    async fn exec_returns_nonzero_exit_for_failing_command() {
        let client = spawn_pm();
        let result = client
            .exec(ProcessExecRequest {
                session_id: Uuid::now_v7(),
                call_id: "call_exit".into(),
                command: vec!["sh".into(), "-c".into(), "exit 7".into()],
                cwd: std::env::current_dir().unwrap(),
            })
            .await
            .unwrap();

        let ToolResult::Text(text) = result else {
            panic!("expected text result");
        };
        assert!(text.contains("exit_code=\"7\""), "got:\n{text}");
    }

    #[tokio::test]
    async fn exec_reports_spawn_failure() {
        let client = spawn_pm();
        let err = client
            .exec(ProcessExecRequest {
                session_id: Uuid::now_v7(),
                call_id: "call_spawn_fail".into(),
                command: vec!["this-binary-does-not-exist-xyz".into()],
                cwd: std::env::current_dir().unwrap(),
            })
            .await
            .unwrap_err();

        assert!(matches!(err, ProcessManagerError::Spawn(_)));
    }

    #[tokio::test]
    async fn cancel_session_kills_running_command() {
        let client = spawn_pm();
        let session_id = Uuid::now_v7();
        let cmd = client.exec(ProcessExecRequest {
            session_id,
            call_id: "call_cancel".into(),
            // 30s sleep — would normally time out at COMMAND_TIMEOUT or
            // outlive the test if not cancelled.
            command: vec!["sleep".into(), "30".into()],
            cwd: std::env::current_dir().unwrap(),
        });

        // Cancel after a short delay so the spawn has time to land.
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            client.cancel_session(session_id).await.unwrap();
        };

        let (result, _) = tokio::join!(cmd, cancel);
        let result = result.unwrap();
        let ToolResult::Text(text) = result else {
            panic!("expected text result");
        };
        // Killed by SIGKILL -> no exit code, surfaced as terminated_by_signal.
        assert!(
            text.contains("terminated_by_signal"),
            "expected signal termination, got:\n{text}"
        );
    }

    #[tokio::test]
    async fn exec_spills_when_output_exceeds_payload_cap() {
        // Twice the payload cap of stdout — expect truncated="true", a
        // full_output path keyed by call_id, and the body to end in "...".
        let client = spawn_pm();
        let bytes = MAX_STREAM_PAYLOAD_BYTES * 2;
        let result = client
            .exec(ProcessExecRequest {
                session_id: Uuid::now_v7(),
                call_id: "call_spill".into(),
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    format!("head -c {bytes} /dev/zero | tr '\\0' 'A'"),
                ],
                cwd: std::env::current_dir().unwrap(),
            })
            .await
            .unwrap();

        let ToolResult::Text(text) = result else {
            panic!("expected text result");
        };

        // XML envelope sanity checks.
        assert!(text.contains("exit_code=\"0\""), "got:\n{text}");
        assert!(text.contains("exec_id=\"call_spill\""), "got:\n{text}");
        assert!(text.contains("truncated=\"true\""), "got:\n{text}");
        assert!(
            text.contains("full_output=\"/tmp/scry/call_spill/out\""),
            "got:\n{text}"
        );
        assert!(
            text.contains(&format!("total_bytes=\"{bytes}\"")),
            "got:\n{text}"
        );

        // The body ends with the "..." marker we append on truncation.
        assert!(text.contains("...]]></stdout>"), "got:\n{text}");

        // The on-disk file holds the full output, not just the prefix.
        let on_disk = std::fs::read("/tmp/scry/call_spill/out").expect("spill file should exist");
        assert_eq!(on_disk.len(), bytes, "spill file should hold full output");
    }

    #[test]
    fn strip_ansi_removes_color_codes() {
        let input = "\x1b[31mred\x1b[0m plain";
        assert_eq!(strip_ansi(input), "red plain");
    }

    #[test]
    fn strip_ansi_preserves_utf8() {
        let input = "\x1b[1mhéllo\x1b[0m 世界";
        assert_eq!(strip_ansi(input), "héllo 世界");
    }
}
