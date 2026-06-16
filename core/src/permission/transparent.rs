//! Not a good solution but a basic way to strip the transparent wrapper output the actual command
//! Such that user can approve on the actual one.

use std::sync::LazyLock;

use regex::Regex;

/// Wrapper layers a command may realistically nest before we refuse.
const MAX_DEPTH: usize = 8;

/// The duration shape both GNU and BSD `timeout` accept: a non-negative
/// integer or real (decimal) number, with an optional leading `+` and an
/// optional exponent (`strtod` semantics), and an optional unit suffix —
/// `s` seconds (the default), `m` minutes, `h` hours, `d` days.
/// `[0-9]` rather than `\d`, which would also match non-ASCII digits.
static VALID_DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\+?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?[smhd]?$")
        .expect("valid duration regex for timeout")
});

/// A `nice` adjustment: an optionally signed integer.
static VALID_ADJUSTMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[+-]?[0-9]+$").expect("valid adjustment regex for nice"));

/// A signal number, or a POSIX signal name with optional `SIG` prefix,
/// case-insensitive.
static VALID_SIGNAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?i)(SIG)?(HUP|INT|QUIT|ILL|TRAP|ABRT|BUS|FPE|KILL|USR1|SEGV|USR2|PIPE|ALRM|TERM|CHLD|CONT|STOP|TSTP|TTIN|TTOU|URG|XCPU|XFSZ|VTALRM|PROF|WINCH|IO|SYS)$|^[0-9]+$",
    )
    .expect("valid signal regex")
});

/// Strip transparent wrapper layers from a single command's argv, returning
/// the innermost command.
pub(crate) fn strip_transparent_command(argv: &[String]) -> Result<&[String]> {
    let mut inner = argv;
    let mut depth = 0;
    while let Some(name) = inner.first().and_then(|arg0| wrapper_name(arg0)) {
        // `--help`/`--version` make these tools print and exit without
        // running anything — not a wrapper invocation, nothing to strip.
        if matches!(
            inner.get(1).map(String::as_str),
            Some("--help" | "--version")
        ) {
            break;
        }
        let (wrapper, rest): (&'static str, &[String]) = match name {
            "timeout" => ("timeout", strip_timeout(&inner[1..])?),
            "env" => ("env", strip_env(&inner[1..])?),
            "nice" => ("nice", strip_nice(&inner[1..])?),
            "nohup" => ("nohup", strip_nohup(&inner[1..])?),
            _ => break,
        };
        if rest.is_empty() {
            // No inner command. `env` and `nice` print their state and exit —
            // a valid terminal invocation, not a wrapper — so leave the argv
            // intact and let it classify on its own. `timeout` and `nohup`
            // require a command, so a missing one is an error.
            match wrapper {
                "env" | "nice" => break,
                _ => {
                    return Err(StripTransparentWrapperError::InvalidCommand {
                        wrapper,
                        found: String::new(),
                    });
                },
            }
        }
        // A revealed "command" that still looks like a flag means the wrapper
        // invocation was malformed; refuse rather than hand back nonsense.
        if rest[0].starts_with('-') {
            return Err(StripTransparentWrapperError::InvalidCommand {
                wrapper,
                found: rest[0].clone(),
            });
        }
        inner = rest;
        depth += 1;
        if depth > MAX_DEPTH {
            return Err(StripTransparentWrapperError::DepthExceeded);
        }
    }
    Ok(inner)
}

/// Only bare names are eligible for stripping: `./timeout` or `/opt/x/timeout`
/// may be a different program entirely, so those fall through unstriped and
/// classify conservatively upstream.
fn wrapper_name(arg0: &str) -> Option<&str> {
    (!arg0.contains('/')).then_some(arg0)
}

/// GNU: <https://www.gnu.org/software/coreutils/manual/html_node/timeout-invocation.html>
/// BSD/macOS: <https://man.freebsd.org/cgi/man.cgi?query=timeout>
fn strip_timeout(args: &[String]) -> Result<&[String]> {
    const WRAPPER: &str = "timeout";
    let mut i = 0;
    while let Some(arg) = args.get(i).map(String::as_str) {
        if !arg.starts_with('-') {
            break;
        }
        // `--` ends option parsing; the duration must follow.
        if arg == "--" {
            i += 1;
            break;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v)),
            _ => (arg, None),
        };
        match flag {
            "-f" | "--foreground" | "-p" | "--preserve-status" | "-v" | "--verbose"
                if inline.is_none() => {},
            "-s" | "--signal" | "-k" | "--kill-after" => {
                // -s KILL — short, next token (shorts have no = form here)
                // --signal KILL — long, next token
                // --signal=KILL — long, inline
                let value = match inline {
                    Some(v) => v,
                    // for the first two cases
                    None => {
                        i += 1;
                        args.get(i).map(String::as_str).ok_or_else(|| {
                            StripTransparentWrapperError::MissingFlagValue {
                                wrapper: WRAPPER,
                                flag: flag.to_owned(),
                            }
                        })?
                    },
                };
                let valid = if matches!(flag, "-s" | "--signal") {
                    VALID_SIGNAL.is_match(value)
                } else {
                    VALID_DURATION.is_match(value)
                };
                if !valid {
                    return Err(StripTransparentWrapperError::InvalidFlagValue {
                        wrapper: WRAPPER,
                        flag: flag.to_owned(),
                        value: value.to_owned(),
                    });
                }
            },
            _ => {
                return Err(StripTransparentWrapperError::UnknownOption {
                    wrapper: WRAPPER,
                    option: arg.to_owned(),
                });
            },
        }
        i += 1;
    }
    match args.get(i) {
        // somehow missing inner command
        None => Err(StripTransparentWrapperError::InvalidCommand {
            wrapper: WRAPPER,
            found: String::new(),
        }),
        Some(duration) => {
            // the actual value is a valid duration for timeout
            if VALID_DURATION.is_match(duration) {
                Ok(&args[i + 1..])
            } else {
                Err(StripTransparentWrapperError::InvalidCommand {
                    wrapper: WRAPPER,
                    found: duration.clone(),
                })
            }
        },
    }
}

/// GNU: <https://www.gnu.org/software/coreutils/manual/html_node/env-invocation.html>
/// BSD/macOS: <https://man.freebsd.org/cgi/man.cgi?query=env>
fn strip_env(args: &[String]) -> Result<&[String]> {
    const WRAPPER: &str = "env";
    let mut i = 0;
    let mut print_mode = false;
    while let Some(arg) = args.get(i).map(String::as_str) {
        if !arg.starts_with('-') {
            break;
        }
        // `--` ends option parsing; the command follows.
        if arg == "--" {
            i += 1;
            break;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v)),
            _ => (arg, None),
        };
        match flag {
            // `-` is the obsolete spelling of `-i`; `-v/--debug` and
            // `--list-signal-handling` only print diagnostics to stderr.
            "-" | "-i" | "--ignore-environment" | "-v" | "--debug" | "--list-signal-handling"
                if inline.is_none() => {},
            "-u" | "--unset" => {
                // The value is a variable name to remove; env accepts any
                // name, so just consume it without validation.
                if inline.is_none() {
                    i += 1;
                    if args.get(i).is_none() {
                        return Err(StripTransparentWrapperError::MissingFlagValue {
                            wrapper: WRAPPER,
                            flag: flag.to_owned(),
                        });
                    }
                }
            },
            // Optional-argument long options: getopt never consumes the next
            // token for these, so the value is inline-only — a comma-separated
            // signal list.
            "--default-signal" | "--ignore-signal" | "--block-signal" => {
                if let Some(list) = inline {
                    if list.is_empty() || !list.split(',').all(|s| VALID_SIGNAL.is_match(s)) {
                        return Err(StripTransparentWrapperError::InvalidFlagValue {
                            wrapper: WRAPPER,
                            flag: flag.to_owned(),
                            value: list.to_owned(),
                        });
                    }
                }
            },
            // These change execution semantics, so judging the inner command
            // as if it ran clean would be unsound:
            // `-S` re-splits a string under env's own quoting rules;
            // `-C` changes the working directory the inner command runs in;
            // `-a` makes the inner command run under a different argv[0];
            // `-L`/`-U` replace the environment from login.conf (BSD);
            // `-P` changes where the utility binary is looked up (BSD).
            "-S" | "--split-string" | "-C" | "--chdir" | "-a" | "--argv0" | "-L" | "-U" | "-P" => {
                return Err(StripTransparentWrapperError::UnsafeToStrip {
                    wrapper: WRAPPER,
                    construct: flag.to_owned(),
                });
            },
            // Print-mode: `env -0` dumps the environment NUL-separated. Valid
            // on its own, but the real tool refuses to combine it with a
            // command — checked once option parsing is done.
            "-0" | "--null" if inline.is_none() => print_mode = true,
            _ => {
                return Err(StripTransparentWrapperError::UnknownOption {
                    wrapper: WRAPPER,
                    option: arg.to_owned(),
                });
            },
        }
        i += 1;
    }
    // In print mode the real tool refuses a trailing command; with no command
    // it just prints the environment, handled by the empty return below.
    if print_mode {
        if let Some(command) = args.get(i) {
            return Err(StripTransparentWrapperError::InvalidCommand {
                wrapper: WRAPPER,
                found: command.clone(),
            });
        }
    }
    // env treats any operand containing `=` as an assignment; assignments can
    // change what the inner command does (LD_PRELOAD, PATH, …), never strip.
    if let Some(assignment) = args.get(i).filter(|arg| arg.contains('=')) {
        return Err(StripTransparentWrapperError::UnsafeToStrip {
            wrapper: WRAPPER,
            construct: assignment.clone(),
        });
    }
    Ok(&args[i..])
}

/// GNU: <https://www.gnu.org/software/coreutils/manual/html_node/nice-invocation.html>
/// BSD/macOS: <https://man.freebsd.org/cgi/man.cgi?query=nice>
fn strip_nice(args: &[String]) -> Result<&[String]> {
    const WRAPPER: &str = "nice";
    let mut i = 0;
    while let Some(arg) = args.get(i).map(String::as_str) {
        if !arg.starts_with('-') {
            break;
        }
        // `--` ends option parsing; the duration must follow.
        if arg == "--" {
            i += 1;
            break;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v)),
            _ => (arg, None),
        };

        match flag {
            "-n" | "--adjustment" => {
                let value = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        args.get(i).map(String::as_str).ok_or_else(|| {
                            StripTransparentWrapperError::MissingFlagValue {
                                wrapper: WRAPPER,
                                flag: flag.to_owned(),
                            }
                        })?
                    },
                };
                if !VALID_ADJUSTMENT.is_match(value) {
                    return Err(StripTransparentWrapperError::InvalidFlagValue {
                        wrapper: WRAPPER,
                        flag: flag.to_owned(),
                        value: value.to_owned(),
                    });
                }
            },
            _ => {
                return Err(StripTransparentWrapperError::UnknownOption {
                    wrapper: WRAPPER,
                    option: arg.to_owned(),
                });
            },
        }
        i += 1;
    }
    Ok(&args[i..])
}

/// GNU: <https://www.gnu.org/software/coreutils/manual/html_node/nohup-invocation.html>
/// BSD/macOS: <https://man.freebsd.org/cgi/man.cgi?query=nohup>
fn strip_nohup(args: &[String]) -> Result<&[String]> {
    match args.first().map(String::as_str) {
        // `--` ends option parsing; the command follows.
        Some("--") => Ok(&args[1..]),
        Some(first) if first.starts_with('-') => Err(StripTransparentWrapperError::UnknownOption {
            wrapper: "nohup",
            option: first.to_owned(),
        }),
        _ => Ok(args),
    }
}

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

type Result<T> = std::result::Result<T, StripTransparentWrapperError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    fn striped(parts: &[&str], inner: &[&str]) {
        let args = argv(parts);
        assert_eq!(
            strip_transparent_command(&args).expect("strip"),
            argv(inner),
            "{parts:?}"
        );
    }

    fn strip_err(parts: &[&str]) -> StripTransparentWrapperError {
        let args = argv(parts);
        strip_transparent_command(&args).expect_err("expected strip error")
    }

    // ------------------------------------------------------------------
    // strip_timeout
    // ------------------------------------------------------------------

    #[test]
    fn strip_timeout_bare_duration() {
        for duration in [
            "75", "1.5", "30s", "5m", "2h", "1d", "1.5h", "0.5s", "0", "0s", "+5", "+5s", "1e3",
            "1.5e2h", "2E2s", "1e-1",
        ] {
            let args = argv(&[duration, "cargo", "build"]);
            assert_eq!(
                strip_timeout(&args).expect("strip timeout should succeed"),
                argv(&["cargo", "build"]),
                "{duration:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_duration_invalid_duration_format() {
        for duration in [
            "5x", "abc", "1.5.2", "5h30m", ".5", "5.", "", "1,5", "5S", "1.5M", "1H", "1D", "٥",
            "５", "s", " ", "e3", "5e", "++5", "5e1.5",
        ] {
            let args = argv(&[duration, "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidCommand { found, .. }) if found == duration
                ),
                "{duration:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_signal_short_flag() {
        for signal in ["KILL", "SIGKILL", "kill", "sigterm", "9", "15"] {
            let args = argv(&["-s", signal, "5", "cmd"]);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(&["cmd"]),
                "{signal:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_signal_long_flag() {
        for signal in ["KILL", "SIGKILL", "kill", "sigterm", "9", "15"] {
            let args = argv(&["--signal", signal, "5", "cmd"]);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(&["cmd"]),
                "{signal:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_signal_long_flag_inline() {
        for signal in ["KILL", "SIGKILL", "kill", "sigterm", "9", "15"] {
            let args = argv(&[format!("--signal={signal}").as_str(), "5", "cmd"]);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(&["cmd"]),
                "{signal:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_invalid_signal_short_values() {
        for signal in ["cargo", "KILL9", "SIG", "-9", ""] {
            let args = argv(&["-s", signal, "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value, .. })
                        if flag == "-s" && value == signal
                ),
                "{signal:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_invalid_signal_long_values() {
        for signal in ["cargo", "KILL9", "SIG", "-9", ""] {
            let args = argv(&["--signal", signal, "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value, .. })
                        if flag == "--signal" && value == signal
                ),
                "{signal:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_invalid_signal_long_inline() {
        for signal in ["cargo", "KILL9", "SIG", "-9", "", "KILL=extra"] {
            let args = argv(&[format!("--signal={signal}").as_str(), "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value, .. })
                        if flag == "--signal" && value == signal
                ),
                "{signal:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_kill_after_short_flag() {
        for duration in ["10", "30s", "1.5m"] {
            let args = argv(&["-k", duration, "5", "cmd"]);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(&["cmd"]),
                "{duration:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_kill_after_long_flag() {
        for duration in ["10", "30s", "1.5m"] {
            let args = argv(&["--kill-after", duration, "5", "cmd"]);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(&["cmd"]),
                "{duration:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_kill_after_long_flag_inline() {
        for duration in ["10", "30s", "1.5m"] {
            let args = argv(&[format!("--kill-after={duration}").as_str(), "5", "cmd"]);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(&["cmd"]),
                "{duration:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_invalid_kill_after_short_values() {
        for value in ["KILL", "abc", "5x", "5S", "1.5M", ""] {
            let args = argv(&["-k", value, "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value: v, .. })
                        if flag == "-k" && v == value
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_invalid_kill_after_long_values() {
        for value in ["KILL", "abc", "5x", "5S", "1.5M", ""] {
            let args = argv(&["--kill-after", value, "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value: v, .. })
                        if flag == "--kill-after" && v == value
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_invalid_kill_after_long_inline() {
        for value in ["KILL", "abc", "5x", "5S", "1.5M", ""] {
            let args = argv(&[format!("--kill-after={value}").as_str(), "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value: v, .. })
                        if flag == "--kill-after" && v == value
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_no_value_flags() {
        for flag in [
            "-f",
            "--foreground",
            "-p",
            "--preserve-status",
            "-v",
            "--verbose",
        ] {
            let args = argv(&[flag, "5", "cmd"]);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(&["cmd"]),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_timeout_no_value_flags_with_space_value() {
        for flag in [
            "-f",
            "--foreground",
            "-p",
            "--preserve-status",
            "-v",
            "--verbose",
        ] {
            let args = argv(&[flag, "x", "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidCommand { found, .. })
                        if found == "x"
                ),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_timeout_no_value_flags_reject_inline_value() {
        for flag in ["--foreground=x", "--preserve-status=x", "--verbose=x"] {
            let args = argv(&[flag, "5", "cmd"]);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::UnknownOption { option, .. })
                        if option == flag
                ),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_timeout_multiple_flags_combined() {
        for (parts, inner) in [
            (
                &["-v", "-f", "-p", "-s", "KILL", "5", "cmd"][..],
                &["cmd"][..],
            ),
            (
                &["-s", "KILL", "-k", "10", "75", "cargo", "build"][..],
                &["cargo", "build"][..],
            ),
            (
                &["--foreground", "--signal=TERM", "-k", "5s", "30", "ls"][..],
                &["ls"][..],
            ),
            (
                &["-p", "--kill-after", "10", "-s", "9", "5m", "make", "-j4"][..],
                &["make", "-j4"][..],
            ),
            (
                &[
                    "--verbose",
                    "--preserve-status",
                    "--signal",
                    "HUP",
                    "1.5h",
                    "cmd",
                    "-v",
                ][..],
                &["cmd", "-v"][..],
            ),
        ] {
            let args = argv(parts);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(inner),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_combined_value_flag_swallows_next_flag() {
        for (parts, flag, value) in [
            (&["-s", "-f", "5", "cmd"][..], "-s", "-f"),
            (&["-v", "-k", "-p", "5", "cmd"][..], "-k", "-p"),
        ] {
            let args = argv(parts);
            assert!(
                matches!(
                    strip_timeout(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag: f, value: v, .. })
                        if f == flag && v == value
                ),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_combined_unknown_flag() {
        let args = argv(&["-v", "-x", "5", "cmd"]);
        assert!(matches!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::UnknownOption { option, .. }) if option == "-x"
        ));
    }

    #[test]
    fn strip_timeout_combined_invalid_duration() {
        let args = argv(&["-f", "-s", "KILL", "5x", "cmd"]);
        assert!(matches!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::InvalidCommand { found, .. }) if found == "5x"
        ));
    }

    #[test]
    fn strip_timeout_combined_dangling_value_flag() {
        let args = argv(&["-v", "-s", "KILL", "-k"]);
        assert!(matches!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::MissingFlagValue { flag, .. }) if flag == "-k"
        ));
    }

    #[test]
    fn strip_timeout_options_stop_at_duration() {
        // This is not a correctness check but behavior check
        // even for malform command, as long as we hit a duration properly
        // strip time will consider it as a good command before the duration
        for (parts, inner) in [
            (&["5", "cmd", "-s", "KILL"][..], &["cmd", "-s", "KILL"][..]),
            (&["5", "cmd", "--verbose"][..], &["cmd", "--verbose"][..]),
            (&["5", "timeout"][..], &["timeout"][..]),
            // Even a flag-looking token right after the duration is the
            // inner command, exactly like the real tool parses it.
            (&["-v", "5", "-f", "ls"][..], &["-f", "ls"][..]),
        ] {
            let args = argv(parts);
            assert_eq!(
                strip_timeout(&args).expect("strip"),
                argv(inner),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_missing_pieces() {
        assert!(matches!(
            strip_timeout(&[]),
            Err(StripTransparentWrapperError::InvalidCommand { .. })
        ));

        let args = argv(&["-v"]);
        assert!(matches!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::InvalidCommand { .. })
        ));

        let args = argv(&["-s"]);
        assert!(matches!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::MissingFlagValue { flag, .. }) if flag == "-s"
        ));

        let args = argv(&["--kill-after"]);
        assert!(matches!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::MissingFlagValue { .. })
        ));

        // A flag value present but no duration/command after it.
        let args = argv(&["-s", "KILL"]);
        assert!(matches!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::InvalidCommand { .. })
        ));

        let args = argv(&["--"]);
        assert_eq!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::InvalidCommand {
                wrapper: "timeout",
                found: String::new(),
            })
        );
    }

    #[test]
    fn strip_timeout_unknown_option_shapes() {
        for parts in [
            &["-vp", "5", "cmd"][..],
            &["-sKILL", "5", "cmd"][..],
            &["-s=KILL", "5", "cmd"][..],
            &["--foreground=x", "5", "cmd"][..],
            &["--frobnicate", "5", "cmd"][..],
            &["-x", "5", "cmd"][..],
            &["-5", "cmd"][..],
        ] {
            let args = argv(parts);
            assert_eq!(
                strip_timeout(&args),
                Err(StripTransparentWrapperError::UnknownOption {
                    wrapper: "timeout",
                    option: parts[0].to_string(),
                }),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn strip_timeout_double_dash_separator() {
        let args = argv(&["--", "5", "cmd"]);
        assert_eq!(strip_timeout(&args).expect("strip"), argv(&["cmd"]));

        let args = argv(&["-s", "KILL", "--", "5", "cmd"]);
        assert_eq!(strip_timeout(&args).expect("strip"), argv(&["cmd"]));

        // The token after `--` is still the duration and still validated.
        let args = argv(&["--", "x", "cmd"]);
        assert_eq!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::InvalidCommand {
                wrapper: "timeout",
                found: "x".to_string(),
            })
        );
    }

    #[test]
    fn strip_timeout_double_dash_separator_should_follow_by_duration() {
        let args = argv(&["--", "x", "cmd"]);
        assert_eq!(
            strip_timeout(&args),
            Err(StripTransparentWrapperError::InvalidCommand {
                wrapper: "timeout",
                found: "x".to_string(),
            })
        );
    }

    #[test]
    fn strip_timeout_repeated_flags() {
        let args = argv(&["-s", "KILL", "-s", "TERM", "5", "cmd"]);
        assert_eq!(strip_timeout(&args).expect("strip"), argv(&["cmd"]));

        let args = argv(&["-v", "-v", "-k", "5", "-k", "10", "5", "cmd"]);
        assert_eq!(strip_timeout(&args).expect("strip"), argv(&["cmd"]));
    }

    // ------------------------------------------------------------------
    // strip_env
    // ------------------------------------------------------------------

    #[test]
    fn strip_env_bare_command() {
        let args = argv(&["make", "-j4"]);
        assert_eq!(strip_env(&args).expect("strip"), argv(&["make", "-j4"]));
    }

    #[test]
    fn strip_env_ignore_environment() {
        for flag in ["-", "-i", "--ignore-environment"] {
            let args = argv(&[flag, "make"]);
            assert_eq!(strip_env(&args).expect("strip"), argv(&["make"]), "{flag}");
        }
    }

    #[test]
    fn strip_env_debug_flags() {
        for flag in ["-v", "--debug", "--list-signal-handling"] {
            let args = argv(&[flag, "make"]);
            assert_eq!(strip_env(&args).expect("strip"), argv(&["make"]), "{flag}");
        }
    }

    #[test]
    fn strip_env_unset_short_flag() {
        for name in ["PATH", "_FOO", "A1"] {
            let args = argv(&["-u", name, "make"]);
            assert_eq!(strip_env(&args).expect("strip"), argv(&["make"]), "{name}");
        }
    }

    #[test]
    fn strip_env_unset_long_flag() {
        for name in ["PATH", "_FOO", "A1"] {
            let args = argv(&["--unset", name, "make"]);
            assert_eq!(strip_env(&args).expect("strip"), argv(&["make"]), "{name}");
        }
    }

    #[test]
    fn strip_env_unset_long_flag_inline() {
        for name in ["PATH", "_FOO", "A1"] {
            let args = argv(&[format!("--unset={name}").as_str(), "make"]);
            assert_eq!(strip_env(&args).expect("strip"), argv(&["make"]), "{name}");
        }
    }

    #[test]
    fn strip_env_signal_disposition_flags() {
        for flag in [
            "--default-signal",
            "--ignore-signal",
            "--block-signal",
            "--default-signal=INT",
            "--ignore-signal=15",
            "--block-signal=INT,TERM",
            "--default-signal=sigint",
        ] {
            let args = argv(&[flag, "make"]);
            assert_eq!(strip_env(&args).expect("strip"), argv(&["make"]), "{flag}");
        }
    }

    #[test]
    fn strip_env_signal_disposition_space_form_is_command() {
        let args = argv(&["--default-signal", "make", "-j4"]);
        assert_eq!(strip_env(&args).expect("strip"), argv(&["make", "-j4"]));

        // This is a behavior check, not accuracy check
        let args = argv(&["--ignore-signal", "INT", "make"]);
        assert_eq!(strip_env(&args).expect("strip"), argv(&["INT", "make"]));
    }

    #[test]
    fn strip_env_double_dash_separator() {
        for (parts, inner) in [
            (&["--", "make"][..], &["make"][..]),
            (&["-i", "--", "make", "-j4"][..], &["make", "-j4"][..]),
        ] {
            let args = argv(parts);
            assert_eq!(strip_env(&args).expect("strip"), argv(inner), "{parts:?}");
        }
    }

    #[test]
    fn strip_env_combined_flags() {
        for (parts, inner) in [
            (
                &["-i", "-u", "HOME", "-v", "--", "make", "-j4"][..],
                &["make", "-j4"][..],
            ),
            (
                &["--unset=HOME", "-i", "--default-signal=INT", "ls"][..],
                &["ls"][..],
            ),
            (&["-u", "A", "-u", "B", "ls"][..], &["ls"][..]),
        ] {
            let args = argv(parts);
            assert_eq!(strip_env(&args).expect("strip"), argv(inner), "{parts:?}");
        }
    }

    #[test]
    fn strip_env_missing_unset_value() {
        for flag in ["-u", "--unset"] {
            let args = argv(&[flag]);
            assert_eq!(
                strip_env(&args),
                Err(StripTransparentWrapperError::MissingFlagValue {
                    wrapper: "env",
                    flag: flag.to_string(),
                }),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_env_unset_accepts_any_name() {
        for value in ["not-a-name", "1ABC", "PATH"] {
            let args = argv(&["-u", value, "make"]);
            assert_eq!(
                strip_env(&args).expect("strip"),
                argv(&["make"]),
                "{value:?}"
            );
        }
    }

    #[test]
    fn strip_env_invalid_signal_list() {
        for flag in [
            "--default-signal=BOGUS",
            "--ignore-signal=",
            "--block-signal=INT,BOGUS",
        ] {
            let args = argv(&[flag, "make"]);
            assert!(
                matches!(
                    strip_env(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { .. })
                ),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_env_unsafe_to_strip_flags() {
        for (parts, flag) in [
            (&["-S", "echo hi", "make"][..], "-S"),
            (&["--split-string", "echo hi", "make"][..], "--split-string"),
            (&["--split-string=echo hi", "make"][..], "--split-string"),
            (&["-C", "/tmp", "make"][..], "-C"),
            (&["--chdir=/tmp", "make"][..], "--chdir"),
            (&["-a", "sh", "make"][..], "-a"),
            (&["--argv0=sh", "make"][..], "--argv0"),
            (&["-L", "user", "make"][..], "-L"),
            (&["-U", "user", "make"][..], "-U"),
            (&["-P", "/bin", "make"][..], "-P"),
        ] {
            let args = argv(parts);
            assert_eq!(
                strip_env(&args),
                Err(StripTransparentWrapperError::UnsafeToStrip {
                    wrapper: "env",
                    construct: flag.to_string(),
                }),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn strip_env_null_print_mode_is_valid() {
        // `env -0` / `env --null` with no command just prints the environment;
        // valid, with nothing to strip.
        for flag in ["-0", "--null"] {
            let args = argv(&[flag]);
            assert_eq!(strip_env(&args).expect("strip"), argv(&[]), "{flag}");
        }
        // Combined with other print-compatible options.
        let args = argv(&["-i", "-0"]);
        assert_eq!(strip_env(&args).expect("strip"), argv(&[]));
    }

    #[test]
    fn strip_env_null_with_command_is_invalid() {
        // The real tool refuses `-0`/`--null` together with a command.
        for flag in ["-0", "--null"] {
            let args = argv(&[flag, "make"]);
            assert_eq!(
                strip_env(&args),
                Err(StripTransparentWrapperError::InvalidCommand {
                    wrapper: "env",
                    found: "make".to_string(),
                }),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_env_assignment_unsafe_to_strip() {
        for (parts, assignment) in [
            (&["FOO=bar", "make"][..], "FOO=bar"),
            (&["-i", "FOO=bar", "make"][..], "FOO=bar"),
            (&["./weird=thing", "ls"][..], "./weird=thing"),
        ] {
            let args = argv(parts);
            assert_eq!(
                strip_env(&args),
                Err(StripTransparentWrapperError::UnsafeToStrip {
                    wrapper: "env",
                    construct: assignment.to_string(),
                }),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn strip_env_unknown_option() {
        // `-uHOME` and `-Sfoo` are real-tool valid, but attached short values
        // stay outside the supported subset, like timeout's `-sKILL`.
        for flag in ["-x", "--frobnicate", "-uHOME", "-Sfoo"] {
            let args = argv(&[flag, "make"]);
            assert_eq!(
                strip_env(&args),
                Err(StripTransparentWrapperError::UnknownOption {
                    wrapper: "env",
                    option: flag.to_string(),
                }),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_env_combined_dangling_value_flag() {
        let args = argv(&["-i", "-u"]);
        assert!(matches!(
            strip_env(&args),
            Err(StripTransparentWrapperError::MissingFlagValue { flag, .. }) if flag == "-u"
        ));
    }

    #[test]
    fn strip_env_combined_invalid_value() {
        let args = argv(&["-i", "--default-signal=BOGUS", "ls"]);
        assert!(matches!(
            strip_env(&args),
            Err(StripTransparentWrapperError::InvalidFlagValue { flag, value, .. })
                if flag == "--default-signal" && value == "BOGUS"
        ));
    }

    #[test]
    fn strip_env_combined_unknown_flag() {
        let args = argv(&["-i", "-x", "ls"]);
        assert!(matches!(
            strip_env(&args),
            Err(StripTransparentWrapperError::UnknownOption { option, .. }) if option == "-x"
        ));
    }

    // ------------------------------------------------------------------
    // strip_nice
    // ------------------------------------------------------------------

    #[test]
    fn strip_nice_bare_command() {
        let args = argv(&["make", "-j4"]);
        assert_eq!(strip_nice(&args).expect("strip"), argv(&["make", "-j4"]));
    }

    #[test]
    fn strip_nice_short_flag() {
        for adjustment in ["10", "-5", "+3", "0"] {
            let args = argv(&["-n", adjustment, "make"]);
            assert_eq!(
                strip_nice(&args).expect("strip"),
                argv(&["make"]),
                "{adjustment:?}"
            );
        }
    }

    #[test]
    fn strip_nice_adjustment_long_flag() {
        for adjustment in ["10", "-5", "+3", "0"] {
            let args = argv(&["--adjustment", adjustment, "make"]);
            assert_eq!(
                strip_nice(&args).expect("strip"),
                argv(&["make"]),
                "{adjustment:?}"
            );
        }
    }

    #[test]
    fn strip_nice_adjustment_long_flag_inline() {
        for adjustment in ["10", "-5", "+3", "0"] {
            let args = argv(&[format!("--adjustment={adjustment}").as_str(), "make"]);
            assert_eq!(
                strip_nice(&args).expect("strip"),
                argv(&["make"]),
                "{adjustment:?}"
            );
        }
    }

    #[test]
    fn strip_nice_double_dash_separator() {
        for (parts, inner) in [
            (&["--", "make"][..], &["make"][..]),
            (&["-n", "10", "--", "make"][..], &["make"][..]),
            (&["--adjustment", "10", "--", "make"][..], &["make"][..]),
            (&["--adjustment=5", "--", "make"][..], &["make"][..]),
        ] {
            let args = argv(parts);
            assert_eq!(strip_nice(&args).expect("strip"), argv(inner), "{parts:?}");
        }
    }

    #[test]
    fn strip_nice_combined_flags() {
        for (parts, inner) in [
            (&["-n", "5", "--adjustment", "3", "make"][..], &["make"][..]),
            (
                &["--adjustment=5", "-n", "3", "make", "-j4"][..],
                &["make", "-j4"][..],
            ),
            (&["-n", "5", "-n", "3", "--", "make"][..], &["make"][..]),
        ] {
            let args = argv(parts);
            assert_eq!(strip_nice(&args).expect("strip"), argv(inner), "{parts:?}");
        }
    }

    #[test]
    fn strip_nice_combined_invalid_value() {
        // The invalid value on the second flag is still caught.
        let args = argv(&["-n", "5", "--adjustment", "abc", "make"]);
        assert!(matches!(
            strip_nice(&args),
            Err(StripTransparentWrapperError::InvalidFlagValue { flag, value, .. })
                if flag == "--adjustment" && value == "abc"
        ));
    }

    #[test]
    fn strip_nice_combined_dangling_value_flag() {
        let args = argv(&["--adjustment=5", "-n"]);
        assert!(matches!(
            strip_nice(&args),
            Err(StripTransparentWrapperError::MissingFlagValue { flag, .. }) if flag == "-n"
        ));
    }

    #[test]
    fn strip_nice_combined_unknown_flag() {
        let args = argv(&["-n", "5", "-x", "make"]);
        assert!(matches!(
            strip_nice(&args),
            Err(StripTransparentWrapperError::UnknownOption { option, .. }) if option == "-x"
        ));
    }

    #[test]
    fn strip_nice_invalid_adjustment_short_values() {
        for value in ["abc", "1.5", "1e3", "--5", ""] {
            let args = argv(&["-n", value, "make"]);
            assert!(
                matches!(
                    strip_nice(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value: v, .. })
                        if flag == "-n" && v == value
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn strip_nice_invalid_adjustment_long_values() {
        for value in ["abc", "1.5", "1e3", "--5", ""] {
            let args = argv(&["--adjustment", value, "make"]);
            assert!(
                matches!(
                    strip_nice(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value: v, .. })
                        if flag == "--adjustment" && v == value
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn strip_nice_invalid_adjustment_long_inline() {
        for value in ["abc", "1.5", "1e3", "--5", ""] {
            let args = argv(&[format!("--adjustment={value}").as_str(), "make"]);
            assert!(
                matches!(
                    strip_nice(&args),
                    Err(StripTransparentWrapperError::InvalidFlagValue { flag, value: v, .. })
                        if flag == "--adjustment" && v == value
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn strip_nice_missing_value() {
        for flag in ["-n", "--adjustment"] {
            let args = argv(&[flag]);
            assert_eq!(
                strip_nice(&args),
                Err(StripTransparentWrapperError::MissingFlagValue {
                    wrapper: "nice",
                    flag: flag.to_string(),
                }),
                "{flag}"
            );
        }
    }

    #[test]
    fn strip_nice_unknown_option() {
        for flag in ["-10", "-x", "--frobnicate"] {
            let args = argv(&[flag, "make"]);
            assert_eq!(
                strip_nice(&args),
                Err(StripTransparentWrapperError::UnknownOption {
                    wrapper: "nice",
                    option: flag.to_string(),
                }),
                "{flag}"
            );
        }
    }

    // ------------------------------------------------------------------
    // strip_nohup
    // ------------------------------------------------------------------

    #[test]
    fn strip_nohup_bare_command() {
        let args = argv(&["make", "-j4"]);
        assert_eq!(strip_nohup(&args).expect("strip"), argv(&["make", "-j4"]));
    }

    #[test]
    fn strip_nohup_double_dash_separator() {
        let args = argv(&["--", "make", "-j4"]);
        assert_eq!(strip_nohup(&args).expect("strip"), argv(&["make", "-j4"]));
    }

    #[test]
    fn strip_nohup_unknown_option() {
        let args = argv(&["-x", "make"]);
        assert_eq!(
            strip_nohup(&args),
            Err(StripTransparentWrapperError::UnknownOption {
                wrapper: "nohup",
                option: "-x".to_string(),
            })
        );
    }

    // ------------------------------------------------------------------
    // strip_transparent_error (strip loop & other wrappers)
    // ------------------------------------------------------------------

    #[test]
    fn test_strip_timeout() {
        striped(
            &[
                "timeout",
                "75",
                "aria2c",
                "--dir=/tmp",
                "magnet:?xt=urn:btih:123456",
            ],
            &["aria2c", "--dir=/tmp", "magnet:?xt=urn:btih:123456"],
        );
        striped(&["timeout", "75", "sudo", "a", "b"], &["sudo", "a", "b"]);
    }

    #[test]
    fn strips_timeout_with_validated_flags() {
        striped(
            &["timeout", "-s", "KILL", "-k", "10", "75", "cargo", "build"],
            &["cargo", "build"],
        );
        striped(
            &["timeout", "--signal=TERM", "--foreground", "30", "ls"],
            &["ls"],
        );
        striped(
            &[
                "timeout",
                "-f",
                "-p",
                "-v",
                "--preserve-status",
                "--verbose",
                "5",
                "ls",
            ],
            &["ls"],
        );
        striped(&["timeout", "-s", "9", "5", "ls"], &["ls"]);
        striped(&["timeout", "-s", "sigkill", "5", "ls"], &["ls"]);
        striped(&["timeout", "1.5h", "make"], &["make"]);
    }

    #[test]
    fn timeout_flags_after_duration_belong_to_inner_command() {
        striped(
            &["timeout", "5", "cmd", "-s", "KILL"],
            &["cmd", "-s", "KILL"],
        );
    }

    #[test]
    fn timeout_invalid_shapes_fail_closed() {
        assert!(matches!(
            strip_err(&["timeout", "-s", "cargo", "test"]),
            StripTransparentWrapperError::InvalidFlagValue { .. }
        ));
        assert!(matches!(
            strip_err(&["timeout", "-s"]),
            StripTransparentWrapperError::MissingFlagValue { .. }
        ));
        assert!(matches!(
            strip_err(&["timeout", "5x", "ls"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
        assert!(matches!(
            strip_err(&["timeout", "--frobnicate", "5", "ls"]),
            StripTransparentWrapperError::UnknownOption { .. }
        ));
        assert!(matches!(
            strip_err(&["timeout", "5"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
    }

    #[test]
    fn strip_rejects_flag_looking_inner_command() {
        for parts in [
            &["timeout", "-v", "5", "-f", "ls"][..],
            &["timeout", "5", "--verbose", "ls"][..],
            &["nice", "-n", "5", "--", "-f", "ls"][..],
        ] {
            assert!(
                matches!(
                    strip_err(parts),
                    StripTransparentWrapperError::InvalidCommand { .. }
                ),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn help_and_version_are_not_wrapper_invocations() {
        for parts in [
            &["timeout", "--help"][..],
            &["timeout", "--version"][..],
            &["env", "--help"][..],
            &["nice", "--version"][..],
            &["nohup", "--help"][..],
        ] {
            striped(parts, parts);
        }
    }

    #[test]
    fn non_wrapper_passes_through_unchanged() {
        striped(
            &["cargo", "build", "--release"],
            &["cargo", "build", "--release"],
        );
    }

    #[test]
    fn strips_env() {
        striped(&["env", "-i", "make", "-j4"], &["make", "-j4"]);
        striped(&["env", "-u", "DISPLAY", "ls"], &["ls"]);
        striped(&["env", "-", "ls"], &["ls"]);
        striped(&["env", "--unset=DISPLAY", "--", "ls"], &["ls"]);
    }

    #[test]
    fn print_mode_invocations_are_not_wrapper_invocations() {
        // env/nice with no command print their state and exit; the argv is
        // left intact to classify on its own.
        for parts in [
            &["env"][..],
            &["env", "-0"][..],
            &["env", "-i", "--null"][..],
            &["nice"][..],
        ] {
            striped(parts, parts);
        }
    }

    #[test]
    fn env_null_with_command_fails_through_strip() {
        assert!(matches!(
            strip_err(&["env", "-0", "make"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
    }

    #[test]
    fn inner_command_flags_are_not_interpreted() {
        // `-0` is print-mode only as an env option; once it belongs to the
        // inner command it is left untouched, not re-interpreted by env/timeout.
        striped(&["env", "echo", "-0"], &["echo", "-0"]);
        striped(&["timeout", "5", "echo", "-0"], &["echo", "-0"]);
        striped(&["env", "-i", "echo", "--null"], &["echo", "--null"]);
    }

    #[test]
    fn env_assignment_fails_closed_through_strip() {
        // Assignments can change inner-command behavior (LD_PRELOAD, PATH);
        // never judge the inner command as if it ran clean.
        assert!(matches!(
            strip_err(&["env", "LD_PRELOAD=/evil.so", "cat", "x"]),
            StripTransparentWrapperError::UnsafeToStrip { .. }
        ));
    }

    #[test]
    fn strips_nice() {
        striped(&["nice", "-n", "10", "make"], &["make"]);
        striped(&["nice", "-n", "-5", "make"], &["make"]);
        striped(&["nice", "--adjustment", "10", "make"], &["make"]);
        striped(&["nice", "--adjustment=-5", "make"], &["make"]);
        striped(&["nice", "make"], &["make"]);

        assert!(matches!(
            strip_err(&["nice", "-10", "make"]),
            StripTransparentWrapperError::UnknownOption { .. }
        ));
        assert!(matches!(
            strip_err(&["nice", "-n", "abc", "make"]),
            StripTransparentWrapperError::InvalidFlagValue { .. }
        ));
    }

    #[test]
    fn strips_nohup() {
        striped(&["nohup", "make"], &["make"]);
        striped(&["nohup", "--", "make"], &["make"]);

        assert!(matches!(
            strip_err(&["nohup", "-x", "make"]),
            StripTransparentWrapperError::UnknownOption { .. }
        ));
        assert!(matches!(
            strip_err(&["nohup"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
        assert!(matches!(
            strip_err(&["nohup", "--"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
        // After `--`, a flag-looking token is the (invalid) command.
        assert!(matches!(
            strip_err(&["nohup", "--", "-x"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
    }

    #[test]
    fn strips_all_four_wrappers_nested() {
        striped(
            &[
                "timeout", "75", "env", "-i", "nice", "-n", "10", "nohup", "make",
            ],
            &["make"],
        );
    }

    #[test]
    fn strips_nested_independent_of_order() {
        striped(&["nice", "-n", "10", "timeout", "30", "make"], &["make"]);
    }

    #[test]
    fn strips_nested_without_timeout() {
        striped(&["env", "-i", "nohup", "nice", "make"], &["make"]);
    }

    #[test]
    fn strips_nested_preserving_inner_arguments() {
        striped(
            &[
                "timeout", "5", "nice", "-n", "5", "env", "-u", "HOME", "grep", "-r", "x", ".",
            ],
            &["grep", "-r", "x", "."],
        );
    }

    #[test]
    fn strips_nested_with_double_dash_separator() {
        striped(
            &["nice", "-n", "1", "--", "timeout", "5", "make"],
            &["make"],
        );
        striped(&["env", "-i", "--", "nohup", "make"], &["make"]);
    }

    #[test]
    fn strips_nested_surfacing_dangerous_inner_command() {
        // Nesting strips down to a non-wrapper that is itself dangerous; it
        // surfaces for the safety check rather than being hidden.
        striped(
            &["timeout", "5", "env", "-i", "sudo", "rm", "-rf", "x"],
            &["sudo", "rm", "-rf", "x"],
        );
    }

    #[test]
    fn nested_propagates_unsafe_to_strip() {
        assert!(matches!(
            strip_err(&["timeout", "5", "env", "FOO=bar", "make"]),
            StripTransparentWrapperError::UnsafeToStrip { .. }
        ));
    }

    #[test]
    fn nested_propagates_invalid_command() {
        assert!(matches!(
            strip_err(&["nice", "-n", "10", "timeout", "abc", "make"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
    }

    #[test]
    fn nested_propagates_invalid_flag_value() {
        assert!(matches!(
            strip_err(&["timeout", "5", "nice", "-n", "xyz", "make"]),
            StripTransparentWrapperError::InvalidFlagValue { .. }
        ));
    }

    #[test]
    fn nested_propagates_missing_flag_value() {
        assert!(matches!(
            strip_err(&["nohup", "timeout", "5", "env", "-u"]),
            StripTransparentWrapperError::MissingFlagValue { .. }
        ));
    }

    #[test]
    fn nested_propagates_missing_command() {
        assert!(matches!(
            strip_err(&["timeout", "5", "nohup"]),
            StripTransparentWrapperError::InvalidCommand { .. }
        ));
    }

    #[test]
    fn nesting_beyond_depth_limit_fails_closed() {
        let parts: Vec<&str> = std::iter::repeat_n("nohup", MAX_DEPTH + 1)
            .chain(["ls"])
            .collect();
        assert_eq!(
            strip_err(&parts),
            StripTransparentWrapperError::DepthExceeded
        );
    }

    #[test]
    fn path_qualified_wrappers_are_not_striped() {
        // `/usr/bin/timeout` or `./timeout` may be a different program;
        // leave the argv intact so it classifies conservatively.
        striped(
            &["/usr/bin/timeout", "5", "ls"],
            &["/usr/bin/timeout", "5", "ls"],
        );
        striped(&["./timeout", "5", "ls"], &["./timeout", "5", "ls"]);
    }
}
