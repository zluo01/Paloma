//! Bash composite parser, Logic partially ported from openai/codex
//! (`codex-rs/shell-command/src/bash.rs`).

use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Tree;
use tree_sitter_bash::LANGUAGE as BASH;

use crate::error::{PermissionError, Result};
use crate::utils::is_supported_shell;

pub(crate) fn parse_commands(command: &[String]) -> Result<Option<Vec<Vec<String>>>> {
    if command.is_empty() {
        return Err(PermissionError::EmptyCommand);
    }
    let script = extract_bash_command(command)
        .map(str::to_owned)
        .unwrap_or_else(|| command.join(" "));
    let Some(tree) = try_parse_shell(&script) else {
        return Ok(None);
    };
    match try_parse_word_only_commands_sequence(&tree, &script) {
        Some(commands) if commands.is_empty() => Err(PermissionError::EmptyCommand),
        other => Ok(other),
    }
}

/// Parse the provided bash source using tree-sitter-bash, returning a `Tree`
/// on success or `None` if parsing failed.
fn try_parse_shell(shell_lc_arg: &str) -> Option<Tree> {
    let lang = BASH.into();
    let mut parser = Parser::new();
    parser
        .set_language(&lang)
        .expect("tree-sitter-bash grammar should load");
    let old_tree: Option<&Tree> = None;
    parser.parse(shell_lc_arg, old_tree)
}

/// If `command` has the shape `[shell, -c|-lc, script]` where `shell` is in
/// [`crate::utils::SHELLS`], return the inner script. Otherwise `None`.
fn extract_bash_command(command: &[String]) -> Option<&str> {
    let [shell, flag, script] = command else {
        return None;
    };
    if !matches!(flag.as_str(), "-lc" | "-c") {
        return None;
    }
    if !is_supported_shell(shell) {
        return None;
    }
    Some(script)
}

/// Parse a script which may contain multiple simple commands joined only by
/// the safe logical/pipe/sequencing operators: `&&`, `||`, `;`, `|`.
///
/// Returns `Some(Vec<command_words>)` if every command is a plain word‑only
/// command and the parse tree does not contain disallowed constructs
/// (parentheses, redirections, substitutions, control flow, etc.). Otherwise
/// returns `None`.
fn try_parse_word_only_commands_sequence(tree: &Tree, src: &str) -> Option<Vec<Vec<String>>> {
    if tree.root_node().has_error() {
        return None;
    }

    // List of allowed (named) node kinds for a "word only commands sequence".
    // If we encounter a named node that is not in this list we reject.
    const ALLOWED_KINDS: &[&str] = &[
        // top level containers
        "program",
        "list",
        "pipeline",
        // commands & words
        "command",
        "command_name",
        "word",
        "string",
        "string_content",
        "raw_string",
        "number",
        "concatenation",
    ];
    // Allow only safe punctuation / operator tokens; anything else causes reject.
    const ALLOWED_PUNCT_TOKENS: &[&str] = &["&&", "||", ";", "|", "\"", "'"];

    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut stack = vec![root];
    let mut command_nodes = Vec::new();
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        if node.is_named() {
            if !ALLOWED_KINDS.contains(&kind) {
                return None;
            }
            if kind == "command" {
                command_nodes.push(node);
            }
        } else {
            // Reject any punctuation / operator tokens that are not explicitly allowed.
            if kind.chars().any(|c| "&;|".contains(c)) && !ALLOWED_PUNCT_TOKENS.contains(&kind) {
                return None;
            }
            if !(ALLOWED_PUNCT_TOKENS.contains(&kind) || kind.trim().is_empty()) {
                // If it's a quote token or operator it's allowed above; we also allow whitespace tokens.
                // Any other punctuation like parentheses, braces, redirects, backticks, etc are rejected.
                return None;
            }
        }
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    // Walk uses a stack (LIFO), so re-sort by position to restore source order.
    command_nodes.sort_by_key(Node::start_byte);

    let mut commands = Vec::new();
    for node in command_nodes {
        if let Some(words) = parse_plain_command_from_node(node, src) {
            commands.push(words);
        } else {
            return None;
        }
    }
    Some(commands)
}

fn parse_plain_command_from_node(cmd: Node, src: &str) -> Option<Vec<String>> {
    if cmd.kind() != "command" {
        return None;
    }
    let mut words = Vec::new();
    let mut cursor = cmd.walk();
    for child in cmd.named_children(&mut cursor) {
        match child.kind() {
            "command_name" => {
                let word_node = child.named_child(0)?;
                if word_node.kind() != "word" {
                    return None;
                }
                words.push(word_node.utf8_text(src.as_bytes()).ok()?.to_owned());
            }
            "word" | "number" => {
                words.push(child.utf8_text(src.as_bytes()).ok()?.to_owned());
            }
            "string" => {
                let parsed = parse_double_quoted_string(child, src)?;
                words.push(parsed);
            }
            "raw_string" => {
                let parsed = parse_raw_string(child, src)?;
                words.push(parsed);
            }
            "concatenation" => {
                // Handle concatenated arguments like -g"*.py"
                let mut concatenated = String::new();
                let mut concat_cursor = child.walk();
                for part in child.named_children(&mut concat_cursor) {
                    match part.kind() {
                        "word" | "number" => {
                            concatenated
                                .push_str(part.utf8_text(src.as_bytes()).ok()?.to_owned().as_str());
                        }
                        "string" => {
                            let parsed = parse_double_quoted_string(part, src)?;
                            concatenated.push_str(&parsed);
                        }
                        "raw_string" => {
                            let parsed = parse_raw_string(part, src)?;
                            concatenated.push_str(&parsed);
                        }
                        _ => return None,
                    }
                }
                if concatenated.is_empty() {
                    return None;
                }
                words.push(concatenated);
            }
            _ => return None,
        }
    }
    Some(words)
}

fn parse_double_quoted_string(node: Node, src: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }

    let mut cursor = node.walk();
    for part in node.named_children(&mut cursor) {
        if part.kind() != "string_content" {
            return None;
        }
    }
    let raw = node.utf8_text(src.as_bytes()).ok()?;
    let stripped = raw
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))?;
    Some(stripped.to_string())
}

fn parse_raw_string(node: Node, src: &str) -> Option<String> {
    if node.kind() != "raw_string" {
        return None;
    }

    let raw_string = node.utf8_text(src.as_bytes()).ok()?;
    let stripped = raw_string
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''));
    stripped.map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    fn parsed(parts: &[&str]) -> Vec<Vec<String>> {
        parse_commands(&argv(parts)).unwrap().unwrap()
    }

    fn is_complex(parts: &[&str]) -> bool {
        matches!(parse_commands(&argv(parts)), Ok(None))
    }

    #[test]
    fn accepts_single_simple_command() {
        assert_eq!(
            parsed(&["ls -1"]),
            vec![vec!["ls".to_string(), "-1".to_string()]]
        );
    }

    #[test]
    fn accepts_multiple_commands_with_allowed_operators() {
        let cmds = parsed(&["ls && pwd; echo 'hi there' | wc -l"]);
        let expected: Vec<Vec<String>> = vec![
            vec!["ls".to_string()],
            vec!["pwd".to_string()],
            vec!["echo".to_string(), "hi there".to_string()],
            vec!["wc".to_string(), "-l".to_string()],
        ];
        assert_eq!(cmds, expected);
    }

    #[test]
    fn extracts_double_and_single_quoted_strings() {
        assert_eq!(
            parsed(&["echo \"hello world\""]),
            vec![vec!["echo".to_string(), "hello world".to_string()]]
        );
        assert_eq!(
            parsed(&["echo 'hi there'"]),
            vec![vec!["echo".to_string(), "hi there".to_string()]]
        );
    }

    #[test]
    fn accepts_double_quoted_strings_with_newlines() {
        assert_eq!(
            parsed(&["git commit -m \"line1\nline2\""]),
            vec![vec![
                "git".to_string(),
                "commit".to_string(),
                "-m".to_string(),
                "line1\nline2".to_string(),
            ]]
        );
    }

    #[test]
    fn accepts_mixed_quote_concatenation() {
        assert_eq!(
            parsed(&[r#"echo "/usr"'/'"local"/bin"#]),
            vec![vec!["echo".to_string(), "/usr/local/bin".to_string()]]
        );
        assert_eq!(
            parsed(&[r#"echo '/usr'"/"'local'/bin"#]),
            vec![vec!["echo".to_string(), "/usr/local/bin".to_string()]]
        );
    }

    #[test]
    fn rejects_double_quoted_strings_with_expansions() {
        assert!(is_complex(&[r#"echo "hi ${USER}""#]));
        assert!(is_complex(&[r#"echo "$HOME""#]));
    }

    #[test]
    fn accepts_numbers_as_words() {
        assert_eq!(
            parsed(&["echo 123 456"]),
            vec![vec![
                "echo".to_string(),
                "123".to_string(),
                "456".to_string()
            ]]
        );
    }

    #[test]
    fn rejects_parentheses_and_subshells() {
        assert!(is_complex(&["(ls)"]));
        assert!(is_complex(&["ls || (pwd && echo hi)"]));
    }

    #[test]
    fn rejects_redirections_and_unsupported_operators() {
        assert!(is_complex(&["ls > out.txt"]));
        assert!(is_complex(&["echo hi & echo bye"]));
    }

    #[test]
    fn rejects_command_and_process_substitutions_and_expansions() {
        assert!(is_complex(&["echo $(pwd)"]));
        assert!(is_complex(&["echo `pwd`"]));
        assert!(is_complex(&["echo $HOME"]));
        assert!(is_complex(&["echo \"hi $USER\""]));
    }

    #[test]
    fn rejects_variable_assignment_prefix() {
        assert!(is_complex(&["FOO=bar ls"]));
    }

    #[test]
    fn rejects_trailing_operator_parse_error() {
        assert!(is_complex(&["ls &&"]));
    }

    #[test]
    fn rejects_empty_command_position_with_leading_operator() {
        assert!(is_complex(&["&& ls"]));
    }

    #[test]
    fn rejects_empty_command_position_with_double_separator() {
        assert!(is_complex(&["ls ;; pwd"]));
    }

    #[test]
    fn rejects_empty_command_position_with_empty_pipeline_segment() {
        assert!(is_complex(&["ls | | wc"]));
    }

    #[test]
    fn parse_zsh_lc_plain_commands() {
        assert_eq!(parsed(&["zsh", "-lc", "ls"]), vec![vec!["ls".to_string()]]);
    }

    #[test]
    fn accepts_concatenated_flag_and_value() {
        // -g"*.py" (flag directly concatenated with quoted value)
        assert_eq!(
            parsed(&["rg -n \"foo\" -g\"*.py\""]),
            vec![vec![
                "rg".to_string(),
                "-n".to_string(),
                "foo".to_string(),
                "-g*.py".to_string(),
            ]]
        );
    }

    #[test]
    fn accepts_concatenated_flag_with_single_quotes() {
        assert_eq!(
            parsed(&["grep -n 'pattern' -g'*.txt'"]),
            vec![vec![
                "grep".to_string(),
                "-n".to_string(),
                "pattern".to_string(),
                "-g*.txt".to_string(),
            ]]
        );
    }

    #[test]
    fn rejects_concatenation_with_variable_substitution() {
        assert!(is_complex(&["rg -g\"$VAR\" pattern"]));
        assert!(is_complex(&["rg -g\"${VAR}\" pattern"]));
    }

    #[test]
    fn rejects_concatenation_with_command_substitution() {
        assert!(is_complex(&["rg -g\"$(pwd)\" pattern"]));
        assert!(is_complex(&["rg -g\"$(echo '*.py')\" pattern"]));
    }

    #[test]
    fn rejects_complex_for_loop() {
        assert!(is_complex(&[
            "for d in /usr/lib/jvm/*; do echo \"$d\"; done"
        ]));
    }

    #[test]
    fn parse_commands_accepts_supported_shells() {
        let one_ls = vec![vec!["ls".to_string()]];
        for parts in [
            &["bash", "-lc", "ls"][..],
            &["bash", "-c", "ls"][..],
            &["sh", "-c", "ls"][..],
            &["zsh", "-lc", "ls"][..],
            &["/bin/bash", "-lc", "ls"][..],
            &["fish", "-lc", "ls"][..],
            &["dash", "-c", "ls"][..],
            &["ksh", "-c", "ls"][..],
        ] {
            assert_eq!(parsed(parts), one_ls, "{parts:?}");
        }
    }

    #[test]
    fn parse_commands_empty_argv_is_error() {
        assert!(matches!(
            parse_commands(&argv(&[])),
            Err(PermissionError::EmptyCommand)
        ));
    }

    #[test]
    fn parse_commands_passes_through_direct_commands() {
        // Non-shell argv falls back to join-and-parse → one passthrough command.
        assert_eq!(
            parsed(&["ls", "-la"]),
            vec![vec!["ls".to_string(), "-la".to_string()]]
        );
        assert_eq!(
            parsed(&["python", "script.py"]),
            vec![vec!["python".to_string(), "script.py".to_string()]]
        );
        assert_eq!(
            parsed(&["node", "app.js", "--port", "3000"]),
            vec![vec![
                "node".to_string(),
                "app.js".to_string(),
                "--port".to_string(),
                "3000".to_string(),
            ]]
        );
    }

    #[test]
    fn parse_commands_rejects_unparseable_join() {
        // Joined argv contains bash metacharacters → AST whitelist rejects → Complex.
        assert!(is_complex(&["python", "-c", "print(1)"]));
        assert!(is_complex(&["sh", "-c", "echo hi > out"]));
    }

    #[test]
    fn parse_commands_decomposes_complex_scripts() {
        assert_eq!(
            parsed(&["bash", "-lc", "ls && pwd; echo 'hi there' | wc -l"]),
            vec![
                vec!["ls".to_string()],
                vec!["pwd".to_string()],
                vec!["echo".to_string(), "hi there".to_string()],
                vec!["wc".to_string(), "-l".to_string()],
            ]
        );

        assert_eq!(
            parsed(&["bash", "-c", "git commit -m \"line1\nline2\""]),
            vec![vec![
                "git".to_string(),
                "commit".to_string(),
                "-m".to_string(),
                "line1\nline2".to_string(),
            ]]
        );

        assert_eq!(
            parsed(&["zsh", "-lc", "rg -n \"foo\" -g\"*.py\""]),
            vec![vec![
                "rg".to_string(),
                "-n".to_string(),
                "foo".to_string(),
                "-g*.py".to_string(),
            ]]
        );

        assert_eq!(
            parsed(&["sh", "-c", "echo 123 456"]),
            vec![vec![
                "echo".to_string(),
                "123".to_string(),
                "456".to_string(),
            ]]
        );
    }

    #[test]
    fn parse_commands_rejects_complex_scripts() {
        assert!(is_complex(&["bash", "-lc", "echo $(pwd)"]));
        assert!(is_complex(&["bash", "-lc", "echo $HOME"]));
        assert!(is_complex(&["bash", "-lc", "ls > out.txt"]));
        assert!(is_complex(&["bash", "-lc", "(ls && pwd)"]));
        assert!(is_complex(&[
            "bash",
            "-lc",
            "for d in /tmp/*; do echo \"$d\"; done",
        ]));
        assert!(is_complex(&["bash", "-lc", "FOO=bar ls"]));
        assert!(is_complex(&["bash", "-lc", "sleep 1 &"]));
    }
}
