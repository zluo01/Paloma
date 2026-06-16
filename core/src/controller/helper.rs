use log::error;

use crate::capability::{Shell, ShellArgs, Tool, ToolSpec};

pub(crate) enum Disposition {
    Gated(Vec<String>, Option<String>),
    Passthrough,
    Skip,
}

/// extract commands and description for permission checking
pub(crate) fn extract_args(spec: ToolSpec, raw_args: &str) -> Disposition {
    if spec.name == Shell::NAME {
        match serde_json::from_str::<ShellArgs>(raw_args) {
            Ok(args) => Disposition::Gated(args.command, Some(args.description)),
            Err(err) => {
                error!("malformed shell arguments: {err}");
                Disposition::Skip
            },
        }
    } else if let Some(tool) = spec.tool {
        // mcp command for permission will be simply server name + tool name
        Disposition::Gated(vec![spec.name, tool], Some(spec.schema.description))
    } else {
        // non-shell, non-mcp tool (e.g. web_search): render plainly, no gating
        Disposition::Passthrough
    }
}
