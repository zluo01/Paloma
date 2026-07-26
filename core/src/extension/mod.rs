mod connection;

use std::{process::exit, sync::LazyLock};

pub use connection::{ExtensionConnectionError, ExtensionPlugin};
use scry_extension_protocol::v1::Capability;

use crate::{HealthStatus, entity::Plugin};

const PLUGIN_INTERNAL: &str = "Internal";
const PLUGIN_SHELL: &str = "Shell";

const EXTENSION_PLUGIN_FLAG: &str = "--extension-plugin";

pub(crate) static BUILTIN_EXTENSIONS: LazyLock<Vec<Plugin>> = LazyLock::new(|| {
    vec![
        Plugin::builtin(PLUGIN_INTERNAL, EXTENSION_PLUGIN_FLAG),
        Plugin::builtin(PLUGIN_SHELL, EXTENSION_PLUGIN_FLAG),
    ]
});

#[derive(Clone)]
pub struct ExtensionInfo {
    pub name: String,
    pub description: String,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub capabilities: Vec<Capability>,
    pub status: HealthStatus,
    pub error: Option<String>,
    /// `None` for built-in providers, which have no user-editable config.
    pub config: Option<Plugin>,
}

pub(crate) fn serve_extension_plugin_and_exit_if_requested() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(EXTENSION_PLUGIN_FLAG) {
        return;
    }
    let Some(name) = args.next() else {
        eprintln!("{EXTENSION_PLUGIN_FLAG} requires an extension plugin name");
        exit(1);
    };
    let result = match name.as_str() {
        PLUGIN_INTERNAL => scry_extension_internal::run(),
        PLUGIN_SHELL => scry_extension_shell::run(),
        _ => {
            eprintln!("unknown extension plugin {name}");
            exit(1);
        },
    };
    match result {
        Ok(()) => exit(0),
        Err(e) => {
            eprintln!("extension plugin {name} failed: {e}");
            exit(1);
        },
    }
}
