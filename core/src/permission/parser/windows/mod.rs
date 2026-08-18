use std::{path::Path, process::Stdio, sync::LazyLock, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use log::warn;
use serde::Deserialize;
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

use crate::permission::{PermissionError, Result};

/// use PowerShell to call the c# api to parse the PowerShell commands
const PARSER_HELPER: &str = include_str!("parse_helper.ps1");

/// `-EncodedCommand` requires base64 over UTF-16LE bytes.
static ENCODED_HELPER: LazyLock<String> = LazyLock::new(|| {
    let utf16le: Vec<u8> = PARSER_HELPER
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    STANDARD.encode(utf16le)
});

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const SIDECAR_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct ParseOutcome {
    ok: bool,
    #[serde(default)]
    commands: Vec<Vec<String>>,
}

pub async fn parse_commands(command: &[String]) -> Result<Option<Vec<Vec<String>>>> {
    if command.is_empty() {
        return Err(PermissionError::EmptyCommand);
    }
    let program = Path::new(command[0].as_str())
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&command[0]);
    // skip parsing if not a PowerShell command
    if !program.eq_ignore_ascii_case("powershell") {
        return Ok(Some(vec![command.to_vec()]));
    }
    let mut script: Option<String> = None;
    let mut args = command[1..].iter();
    while let Some(arg) = args.next() {
        if arg.eq_ignore_ascii_case("-NoProfile")
            || arg.eq_ignore_ascii_case("-NonInteractive")
            || arg.eq_ignore_ascii_case("-NoLogo")
        {
            continue;
        }
        if arg.eq_ignore_ascii_case("-Command") {
            let rest: Vec<&str> = args.map(String::as_str).collect();
            if !rest.is_empty() {
                script = Some(rest.join(" "));
            }
        }
        break;
    }
    // Opaque invocation (-File, -EncodedCommand, unknown switch, bare -Command)
    let Some(script) = script else {
        return Ok(Some(vec![command.to_vec()]));
    };
    match try_parse_shell(&script).await {
        Some(commands) if commands.is_empty() => Err(PermissionError::EmptyCommand),
        other => Ok(other),
    }
}

async fn try_parse_shell(script: &str) -> Option<Vec<Vec<String>>> {
    let stdout = match parse_command(script).await {
        Ok(stdout) => stdout,
        Err(stage) => {
            warn!("PowerShell parser unavailable ({stage}); treating command as unparseable");
            return None;
        },
    };
    let outcome: ParseOutcome = match serde_json::from_str(stdout.trim()) {
        Ok(outcome) => outcome,
        Err(_) => {
            warn!("PowerShell parser emitted malformed output; treating command as unparseable");
            return None;
        },
    };
    outcome.ok.then_some(outcome.commands)
}

async fn parse_command(script: &str) -> std::result::Result<String, &'static str> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &ENCODED_HELPER,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn().map_err(|_| "spawn failed")?;
    let mut stdin = child.stdin.take().ok_or("stdin handle missing")?;
    stdin
        .write_all(script.as_bytes())
        .await
        .map_err(|_| "stdin write failed")?;
    drop(stdin);
    let output = timeout(SIDECAR_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "timed out")?
        .map_err(|_| "wait failed")?;
    String::from_utf8(output.stdout).map_err(|_| "non-utf8 stdout")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REJECT: &str = r#"{"ok":false}"#;

    async fn output(script: &str) -> String {
        parse_command(script).await.unwrap().trim().to_string()
    }

    #[tokio::test]
    async fn normal_command() {
        assert_eq!(
            output("git status").await,
            r#"{"ok":true,"commands":[["git","status"]]}"#
        );
        assert_eq!(
            output("cargo build --release").await,
            r#"{"ok":true,"commands":[["cargo","build","--release"]]}"#
        );
    }

    #[tokio::test]
    async fn single_word_command() {
        assert_eq!(output("pwd").await, r#"{"ok":true,"commands":[["pwd"]]}"#);
    }

    #[tokio::test]
    async fn powershell_command_with_parameters() {
        assert_eq!(
            output("Remove-Item -LiteralPath C:\\tmp\\x").await,
            r#"{"ok":true,"commands":[["Remove-Item","-LiteralPath","C:\\tmp\\x"]]}"#
        );
    }

    #[tokio::test]
    async fn pipeline_decomposes_per_command() {
        assert_eq!(
            output("Get-ChildItem *.log | Select-String error").await,
            r#"{"ok":true,"commands":[["Get-ChildItem","*.log"],["Select-String","error"]]}"#
        );
    }

    #[tokio::test]
    async fn semicolon_chain_decomposes_per_statement() {
        assert_eq!(
            output("git add .; git commit -m 'fix'").await,
            r#"{"ok":true,"commands":[["git","add","."],["git","commit","-m","fix"]]}"#
        );
    }

    #[tokio::test]
    async fn empty_statement_between_semicolons_is_tolerated() {
        assert_eq!(
            output("ls ;; pwd").await,
            r#"{"ok":true,"commands":[["ls"],["pwd"]]}"#
        );
        assert_eq!(output("ls;").await, r#"{"ok":true,"commands":[["ls"]]}"#);
    }

    #[tokio::test]
    async fn newline_separates_statements() {
        assert_eq!(
            output("ls\npwd").await,
            r#"{"ok":true,"commands":[["ls"],["pwd"]]}"#
        );
        assert_eq!(
            output("ls\r\npwd").await,
            r#"{"ok":true,"commands":[["ls"],["pwd"]]}"#
        );
    }

    #[tokio::test]
    async fn newline_inside_quoted_string_is_one_word() {
        assert_eq!(
            output("git commit -m \"line1\nline2\"").await,
            r#"{"ok":true,"commands":[["git","commit","-m","line1\nline2"]]}"#
        );
        assert_eq!(
            output("git commit -m \"a\r\nb\"").await,
            r#"{"ok":true,"commands":[["git","commit","-m","a\r\nb"]]}"#
        );
    }

    #[tokio::test]
    async fn comments_are_dropped() {
        assert_eq!(
            output("ls # comment").await,
            r#"{"ok":true,"commands":[["ls"]]}"#
        );
    }

    #[tokio::test]
    async fn backtick_in_bareword_normalizes_to_plain_name() {
        assert_eq!(
            output("g`it status").await,
            r#"{"ok":true,"commands":[["git","status"]]}"#
        );
    }

    #[tokio::test]
    async fn doubled_quote_escape_is_one_word() {
        assert_eq!(
            output("echo \"he said \"\"hi\"\"\"").await,
            r#"{"ok":true,"commands":[["echo","he said \"hi\""]]}"#
        );
    }

    #[tokio::test]
    async fn quoted_strings_become_single_words() {
        assert_eq!(
            output("echo \"hello world\"").await,
            r#"{"ok":true,"commands":[["echo","hello world"]]}"#
        );
        assert_eq!(
            output("type 'C:\\a b\\f.txt'").await,
            r#"{"ok":true,"commands":[["type","C:\\a b\\f.txt"]]}"#
        );
    }

    #[tokio::test]
    async fn constant_backtick_escape_is_expanded() {
        assert_eq!(
            output("echo \"a`nb\"").await,
            r#"{"ok":true,"commands":[["echo","a\nb"]]}"#
        );
    }

    #[tokio::test]
    async fn barewords_survive_unchanged() {
        assert_eq!(
            output("git log --format=%H").await,
            r#"{"ok":true,"commands":[["git","log","--format=%H"]]}"#
        );
        assert_eq!(
            output("foo.exe /S /Q").await,
            r#"{"ok":true,"commands":[["foo.exe","/S","/Q"]]}"#
        );
        assert_eq!(
            output("echo 1..3").await,
            r#"{"ok":true,"commands":[["echo","1..3"]]}"#
        );
    }

    #[tokio::test]
    async fn numbers_become_words() {
        assert_eq!(
            output("Start-Sleep 5").await,
            r#"{"ok":true,"commands":[["Start-Sleep","5"]]}"#
        );
        assert_eq!(
            output("echo 3.14").await,
            r#"{"ok":true,"commands":[["echo","3.14"]]}"#
        );
    }

    #[tokio::test]
    async fn numeric_spellings_are_preserved() {
        assert_eq!(
            output("Start-Sleep 0x10").await,
            r#"{"ok":true,"commands":[["Start-Sleep","0x10"]]}"#
        );
        assert_eq!(
            output("echo 007").await,
            r#"{"ok":true,"commands":[["echo","007"]]}"#
        );
        assert_eq!(
            output("foo 1e3").await,
            r#"{"ok":true,"commands":[["foo","1e3"]]}"#
        );
        assert_eq!(
            output("echo 10kb").await,
            r#"{"ok":true,"commands":[["echo","10kb"]]}"#
        );
    }

    #[tokio::test]
    async fn unicode_round_trips() {
        assert_eq!(
            output("echo héllo wörld").await,
            r#"{"ok":true,"commands":[["echo","héllo","wörld"]]}"#
        );
    }

    #[tokio::test]
    async fn rejects_variables_and_expansions() {
        assert_eq!(output("$x = 1").await, REJECT);
        assert_eq!(output("echo $env:USERNAME").await, REJECT);
        assert_eq!(output("echo \"hi $env:USERNAME\"").await, REJECT);
        assert_eq!(output("echo $(pwd)").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_redirections() {
        assert_eq!(output("ls > out.txt").await, REJECT);
        assert_eq!(output("ls >> log.txt").await, REJECT);
        assert_eq!(output("ls 2>&1").await, REJECT);
        assert_eq!(output("ls *> all.txt").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_dotnet_invocation() {
        assert_eq!(output("[Console]::WriteLine('x')").await, REJECT);
        assert_eq!(output("[System.IO.File]::Delete('C:\\x')").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_subexpression_in_string() {
        assert_eq!(output("echo \"x $(rm f)\"").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_collection_literals() {
        assert_eq!(output("echo @{a=1}").await, REJECT);
        assert_eq!(output("echo @(1,2)").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_expression_pipeline_head() {
        assert_eq!(output("'a' | Select-String a").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_script_blocks_and_control_flow() {
        assert_eq!(output("if ($?) { ls }").await, REJECT);
        assert_eq!(output("ls | ForEach-Object { rm $_ }").await, REJECT);
        assert_eq!(output("Write-Output hello; exit 1").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_invocation_operators() {
        assert_eq!(output("& git status").await, REJECT);
        assert_eq!(output(". C:\\profile.ps1").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_powershell7_only_operators() {
        assert_eq!(output("ls && pwd").await, REJECT);
        assert_eq!(output("ls || pwd").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_splatting_and_fused_parameters() {
        assert_eq!(output("echo @args").await, REJECT);
        assert_eq!(output("Get-ChildItem -Path:C:\\tmp").await, REJECT);
        // Load-bearing for the elevation deny: the fused spelling must never
        // tokenize into atoms, or it would bypass the pair-match in safety.
        assert_eq!(output("Start-Process cmd -Verb:RunAs").await, REJECT);
    }

    #[tokio::test]
    async fn rejects_stop_parsing_token() {
        assert_eq!(output("git --% status").await, REJECT);
        assert_eq!(
            output("echo '--%'").await,
            r#"{"ok":true,"commands":[["echo","--%"]]}"#
        );
    }

    #[tokio::test]
    async fn rejects_empty_script() {
        assert_eq!(output("").await, REJECT);
        assert_eq!(output(";").await, REJECT);
    }
}

#[cfg(test)]
mod parse_commands_tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    async fn parsed(command: &[&str]) -> Vec<Vec<String>> {
        parse_commands(&argv(command)).await.unwrap().unwrap()
    }

    async fn is_complex(command: &[&str]) -> bool {
        matches!(parse_commands(&argv(command)).await, Ok(None))
    }

    #[tokio::test]
    async fn empty_argv_is_error() {
        assert!(matches!(
            parse_commands(&[]).await,
            Err(PermissionError::EmptyCommand)
        ));
    }

    #[tokio::test]
    async fn plain_argv_passes_through() {
        assert_eq!(
            parsed(&["git", "status"]).await,
            vec![argv(&["git", "status"])]
        );
        assert_eq!(
            parsed(&["cargo", "build", "--release"]).await,
            vec![argv(&["cargo", "build", "--release"])]
        );
    }

    #[tokio::test]
    async fn plain_argv_with_metacharacters_passes_through() {
        assert_eq!(
            parsed(&["python", "-c", "print(1)"]).await,
            vec![argv(&["python", "-c", "print(1)"])]
        );
    }

    #[tokio::test]
    async fn plain_argv_with_spaced_path_keeps_token_boundaries() {
        // Splitting the head at the space would let the persist option offer
        // a `C:\Program` glob that matches everything under Program Files.
        assert_eq!(
            parsed(&["C:\\Program Files\\Git\\bin\\git.exe", "status"]).await,
            vec![argv(&["C:\\Program Files\\Git\\bin\\git.exe", "status"])]
        );
        assert_eq!(
            parsed(&["git", "add", "My Documents\\notes.txt"]).await,
            vec![argv(&["git", "add", "My Documents\\notes.txt"])]
        );
    }

    #[tokio::test]
    async fn plain_argv_with_separator_in_argument_is_not_split() {
        // A literal `;` or `|` inside one argv element is data, not a
        // statement separator; re-splitting invents atoms that were never
        // part of the command.
        assert_eq!(
            parsed(&["git", "commit", "-m", "fix; format-volume"]).await,
            vec![argv(&["git", "commit", "-m", "fix; format-volume"])]
        );
        assert_eq!(
            parsed(&["echo", "a | rm x"]).await,
            vec![argv(&["echo", "a | rm x"])]
        );
    }

    #[tokio::test]
    #[ignore = "flaky on CI/CD due to powershell code start."]
    async fn extracts_command_script() {
        assert_eq!(
            parsed(&[
                "powershell",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-ChildItem *.log | Select-String error",
            ])
            .await,
            vec![
                argv(&["Get-ChildItem", "*.log"]),
                argv(&["Select-String", "error"]),
            ]
        );
    }

    #[tokio::test]
    #[ignore = "flaky on CI/CD due to powershell code start."]
    async fn extracts_command_rest_arguments_joined() {
        assert_eq!(
            parsed(&[
                "powershell",
                "-NoProfile",
                "-Command",
                "Remove-Item",
                "-LiteralPath",
                "C:\\tmp\\x",
            ])
            .await,
            vec![argv(&["Remove-Item", "-LiteralPath", "C:\\tmp\\x"])]
        );
    }

    #[tokio::test]
    async fn complex_extracted_script_is_complex() {
        assert!(is_complex(&["powershell", "-NoProfile", "-Command", "$x = 1"]).await);
    }

    #[tokio::test]
    async fn program_and_switches_match_case_insensitively() {
        assert_eq!(
            parsed(&["POWERSHELL.EXE", "-noprofile", "-command", "git status"]).await,
            vec![argv(&["git", "status"])]
        );
        assert_eq!(
            parsed(&[
                "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                "-Command",
                "git status",
            ])
            .await,
            vec![argv(&["git", "status"])]
        );
    }

    #[tokio::test]
    async fn file_and_encoded_command_are_opaque() {
        assert_eq!(
            parsed(&["powershell", "-File", "C:\\s.ps1"]).await,
            vec![argv(&["powershell", "-File", "C:\\s.ps1"])]
        );
        assert_eq!(
            parsed(&["powershell", "-EncodedCommand", "ZwBpAHQA"]).await,
            vec![argv(&["powershell", "-EncodedCommand", "ZwBpAHQA"])]
        );
    }

    #[tokio::test]
    async fn file_before_command_is_opaque() {
        assert_eq!(
            parsed(&["powershell", "-File", "evil.ps1", "-Command", "ls"]).await,
            vec![argv(&["powershell", "-File", "evil.ps1", "-Command", "ls"])]
        );
    }

    #[tokio::test]
    async fn unknown_switch_is_opaque() {
        assert_eq!(
            parsed(&["powershell", "-ExecutionPolicy", "Bypass", "-Command", "ls"]).await,
            vec![argv(&[
                "powershell",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "ls"
            ])]
        );
    }

    #[tokio::test]
    async fn abbreviated_switch_is_opaque() {
        assert_eq!(
            parsed(&["powershell", "-nop", "-Command", "ls"]).await,
            vec![argv(&["powershell", "-nop", "-Command", "ls"])]
        );
    }

    #[tokio::test]
    async fn command_without_script_is_opaque() {
        assert_eq!(
            parsed(&["powershell", "-Command"]).await,
            vec![argv(&["powershell", "-Command"])]
        );
    }

    #[tokio::test]
    async fn pwsh_is_not_extracted() {
        assert_eq!(
            parsed(&["pwsh", "-Command", "ls"]).await,
            vec![argv(&["pwsh", "-Command", "ls"])]
        );
    }
}
