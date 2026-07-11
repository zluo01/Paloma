use std::{collections::BTreeMap, path::PathBuf, sync::LazyLock};

pub(crate) const APP_NAME: &str = "scry";
pub(crate) const DATABASE_FILE: &str = "main.db";

pub const RENDER_CHANNEL_CAPACITY: usize = 32;
pub const SESSION_MANAGER_CHANNEL_CAPACITY: usize = 128;
pub const TURN_MANAGER_CHANNEL_CAPACITY: usize = 128;
pub const PERMISSION_WORKFLOW_CHANNEL_CAPACITY: usize = 32;

/// How long a resolved permission request is kept in memory after it
/// completes before it is evicted.
pub const PERMISSION_EVICT_TTL_SECS: u64 = 600;

/// Per-payload cap on tool output shown inline to the model; anything past
/// these spills to a file under [`SPILL_ROOT`] and the inline text freezes
/// at this prefix. Shared by all tools that will output to llm.
pub const MAX_STREAM_PAYLOAD_BYTES: usize = 50 * 1024;

/// Root directory where spilled tool output lives, keyed by call id; never
/// cleaned by the process — relies on the system tmp lifecycle.
pub static SPILL_ROOT: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from("/tmp").join(APP_NAME));

pub static ENVIRONMENT_CONTEXT: LazyLock<BTreeMap<&'static str, String>> =
    LazyLock::new(build_environment_context);

/// Static system prompt sent as the `instructions` field on every LLM call.
/// Describes role, tool contract, and behavioral rules.
///
/// Complements `ENVIRONMENT_CONTEXT`, which is added once as the first message
/// of a session and travels back to the model in the replayed history on every
/// turn — the instruction tells the model *how* to behave, the context tells
/// it *where* it is running (os, arch, shell, home).
pub const INSTRUCTION: &str = include_str!("instruction.md");

fn build_environment_context() -> BTreeMap<&'static str, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".into());
    let home = std::env::var("HOME").unwrap_or_else(|_| "unknown".into());

    BTreeMap::from([
        ("os", std::env::consts::OS.to_string()),
        ("os_family", std::env::consts::FAMILY.to_string()),
        ("arch", std::env::consts::ARCH.to_string()),
        ("home", home),
        ("shell", shell),
    ])
}
