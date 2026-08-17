use std::sync::LazyLock;

use log::error;
use paloma_extension_shell::ShellArgs;
use serde_json::Value;

use crate::{
    entity::ToolSpec,
    extension::{PLUGIN_SHELL, SHELL_CAPABILITY},
    utils::ext_tool_name_encode,
};

pub(crate) enum Disposition {
    Gated {
        name: String,
        arguments: String,
        description: Option<String>,
        require_permission: Vec<String>,
    },
    Passthrough,
    Skip,
}

static SHELL_KEY: LazyLock<String> =
    LazyLock::new(|| ext_tool_name_encode(PLUGIN_SHELL, SHELL_CAPABILITY));

/// Namespace for tool approvals
const TOOL_PERMISSION_NAMESPACE: &str = "tool:";

/// extract commands and description for permission checking
pub(crate) fn extract_args(spec: ToolSpec, raw_args: &str) -> Disposition {
    // have to hardcode on the shell tool to extract the commands for approval
    if spec.schema.name == *SHELL_KEY {
        return match serde_json::from_str::<ShellArgs>(raw_args) {
            Ok(args) => Disposition::Gated {
                name: PLUGIN_SHELL.to_string(),
                arguments: args.command.join(" "),
                description: Some(args.description),
                require_permission: args.command,
            },
            Err(err) => {
                error!("malformed shell arguments: {err}");
                Disposition::Skip
            },
        };
    }
    // all other tools
    Disposition::Gated {
        name: format!("{} - {}", spec.name, spec.tool),
        arguments: prettify_arg(raw_args),
        description: Some(spec.schema.short_description),
        require_permission: vec![
            format!("{TOOL_PERMISSION_NAMESPACE}{}", spec.name),
            spec.tool,
        ],
    }
}

pub(crate) fn prettify_arg(args: &str) -> String {
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or(args.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ToolSchema;

    fn spec(name: &str, tool: &str) -> ToolSpec {
        ToolSpec {
            name: name.to_string(),
            tool: tool.to_string(),
            schema: ToolSchema {
                name: format!("{name}__{tool}"),
                description: String::new(),
                short_description: String::new(),
                parameters: serde_json::json!({}),
            },
        }
    }

    #[test]
    fn tool_permission_key_cannot_collide_with_shell_argv() {
        let Disposition::Gated {
            require_permission, ..
        } = extract_args(spec("git", "status"), "{}")
        else {
            panic!("tool calls are gated");
        };

        assert_eq!(require_permission, ["tool:git", "status"]);
    }
}
