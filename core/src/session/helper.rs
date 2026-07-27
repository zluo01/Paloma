use std::sync::LazyLock;

use log::error;
use scry_extension_shell::ShellArgs;
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
        description: Some(spec.schema.description),
        require_permission: vec![spec.name, spec.tool],
    }
}

pub(crate) fn prettify_arg(args: &str) -> String {
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or(args.to_string())
}
