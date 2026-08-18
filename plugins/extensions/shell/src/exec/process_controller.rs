use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use dashmap::DashMap;
use paloma_extension_protocol::v1::ToolContent;
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    time::timeout,
};
use uuid::Uuid;

use crate::exec::process_group::{ProcessGroup, ProcessGroupHolder};

/// Wall-clock budget for a single command before it is force-killed.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
/// Grace period to flush buffered pipe bytes after the process group is killed.
const IO_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) struct ProcessExecRequest {
    pub session_id: Uuid,
    pub call_id: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
}

pub(crate) struct ProcessController {
    sessions: Arc<DashMap<Uuid, Vec<Arc<ProcessGroup>>>>,
}

impl ProcessController {
    pub(crate) fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    pub(crate) async fn exec(&self, request: ProcessExecRequest) -> Result<ToolContent> {
        if request.command.is_empty() {
            return Err(ProcessControllerError::Spawn("empty command".into()));
        }
        let mut cmd = Command::new(&request.command[0]);
        cmd.args(&request.command[1..])
            .current_dir(&request.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        ProcessGroup::prepare(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| ProcessControllerError::Spawn(e.to_string()))?;

        let group = match ProcessGroup::from_child(&child) {
            Ok(group) => {
                let group = Arc::new(group);
                self.sessions
                    .entry(request.session_id)
                    .or_default()
                    .push(group.clone());
                Some(group)
            },
            Err(e) => match child.try_wait() {
                // cmd is done, nothing can be killed or cleaned, no-op
                Ok(Some(_)) => None,
                // fail to wrap to process group
                // impossible on Unix, possible on Windows as we use kernel job to manage cmd process lifetime.
                // If fail to wrap, kill early such that if cmd is some long-running cpu intensive jobs,
                // it won't become zombie process due to no way for cancelling.
                _ => {
                    let _ = child.start_kill();
                    return Err(ProcessControllerError::Attach(e.to_string()));
                },
            },
        };

        let sessions = self.sessions.clone();
        let session_id = request.session_id;
        let task = tokio::spawn(async move {
            let result = run_to_completion(child, request, group.as_deref()).await;
            if let Some(group) = group {
                remove_group(&sessions, session_id, &group);
            }
            result
        });
        task.await
            .map_err(|e| ProcessControllerError::Task(e.to_string()))?
    }

    pub(crate) fn cancel_session(&self, session_id: Uuid) {
        if let Some((_, groups)) = self.sessions.remove(&session_id) {
            for group in groups {
                group.kill();
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessControllerError {
    #[error("failed to spawn process: {0}")]
    Spawn(String),

    #[error("failed to attach process group: {0}")]
    Attach(String),

    #[error("process task failed: {0}")]
    Task(String),

    #[error("failed to wait on process: {0}")]
    Wait(String),
}

type Result<T> = std::result::Result<T, ProcessControllerError>;

fn remove_group(
    sessions: &DashMap<Uuid, Vec<Arc<ProcessGroup>>>,
    session_id: Uuid,
    group: &Arc<ProcessGroup>,
) {
    let became_empty = match sessions.get_mut(&session_id) {
        Some(mut vec) => {
            vec.retain(|g| !Arc::ptr_eq(g, group));
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

async fn run_to_completion(
    mut child: Child,
    request: ProcessExecRequest,
    group: Option<&ProcessGroup>,
) -> Result<ToolContent> {
    let started_at = std::time::Instant::now();
    let stdout_task = child.stdout.take().map(|s| tokio::spawn(exhaust(s)));
    let stderr_task = child.stderr.take().map(|s| tokio::spawn(exhaust(s)));

    let wait_result = timeout(COMMAND_TIMEOUT, child.wait()).await;

    let (timed_out, status_text) = match wait_result {
        Ok(Ok(status)) => {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "terminated_by_signal".to_string());
            (false, code)
        },
        Ok(Err(e)) => return Err(ProcessControllerError::Wait(e.to_string())),
        Err(_) => {
            // Timeout. Kill the whole process tree, then drain.
            // kill_on_drop also fires when the Child is dropped at the end
            // of this function as a final safety net.
            if let Some(group) = group {
                group.kill();
            }
            let _ = child.start_kill();
            let _ = child.wait().await;
            (true, "timed_out".to_string())
        },
    };

    let stdout = drain_task(stdout_task, group).await;
    let stderr = drain_task(stderr_task, group).await;

    let duration = started_at.elapsed();
    Ok(ToolContent::new("shell_output")
        .attr("command", request.command.join(" "))
        .attr("workdir", request.cwd.display())
        .attr("exec_id", &request.call_id)
        .attr_if(timed_out, "timed_out", "true")
        .attr("exit_code", status_text)
        .attr("duration_ms", duration.as_millis())
        .child(stream_content("stdout", stdout))
        .child(stream_content("stderr", stderr)))
}

fn stream_content(name: &str, text: Option<String>) -> ToolContent {
    match text {
        Some(text) if !text.is_empty() => ToolContent::new(name).cdata(text),
        _ => ToolContent::new(name),
    }
}

async fn drain_task(
    task: Option<tokio::task::JoinHandle<String>>,
    group: Option<&ProcessGroup>,
) -> Option<String> {
    let mut task = task?;
    match timeout(IO_DRAIN_TIMEOUT, &mut task).await {
        Ok(joined) => joined.ok(),
        Err(_) => {
            if let Some(group) = group {
                group.kill();
            }
            timeout(IO_DRAIN_TIMEOUT, task).await.ok()?.ok()
        },
    }
}

async fn exhaust<R>(mut reader: R) -> String
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    let mut buf: Vec<u8> = Vec::new();
    let _ = reader.read_to_end(&mut buf).await;
    strip_ansi(&String::from_utf8_lossy(&buf))
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

#[cfg(test)]
mod tests {
    use paloma_extension_protocol::v1::tool_content;

    use super::*;

    fn attr<'a>(content: &'a ToolContent, key: &str) -> Option<&'a str> {
        content
            .attributes
            .iter()
            .find(|attribute| attribute.key == key)
            .map(|attribute| attribute.value.as_str())
    }

    fn child<'a>(content: &'a ToolContent, tag: &str) -> &'a ToolContent {
        content
            .children()
            .iter()
            .find(|child| child.tag == tag)
            .unwrap_or_else(|| panic!("missing child {tag}"))
    }

    fn text_body(content: &ToolContent) -> Option<&str> {
        match content.body.as_ref()? {
            tool_content::Body::Text(text) => Some(text),
            tool_content::Body::Binary(_) | tool_content::Body::Children(_) => None,
        }
    }

    #[tokio::test]
    async fn exec_reports_spawn_failure() {
        let pm = ProcessController::new();
        let err = pm
            .exec(ProcessExecRequest {
                session_id: Uuid::now_v7(),
                call_id: "call_spawn_fail".into(),
                command: vec!["this-binary-does-not-exist-xyz".into()],
                cwd: std::env::current_dir().unwrap(),
            })
            .await
            .unwrap_err();

        assert!(matches!(err, ProcessControllerError::Spawn(_)));
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

    #[cfg(unix)]
    mod unix {
        use super::*;

        #[tokio::test]
        async fn exec_returns_stdout_and_exit_code() {
            let pm = ProcessController::new();
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_stdout".into(),
                    command: vec!["printf".into(), "hello".into()],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(content.tag, "shell_output");
            assert_eq!(attr(&content, "exit_code"), Some("0"));
            assert_eq!(attr(&content, "exec_id"), Some("call_stdout"));
            assert!(attr(&content, "duration_ms").is_some());
            assert_eq!(attr(&content, "timed_out"), None);
            assert_eq!(text_body(child(&content, "stdout")), Some("hello"));
            assert_eq!(text_body(child(&content, "stderr")), None);
        }

        #[tokio::test]
        async fn exec_returns_nonzero_exit_for_failing_command() {
            let pm = ProcessController::new();
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_exit".into(),
                    command: vec!["sh".into(), "-c".into(), "exit 7".into()],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(attr(&content, "exit_code"), Some("7"));
        }

        #[tokio::test]
        async fn cancel_session_kills_running_command() {
            let pm = ProcessController::new();
            let session_id = Uuid::now_v7();
            let cmd = pm.exec(ProcessExecRequest {
                session_id,
                call_id: "call_cancel".into(),
                // 30s sleep — would normally time out at COMMAND_TIMEOUT or
                // outlive the test if not cancelled.
                command: vec!["sleep".into(), "30".into()],
                cwd: std::env::current_dir().unwrap(),
            });

            // Give the child a moment to become its own process-group leader
            // before killing the group.
            let cancel = async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                pm.cancel_session(session_id);
            };

            let (result, _) = tokio::join!(cmd, cancel);
            let content = result.unwrap();
            // Killed by SIGKILL -> no exit code, surfaced as
            // terminated_by_signal.
            assert_eq!(attr(&content, "exit_code"), Some("terminated_by_signal"));
        }

        #[tokio::test]
        async fn background_grandchild_does_not_discard_buffered_output() {
            let pm = ProcessController::new();
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_bg".into(),
                    // sh exits immediately; the backgrounded sleep inherits the
                    // stdout pipe and holds it past the drain timeout
                    command: vec!["sh".into(), "-c".into(), "echo BUILD_OK; sleep 30 &".into()],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(attr(&content, "exit_code"), Some("0"));
            assert_eq!(text_body(child(&content, "stdout")), Some("BUILD_OK\n"));
        }

        #[tokio::test]
        async fn exec_returns_large_output_in_full() {
            // The plugin never truncates: core owns the size policy.
            let pm = ProcessController::new();
            let bytes = 200 * 1024;
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_large".into(),
                    command: vec![
                        "sh".into(),
                        "-c".into(),
                        format!("head -c {bytes} /dev/zero | tr '\\0' 'A'"),
                    ],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(attr(&content, "exit_code"), Some("0"));
            let stdout = text_body(child(&content, "stdout")).expect("stdout body");
            assert_eq!(stdout.len(), bytes);
        }
    }

    #[cfg(windows)]
    mod windows {
        use super::*;

        #[tokio::test]
        async fn exec_returns_stdout_and_exit_code() {
            let pm = ProcessController::new();
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_stdout".into(),
                    command: vec![
                        "powershell".into(),
                        "-NoProfile".into(),
                        "-Command".into(),
                        "[Console]::Out.Write('hello')".into(),
                    ],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(content.tag, "shell_output");
            assert_eq!(attr(&content, "exit_code"), Some("0"));
            assert_eq!(attr(&content, "exec_id"), Some("call_stdout"));
            assert!(attr(&content, "duration_ms").is_some());
            assert_eq!(attr(&content, "timed_out"), None);
            assert_eq!(text_body(child(&content, "stdout")), Some("hello"));
            assert_eq!(text_body(child(&content, "stderr")), None);
        }

        #[tokio::test]
        async fn exec_returns_nonzero_exit_for_failing_command() {
            let pm = ProcessController::new();
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_exit".into(),
                    command: vec!["cmd".into(), "/d".into(), "/c".into(), "exit 7".into()],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(attr(&content, "exit_code"), Some("7"));
        }

        #[tokio::test]
        async fn cancel_session_kills_running_command() {
            let pm = ProcessController::new();
            let session_id = Uuid::now_v7();
            let cmd = pm.exec(ProcessExecRequest {
                session_id,
                call_id: "call_cancel".into(),
                // ~30s of ping — would normally time out at COMMAND_TIMEOUT or
                // outlive the test if not cancelled.
                command: vec!["ping".into(), "-n".into(), "31".into(), "127.0.0.1".into()],
                cwd: std::env::current_dir().unwrap(),
            });

            // Give the spawn a moment to be assigned to the job before
            // terminating it.
            let cancel = async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                pm.cancel_session(session_id);
            };

            let (result, _) = tokio::join!(cmd, cancel);
            let content = result.unwrap();
            // TerminateJobObject sets every process's exit code to 1.
            assert_eq!(attr(&content, "exit_code"), Some("1"));
        }

        #[tokio::test]
        async fn background_grandchild_does_not_discard_buffered_output() {
            let pm = ProcessController::new();
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_bg".into(),
                    // cmd exits immediately; the powershell started with /b
                    // inherits the stdout pipe and holds it past the drain
                    // timeout
                    command: vec![
                        "cmd".into(),
                        "/d".into(),
                        "/c".into(),
                        "echo BUILD_OK& start /b powershell -NoProfile -Command Start-Sleep 30"
                            .into(),
                    ],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(attr(&content, "exit_code"), Some("0"));
            assert_eq!(text_body(child(&content, "stdout")), Some("BUILD_OK\r\n"));
        }

        #[tokio::test]
        async fn exec_returns_large_output_in_full() {
            // The plugin never truncates: core owns the size policy.
            let pm = ProcessController::new();
            let bytes = 200 * 1024;
            let content = pm
                .exec(ProcessExecRequest {
                    session_id: Uuid::now_v7(),
                    call_id: "call_large".into(),
                    command: vec![
                        "powershell".into(),
                        "-NoProfile".into(),
                        "-Command".into(),
                        format!("[Console]::Out.Write('A' * {bytes})"),
                    ],
                    cwd: std::env::current_dir().unwrap(),
                })
                .await
                .unwrap();

            assert_eq!(attr(&content, "exit_code"), Some("0"));
            let stdout = text_body(child(&content, "stdout")).expect("stdout body");
            assert_eq!(stdout.len(), bytes);
        }
    }
}
