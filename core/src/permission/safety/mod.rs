#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::{safety_check, strip_transparent_command};
#[cfg(windows)]
pub(crate) use windows::{safety_check, strip_transparent_command};

use crate::permission::ArgvDecision;

/// Ripgrep is the same on every platform
fn ripgrep_check(command: &[String]) -> ArgvDecision {
    const UNSAFE_RIPGREP_OPTIONS_WITH_ARGS: &[&str] = &[
        // Takes an arbitrary command that is executed for each match.
        "--pre",
        // Takes a command that can be used to obtain the local hostname.
        "--hostname-bin",
    ];
    const UNSAFE_RIPGREP_OPTIONS_WITHOUT_ARGS: &[&str] = &[
        // Calls out to other decompression tools, so do not auto-approve
        // out of an abundance of caution.
        "--search-zip",
        "-z",
    ];

    let has = |opts: &[&str]| {
        command.iter().any(|a| {
            opts.contains(&a.as_str()) || opts.iter().any(|o| a.starts_with(&format!("{o}=")))
        })
    };

    if has(UNSAFE_RIPGREP_OPTIONS_WITH_ARGS) {
        ArgvDecision::AskNoPersist
    } else if has(UNSAFE_RIPGREP_OPTIONS_WITHOUT_ARGS) {
        ArgvDecision::Unknown
    } else {
        ArgvDecision::Allow
    }
}

#[cfg_attr(windows, allow(dead_code))]
#[derive(Debug, PartialEq, thiserror::Error)]
pub(crate) enum StripTransparentWrapperError {
    #[error("{wrapper}: option {flag} requires a value")]
    MissingFlagValue { wrapper: &'static str, flag: String },
    #[error("{wrapper}: invalid value {value:?} for option {flag}")]
    InvalidFlagValue {
        wrapper: &'static str,
        flag: String,
        value: String,
    },
    #[error("{wrapper}: unrecognized option {option}")]
    UnknownOption {
        wrapper: &'static str,
        option: String,
    },
    #[error("invalid {wrapper} command, found: {found:?}")]
    InvalidCommand {
        wrapper: &'static str,
        /// The offending token; empty when the command ends too early.
        found: String,
    },
    #[error("{wrapper}: {construct} cannot be safely stripped")]
    UnsafeToStrip {
        wrapper: &'static str,
        construct: String,
    },
    #[error("wrapper nesting exceeds depth limit")]
    DepthExceeded,
}
