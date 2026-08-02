//! Per-argv safety check. Logic partially ported from openai/codex
//! (`codex-rs/shell-command/src/command_safety/is_safe_command.rs`).

mod transparent;

use std::{borrow::Cow, collections::HashSet, path::Path, sync::LazyLock};

pub(crate) use transparent::strip_transparent_command;

use crate::permission::{ArgvDecision, PermissionError, Result, constants::SHELLS};

// TODO: auto-approved commands run with full user privileges and no prompt,
//  so `cat ~/.ssh/id_rsa` streams secrets into the LLM request without consent.
//  This list is only safe once the shell plugin gets filesystem isolation.
static ALWAYS_ALLOWED: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "cat", "cd", "cut", "echo", "expr", "false", "grep", "head", "id", "ls", "nl", "paste",
        "pwd", "rev", "seq", "stat", "tail", "tr", "true", "uname", "wc", "which", "whoami",
    ])
});

pub(crate) fn safety_check(command: &[String]) -> Result<ArgvDecision> {
    let Some(cmd0) = command
        .first()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Err(PermissionError::EmptyCommand);
    };

    if command
        .iter()
        .any(|a| executable_name_lookup_key(a).as_deref() == Some("sudo"))
    {
        return Ok(ArgvDecision::NotExecutable);
    }

    match executable_name_lookup_key(cmd0).as_deref() {
        // Require a controlling tty; cannot be driven non-interactively.
        Some("su" | "passwd" | "ssh-add") => Ok(ArgvDecision::NotExecutable),

        // Interactive key-generation prompts; non-`--gen-key` invocations
        // (e.g. `gpg --list-keys`) fall through to the catch-all below.
        Some("gpg")
            if command
                .iter()
                .any(|a| matches!(a.as_str(), "--gen-key" | "--full-generate-key")) =>
        {
            Ok(ArgvDecision::NotExecutable)
        },

        Some("pkexec" | "osascript") => Ok(ArgvDecision::AskNoPersist),

        Some("rm")
            if has_recursive_short_flag(command) || command.iter().any(|a| a == "--recursive") =>
        {
            Ok(ArgvDecision::AskNoPersist)
        },

        Some("chmod" | "chown")
            if has_recursive_short_flag(command) || command.iter().any(|a| a == "--recursive") =>
        {
            Ok(ArgvDecision::AskNoPersist)
        },

        Some("kill") if command.iter().any(|a| a == "-1") => Ok(ArgvDecision::AskNoPersist),

        Some("dd") if command.iter().any(|a| a.starts_with("of=")) => {
            Ok(ArgvDecision::AskNoPersist)
        },

        Some(name) if name == "mkfs" || name.starts_with("mkfs.") => Ok(ArgvDecision::AskNoPersist),

        Some(name) if SHELLS.contains(name) => Ok(ArgvDecision::AskNoPersist),

        Some(cmd) if cfg!(target_os = "linux") && matches!(cmd, "numfmt" | "tac") => {
            Ok(ArgvDecision::Allow)
        },

        Some(name) if ALWAYS_ALLOWED.contains(name) => Ok(ArgvDecision::Allow),

        Some("date") if is_safe_date_argv(command) => Ok(ArgvDecision::Allow),

        Some("base64") => {
            const UNSAFE_BASE64_OPTIONS: &[&str] = &["-o", "--output"];

            if command.iter().skip(1).any(|arg| {
                UNSAFE_BASE64_OPTIONS.contains(&arg.as_str())
                    || arg.starts_with("--output=")
                    || (arg.starts_with("-o") && arg != "-o")
            }) {
                Ok(ArgvDecision::Unknown)
            } else {
                Ok(ArgvDecision::Allow)
            }
        },

        Some("uniq") => {
            if uniq_has_output_operand(command) {
                Ok(ArgvDecision::Unknown)
            } else {
                Ok(ArgvDecision::Allow)
            }
        },

        Some("find") => {
            // Options that can execute arbitrary commands or deletes matching files
            const DANGEROUS_FIND_OPTIONS: &[&str] =
                &["-exec", "-execdir", "-ok", "-okdir", "-delete"];
            // Options that write pathname to a file.
            const WRITES_TO_FILE_FIND_OPTIONS: &[&str] =
                &["-fls", "-fprint", "-fprint0", "-fprintf"];

            if command
                .iter()
                .any(|a| DANGEROUS_FIND_OPTIONS.contains(&a.as_str()))
            {
                Ok(ArgvDecision::AskNoPersist)
            } else if command
                .iter()
                .any(|a| WRITES_TO_FILE_FIND_OPTIONS.contains(&a.as_str()))
            {
                Ok(ArgvDecision::Unknown)
            } else {
                Ok(ArgvDecision::Allow)
            }
        },

        // Ripgrep
        Some("rg") => Ok(super::ripgrep_check(command)),

        // `sed -n {N|M,N}p <file>` read-only shape.
        Some("sed")
            if command.len() <= 4
                && command.get(1).map(String::as_str) == Some("-n")
                && is_valid_sed_n_arg(command.get(2).map(String::as_str)) =>
        {
            Ok(ArgvDecision::Allow)
        },

        _ => Ok(ArgvDecision::Unknown),
    }
}

fn uniq_has_output_operand(command: &[String]) -> bool {
    const VALUE_OPTS: &[&str] = &[
        "-f",
        "-s",
        "-w",
        "--skip-fields",
        "--skip-chars",
        "--check-chars",
    ];
    let mut operands = 0usize;
    let mut args = command.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            operands += args.count();
            break;
        }
        if arg.len() > 1 && arg.starts_with('-') {
            if VALUE_OPTS.contains(&arg.as_str()) {
                args.next();
            }
            continue;
        }
        operands += 1;
    }
    operands >= 2
}

fn has_recursive_short_flag(argv: &[String]) -> bool {
    argv.iter().any(|a| {
        if !a.starts_with('-') || a.starts_with("--") || a.len() < 2 {
            return false;
        }
        a[1..].chars().any(|c| c == 'r' || c == 'R')
    })
}

fn executable_name_lookup_key(raw: &str) -> Option<Cow<'_, str>> {
    Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .map(Cow::Borrowed)
}

/// Matches `^(\d+,)?\d+p$` — the conservative read-only shape for `sed -n`.
fn is_valid_sed_n_arg(arg: Option<&str>) -> bool {
    let Some(s) = arg else { return false };
    let Some(core) = s.strip_suffix('p') else {
        return false;
    };
    let parts: Vec<&str> = core.split(',').collect();
    match parts.as_slice() {
        // single number, e.g. "10"
        [num] => !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()),

        // two numbers, e.g. "1,5"
        [a, b] => {
            !a.is_empty()
                && !b.is_empty()
                && a.chars().all(|c| c.is_ascii_digit())
                && b.chars().all(|c| c.is_ascii_digit())
        },

        // anything else (more than one comma) is invalid
        _ => false,
    }
}

/// only allow date read options
fn is_safe_date_argv(command: &[String]) -> bool {
    command.iter().skip(1).all(|arg| {
        arg.starts_with('+')
            || matches!(
                arg.as_str(),
                "-u" | "--utc" | "--universal" | "-R" | "--rfc-email" | "-I" | "--iso-8601"
            )
            || matches!(
                arg.strip_prefix("-I"),
                Some("date" | "hours" | "minutes" | "seconds" | "ns")
            )
            || arg.starts_with("--iso-8601=")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn is_allow(args: &[&str]) -> bool {
        matches!(safety_check(&argv(args)), Ok(ArgvDecision::Allow))
    }

    fn is_unknown(args: &[&str]) -> bool {
        matches!(safety_check(&argv(args)), Ok(ArgvDecision::Unknown))
    }

    fn is_ask_no_persist(args: &[&str]) -> bool {
        matches!(safety_check(&argv(args)), Ok(ArgvDecision::AskNoPersist))
    }

    fn is_not_executable(args: &[&str]) -> bool {
        matches!(safety_check(&argv(args)), Ok(ArgvDecision::NotExecutable))
    }

    #[test]
    fn date_read_only_shapes() {
        assert!(is_allow(&["date"]));
        assert!(is_allow(&["date", "+%Y-%m-%d"]));
        assert!(is_allow(&["date", "-u", "+%FT%TZ"]));
        assert!(is_allow(&["date", "--iso-8601"]));
        assert!(is_allow(&["date", "--iso-8601=seconds"]));
        assert!(is_allow(&["date", "-I"]));
        assert!(is_allow(&["date", "-Iseconds"]));

        assert!(is_unknown(&["date", "-s", "tomorrow"]));
        assert!(is_unknown(&["date", "--set=tomorrow"]));
        assert!(is_unknown(&["date", "060412002026"]));
        assert!(is_unknown(&["date", "-Invalid"]));
        assert!(is_unknown(&["date", "--rfc-3339=ns"]));
    }

    #[test]
    fn always_allowed() {
        for cmd in ALWAYS_ALLOWED.iter() {
            assert!(is_allow(&[cmd]), "expected {cmd} to be Allow");
        }
        assert!(is_allow(&["ls", "-la", "src"]));
    }

    #[test]
    fn uniq_stdout_forms_are_allowed() {
        assert!(is_allow(&["uniq"]));
        assert!(is_allow(&["uniq", "input.txt"]));
        assert!(is_allow(&["uniq", "-c", "input.txt"]));
        assert!(is_allow(&["uniq", "-f", "2", "input.txt"]));
        assert!(is_allow(&["uniq", "--skip-fields", "2", "input.txt"]));
        assert!(is_allow(&["uniq", "--skip-fields", "2", "-c", "input.txt"]));
        assert!(is_allow(&[
            "uniq",
            "--skip-fields",
            "2",
            "--count",
            "input.txt"
        ]));
        assert!(is_allow(&["uniq", "-i", "-"]));
    }

    #[test]
    fn uniq_with_output_operand_requires_consent() {
        assert!(is_unknown(&["uniq", "input.txt", "output.txt"]));
        assert!(is_unknown(&["uniq", "-i", "input.txt", "output.txt"]));
        assert!(is_unknown(&["uniq", "-f", "2", "input.txt", "output.txt"]));
        assert!(is_unknown(&["uniq", "--", "input.txt", "output.txt"]));
    }

    #[test]
    fn uniq_obsolete_forms_match_gnu_coreutils() {
        assert!(is_allow(&["uniq", "-5", "input.txt"]));
        assert!(is_unknown(&["uniq", "-5", "input.txt", "output.txt"]));
        assert!(is_allow(&["uniq", "+5"]));
        assert!(is_unknown(&["uniq", "+5", "input.txt"]));
    }

    #[test]
    fn empty_argv_is_error() {
        assert!(matches!(
            safety_check(&argv(&[])),
            Err(PermissionError::EmptyCommand),
        ));
        assert!(matches!(
            safety_check(&argv(&[""])),
            Err(PermissionError::EmptyCommand),
        ));
    }

    #[test]
    fn unknown_command_falls_through() {
        assert!(is_unknown(&["foo"]));
        assert!(is_unknown(&["cargo", "build"]));
        assert!(is_unknown(&["./script.sh"]));
    }

    #[test]
    fn find_rejects_dangerous_options() {
        assert!(is_allow(&["find", ".", "-name", "x"]));
        assert!(is_ask_no_persist(&[
            "find", ".", "-name", "x", "-exec", "rm", "{}", ";",
        ]));
        assert!(is_ask_no_persist(&[
            "find", ".", "-execdir", "rm", "{}", ";"
        ]));
        assert!(is_ask_no_persist(&["find", ".", "-ok", "rm", "{}", ";"]));
        assert!(is_ask_no_persist(&["find", ".", "-okdir", "rm", "{}", ";"]));
        assert!(is_ask_no_persist(&["find", ".", "-delete"]));
        assert!(is_unknown(&["find", ".", "-fprint", "out.txt"]));
        assert!(is_unknown(&["find", ".", "-fls", "out.txt"]));
        assert!(is_unknown(&["find", ".", "-fprint0", "out.txt"]));
        assert!(is_unknown(&["find", ".", "-fprintf", "out.txt", "%p\\n"]));
    }

    #[test]
    fn rg_rejects_external_command_flags() {
        assert!(is_allow(&["rg", "-n", "needle"]));
        assert!(is_ask_no_persist(&["rg", "--pre", "pwned", "x"]));
        assert!(is_ask_no_persist(&["rg", "--pre=pwned", "x"]));
        assert!(is_ask_no_persist(&[
            "rg",
            "--hostname-bin",
            "/tmp/x",
            "pat",
            "."
        ]));
        assert!(is_ask_no_persist(&[
            "rg",
            "--hostname-bin=/tmp/x",
            "pat",
            "."
        ]));
        assert!(is_unknown(&["rg", "-z", "x"]));
        assert!(is_unknown(&["rg", "--search-zip", "x"]));
    }

    #[test]
    fn base64_rejects_output_options() {
        assert!(is_allow(&["base64"]));
        assert!(is_allow(&["base64", "-d"]));
        assert!(is_allow(&["base64", "input.txt"]));
        assert!(is_allow(&["base64", "-d", "input.txt"]));
        assert!(is_unknown(&["base64", "-o", "out.bin"]));
        assert!(is_unknown(&["base64", "--output", "out.bin"]));
        assert!(is_unknown(&["base64", "--output=out.bin"]));
        assert!(is_unknown(&["base64", "-oout.bin"]));
    }

    #[test]
    fn sed_read_only_shapes() {
        assert!(is_allow(&["sed", "-n", "1,5p", "f"]));
        assert!(is_allow(&["sed", "-n", "10p", "f"]));
        assert!(is_unknown(&["sed", "-n", "xp", "f"]));
        assert!(is_unknown(&["sed", "-i", "s/x/y/", "f"]));
    }

    #[test]
    fn recursive_rm_variants() {
        assert!(is_ask_no_persist(&["rm", "-r", "x"]));
        assert!(is_ask_no_persist(&["rm", "-R", "x"]));
        assert!(is_ask_no_persist(&["rm", "-rf", "x"]));
        assert!(is_ask_no_persist(&["rm", "-fr", "x"]));
        assert!(is_ask_no_persist(&["rm", "-vrf", "x"]));
        assert!(is_ask_no_persist(&["rm", "-rfv", "x"]));
        assert!(is_ask_no_persist(&["rm", "-Rfv", "x"]));
        assert!(is_ask_no_persist(&["rm", "--recursive", "x"]));
    }

    #[test]
    fn non_recursive_rm_is_not_ask_no_persist() {
        assert!(is_unknown(&["rm", "x"]));
        assert!(is_unknown(&["rm", "-i", "x"]));
    }

    #[test]
    fn recursive_chmod_chown() {
        assert!(is_ask_no_persist(&["chmod", "-R", "755", "."]));
        assert!(is_ask_no_persist(&["chmod", "-Rv", "755", "."]));
        assert!(is_ask_no_persist(&["chown", "--recursive", "u", "."]));
        assert!(is_unknown(&["chmod", "755", "."]));
    }

    #[test]
    fn dd_write_operand() {
        assert!(is_ask_no_persist(&["dd", "if=/dev/zero", "of=/tmp/out"]));
        assert!(is_unknown(&["dd", "if=/dev/zero"]));
    }

    #[test]
    fn mkfs_family() {
        assert!(is_ask_no_persist(&["mkfs", "/dev/sda1"]));
        assert!(is_ask_no_persist(&["mkfs.ext4", "/dev/sda1"]));
        assert!(is_ask_no_persist(&["/sbin/mkfs.btrfs", "/dev/sda1"]));
        assert!(is_unknown(&["mkfsfoo", "/dev/sda1"]));
    }

    #[test]
    fn kill_signal_negative_one() {
        assert!(is_ask_no_persist(&["kill", "-1"]));
        assert!(is_unknown(&["kill", "-9", "123"]));
    }

    #[test]
    fn sudo_anywhere_is_hit() {
        assert!(is_not_executable(&["sudo", "ls"]));
        assert!(is_not_executable(&["env", "FOO=1", "sudo", "ls"]));
        assert!(is_not_executable(&["/usr/bin/sudo", "ls"]));
        assert!(is_not_executable(&["env", "FOO=1", "/usr/bin/sudo", "ls"]));
    }

    #[test]
    fn sudo_outranks_other_dangerous_shapes() {
        assert!(is_not_executable(&["sudo", "rm", "-rf", "x"]));
        assert!(is_not_executable(&["sudo", "mkfs.ext4", "/dev/sda1"]));
        assert!(is_not_executable(&[
            "sudo",
            "dd",
            "if=/dev/zero",
            "of=/dev/sda"
        ]));
        assert!(is_not_executable(&["sudo", "chmod", "-R", "777", "."]));
        assert!(is_not_executable(&[
            "env", "PATH=/x", "sudo", "rm", "-rf", "."
        ]));
        assert!(is_not_executable(&["/usr/bin/sudo", "rm", "-rf", "."]));
    }

    #[test]
    fn interactive_tty_commands() {
        assert!(is_not_executable(&["su", "-"]));
        assert!(is_not_executable(&["passwd"]));
        assert!(is_not_executable(&["ssh-add"]));
        assert!(is_not_executable(&["/bin/su"]));
        assert!(is_not_executable(&["/usr/bin/passwd"]));
        assert!(is_not_executable(&["/usr/bin/ssh-add"]));
    }

    #[test]
    fn gpg_key_generation() {
        assert!(is_not_executable(&["gpg", "--gen-key"]));
        assert!(is_not_executable(&["gpg", "--full-generate-key"]));
        assert!(is_not_executable(&["/usr/bin/gpg", "--gen-key"]));
        assert!(is_unknown(&["gpg", "--list-keys"]));
    }

    #[test]
    fn benign_commands_not_flagged() {
        assert!(!is_not_executable(&["ls", "-la"]));
        assert!(!is_not_executable(&["echo", "hi"]));
    }

    #[test]
    fn shells_are_ask_no_persist() {
        for shell in SHELLS.iter() {
            assert!(
                is_ask_no_persist(&[shell]),
                "expected {shell} to be AskNoPersist"
            );
        }
        assert!(is_ask_no_persist(&["bash", "script.sh"]));
        assert!(is_ask_no_persist(&["/bin/bash", "script.sh"]));
        assert!(is_ask_no_persist(&["/usr/bin/fish", "-c", "echo hi"]));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_only_safe_utilities() {
        assert!(is_allow(&["numfmt", "--to=iec", "1234"]));
        assert!(is_allow(&["tac", "file.txt"]));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn linux_only_utilities_not_on_other_platforms() {
        assert!(is_unknown(&["numfmt", "--to=iec", "1234"]));
        assert!(is_unknown(&["tac", "file.txt"]));
    }
}
