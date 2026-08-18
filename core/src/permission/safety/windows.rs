use std::{borrow::Cow, collections::HashSet, path::Path, sync::LazyLock};

use crate::permission::{
    ArgvDecision, PermissionError, Result, safety::StripTransparentWrapperError,
};

static NOT_EXECUTABLE: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "runas", "sudo", "gsudo", "timeout", "choice", "ssh-add", "mshta",
        // Unsupported by decision: PowerShell covers everything cmd does, and
        // a `cmd /c` string cannot be decomposed for the user to judge.
        "cmd",
        // Unsupported by decision: unix shells on Windows (Git Bash, WSL's
        // System32 bash.exe). Limit the scope.
        "bash",
        // Unsupported by decision: executes inside the WSL VM where argv is
        // opaque to classification but /mnt/<drive> reaches every file.
        "wsl",
    ])
});

static ASK_NO_PERSIST: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        // nested shells
        "powershell",
        "pwsh",
        // launchers
        "start-process",
        "saps",
        "start",
        "invoke-item",
        "ii",
        // eval / code loading
        "invoke-expression",
        "iex",
        "invoke-command",
        "icm",
        "start-job",
        "sajb",
        "add-type",
        "set-executionpolicy",
        // GUI / URL handlers
        "explorer",
        "chrome",
        "msedge",
        "firefox",
        "iexplore",
        // destructive system operations
        "format-volume",
        "clear-disk",
        "diskpart",
        "shutdown",
        "stop-computer",
        "restart-computer",
        "set-date",
        "schtasks",
        "register-scheduledtask",
        "stop-process",
        "spps",
        "kill",
        "taskkill",
    ])
});

static ALWAYS_ALLOWED: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "echo",
        "write-output",
        "write-host",
        "dir",
        "ls",
        "gci",
        "get-childitem",
        "gi",
        "get-item",
        "cat",
        "type",
        "gc",
        "get-content",
        "select-string",
        "sls",
        "findstr",
        "measure-object",
        "measure",
        "get-location",
        "gl",
        "pwd",
        "cd",
        "set-location",
        "test-path",
        "tp",
        "resolve-path",
        "rvpa",
        "select-object",
        "select",
        "sort-object",
        "group-object",
        "compare-object",
        "format-list",
        "fl",
        "format-table",
        "ft",
        "format-wide",
        "fw",
        "out-string",
        "convertto-json",
        "convertfrom-json",
        "get-date",
        "start-sleep",
        "sleep",
        "get-process",
        "gps",
        "get-service",
        "gsv",
        "get-command",
        "gcm",
        "get-alias",
        "gal",
        "get-psdrive",
        "get-host",
        "whoami",
        "hostname",
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

    // Elevation may hide behind another word; scan every position.
    if command.iter().any(|a| {
        matches!(
            executable_name_lookup_key(a).as_deref(),
            Some("runas" | "sudo" | "gsudo")
        )
    }) {
        return Ok(ArgvDecision::NotExecutable);
    }

    if command.iter().any(|a| {
        let a = a.to_ascii_lowercase();
        a.contains("shellexecute") || a.contains("shell.application")
    }) {
        return Ok(ArgvDecision::AskNoPersist);
    }

    match executable_name_lookup_key(cmd0).as_deref() {
        Some(name) if NOT_EXECUTABLE.contains(name) => Ok(ArgvDecision::NotExecutable),

        // Legacy ShellExecute trampoline (`url.dll,FileProtocolHandler <url>`).
        Some("rundll32")
            if command.iter().skip(1).any(|a| {
                a.to_ascii_lowercase()
                    .starts_with("url.dll,fileprotocolhandler")
            }) =>
        {
            Ok(ArgvDecision::NotExecutable)
        },

        // `-Verb` cannot legally abbreviate: every shorter prefix is ambiguous
        // against `-Verbose`, so exact matching is sound.
        Some("start-process" | "saps" | "start")
            if command.windows(2).any(|pair| {
                (pair[0].eq_ignore_ascii_case("-verb") || pair[0].eq_ignore_ascii_case("-verb:"))
                    && pair[1].eq_ignore_ascii_case("runas")
            }) || command
                .iter()
                .any(|a| a.eq_ignore_ascii_case("-verb:runas")) =>
        {
            Ok(ArgvDecision::NotExecutable)
        },

        Some(name) if ASK_NO_PERSIST.contains(name) => Ok(ArgvDecision::AskNoPersist),

        // should always favor move to trash folder to allow recovery
        Some("remove-item" | "ri" | "rm" | "del" | "erase" | "rd" | "rmdir") => {
            Ok(ArgvDecision::NotExecutable)
        },

        Some("reg")
            if command.get(1).is_some_and(|sub| {
                [
                    "add", "delete", "import", "copy", "restore", "load", "unload",
                ]
                .iter()
                .any(|s| sub.eq_ignore_ascii_case(s))
            }) =>
        {
            Ok(ArgvDecision::AskNoPersist)
        },

        Some("rg") => Ok(super::ripgrep_check(command)),

        Some(name) if ALWAYS_ALLOWED.contains(name) => Ok(ArgvDecision::Allow),
        _ => Ok(ArgvDecision::Unknown),
    }
}

/// PowerShell has no transparent-wrapper
pub(crate) fn strip_transparent_command(
    argv: &[String],
) -> std::result::Result<&[String], StripTransparentWrapperError> {
    Ok(argv)
}

fn executable_name_lookup_key(raw: &str) -> Option<Cow<'_, str>> {
    Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            // Win32 ignores trailing dots and spaces when resolving
            // filenames, so `cmd.exe.` still launches cmd.
            let name = name.trim_end_matches(['.', ' ']).to_ascii_lowercase();
            for suffix in [".exe", ".cmd", ".bat", ".com"] {
                if let Some(stripped) = name.strip_suffix(suffix) {
                    return Cow::Owned(stripped.to_string());
                }
            }
            Cow::Owned(name)
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
    fn always_allowed() {
        for cmd in ALWAYS_ALLOWED.iter() {
            assert!(is_allow(&[cmd]), "expected {cmd} to be Allow");
        }
        assert!(is_allow(&["Get-ChildItem", "-Recurse", "C:\\src"]));
        assert!(is_allow(&["Select-String", "error", "*.log"]));
    }

    #[test]
    fn names_resolve_case_insensitively_with_path_and_extension() {
        assert!(is_allow(&["GET-CHILDITEM"]));
        assert!(is_allow(&["findstr.exe", "error", "x.log"]));
        assert!(is_allow(&["C:\\Windows\\System32\\FINDSTR.EXE", "error"]));
    }

    #[test]
    fn not_executable_names() {
        for cmd in NOT_EXECUTABLE.iter() {
            assert!(
                is_not_executable(&[cmd]),
                "expected {cmd} to be NotExecutable"
            );
        }
        assert!(is_not_executable(&["timeout.exe", "/t", "5"]));
        assert!(is_not_executable(&["CMD", "/d", "/c", "del", "x"]));
        assert!(is_not_executable(&["C:\\Windows\\System32\\choice.exe"]));
    }

    #[test]
    fn taskkill_is_ask_no_persist() {
        assert!(is_ask_no_persist(&[
            "taskkill",
            "/F",
            "/IM",
            "chrome.exe",
            "/T"
        ]));
        assert!(is_ask_no_persist(&["TASKKILL.EXE", "/PID", "1234"]));
        assert!(is_ask_no_persist(&[
            "C:\\Windows\\System32\\taskkill.exe",
            "/IM",
            "x.exe"
        ]));
    }

    #[test]
    fn unix_shells_are_not_executable() {
        assert!(is_not_executable(&["bash", "-c", "ls"]));
        assert!(is_not_executable(&["bash", "script.sh"]));
        assert!(is_not_executable(&["BASH.EXE", "-c", "ls"]));
        assert!(is_not_executable(&[
            "C:\\Program Files\\Git\\bin\\bash.exe",
            "-lc",
            "ls"
        ]));
        assert!(is_not_executable(&["wsl", "-e", "bash"]));
        assert!(is_not_executable(&["wsl.exe", "--", "rm", "-rf", "/"]));
        assert!(is_not_executable(&["C:\\Windows\\System32\\wsl.exe", "ls"]));
    }

    #[test]
    fn elevation_anywhere_is_hit() {
        assert!(is_not_executable(&["foo", "runas", "x"]));
        assert!(is_not_executable(&[
            "foo",
            "C:\\Windows\\System32\\runas.exe"
        ]));
        assert!(is_not_executable(&["foo", "gsudo.exe", "ls"]));
    }

    #[test]
    fn trailing_dots_and_spaces_are_normalized() {
        // Win32 ignores trailing dots/spaces when resolving filenames, so
        // `cmd.exe.` really launches cmd
        assert!(is_not_executable(&["cmd.exe."]));
        assert!(is_not_executable(&["cmd.exe.."]));
        assert!(is_not_executable(&["cmd."]));
        assert!(is_not_executable(&["C:\\Windows\\System32\\cmd.exe."]));
        assert!(is_ask_no_persist(&["powershell.exe.", "-Command", "ls"]));
        assert!(is_not_executable(&["foo", "runas.exe."]));
    }

    #[test]
    fn rundll32_trampoline() {
        assert!(is_not_executable(&[
            "rundll32",
            "url.dll,FileProtocolHandler",
            "https://evil.example"
        ]));
        assert!(is_not_executable(&[
            "rundll32.exe",
            "URL.DLL,FileProtocolHandler",
            "https://evil.example"
        ]));
        assert!(is_unknown(&["rundll32", "printui.dll,PrintUIEntry"]));
    }

    #[test]
    fn start_process_runas_verb() {
        assert!(is_not_executable(&[
            "start-process",
            "cmd",
            "-Verb",
            "RunAs"
        ]));
        assert!(is_not_executable(&["Start-Process", "x", "-VERB", "runas"]));
        assert!(is_not_executable(&["saps", "x", "-verb", "RunAs"]));
        // Without the elevation verb it is a plain launcher: ask, never persist.
        assert!(is_ask_no_persist(&["start-process", "notepad"]));
        assert!(is_ask_no_persist(&["start-process", "x", "-Verb", "open"]));
    }

    #[test]
    fn start_process_fused_runas_verb() {
        assert!(is_not_executable(&["start-process", "cmd", "-Verb:RunAs"]));
        assert!(is_not_executable(&["Start-Process", "x", "-VERB:runas"]));
        assert!(is_not_executable(&["saps", "x", "-verb:RunAs"]));
        assert!(is_not_executable(&["start", "x", "-Verb:runas"]));
        assert!(is_not_executable(&[
            "start-process",
            "x",
            "-Verb:",
            "RunAs"
        ]));
        assert!(is_ask_no_persist(&["start-process", "x", "-Verb:open"]));
    }

    #[test]
    fn ask_no_persist_names() {
        for cmd in ASK_NO_PERSIST.iter() {
            assert!(
                is_ask_no_persist(&[cmd]),
                "expected {cmd} to be AskNoPersist"
            );
        }
        assert!(is_ask_no_persist(&[
            "powershell",
            "-EncodedCommand",
            "ZwBpAHQA"
        ]));
        assert!(is_ask_no_persist(&["iex", "'ls'"]));
    }

    #[test]
    fn shellexecute_strings_anywhere() {
        assert!(is_ask_no_persist(&["foo", "Shell.Application"]));
        assert!(is_ask_no_persist(&["foo", "bar", "ShellExecute"]));
        assert!(is_ask_no_persist(&["foo", "shell.application,x"]));
    }

    #[test]
    fn delete_commands_are_refused_outright() {
        assert!(is_not_executable(&["remove-item", "C:\\x"]));
        assert!(is_not_executable(&["Remove-Item", "-Recurse", "C:\\x"]));
        assert!(is_not_executable(&["ri", "-rec", "C:\\x"]));
        assert!(is_not_executable(&["rm", "-Force", "C:\\x"]));
        assert!(is_not_executable(&["rm", "-Confirm", "C:\\x"]));
        assert!(is_not_executable(&["del", "file.txt"]));
        assert!(is_not_executable(&["DEL.EXE", "C:\\x"]));
        assert!(is_not_executable(&["rd", "-RECURSE", "C:\\x"]));
        assert!(is_not_executable(&["rmdir", "-f", "C:\\x"]));
        assert!(is_not_executable(&["erase", "-force", "C:\\x"]));
    }

    #[test]
    fn reg_mutating_subcommands() {
        assert!(is_ask_no_persist(&["reg", "add", "HKCU\\Software\\X"]));
        assert!(is_ask_no_persist(&["reg", "DELETE", "HKCU\\Software\\X"]));
        assert!(is_ask_no_persist(&["reg.exe", "import", "x.reg"]));
        assert!(is_unknown(&["reg", "query", "HKCU\\Software\\X"]));
    }

    #[test]
    fn rg_rejects_external_command_flags() {
        assert!(is_allow(&["rg", "-n", "needle"]));
        assert!(is_ask_no_persist(&["rg", "--pre", "pwned", "x"]));
        assert!(is_ask_no_persist(&["rg", "--pre=pwned", "x"]));
        assert!(is_ask_no_persist(&["rg", "--hostname-bin", "C:\\x", "pat"]));
        assert!(is_unknown(&["rg", "-z", "x"]));
        assert!(is_unknown(&["rg", "--search-zip", "x"]));
    }

    #[test]
    fn unknown_command_falls_through() {
        assert!(is_unknown(&["git", "status"]));
        assert!(is_unknown(&["cargo", "build"]));
        assert!(is_unknown(&["some-random.exe"]));
    }
}
