mod connection;
mod controller;

use std::{process::exit, sync::LazyLock};

pub use connection::{ExtensionConnectionError, ExtensionPlugin};
pub use controller::{ExtensionController, ExtensionControllerError};

use crate::{
    HealthStatus,
    entity::{CapabilityInfo, Plugin},
};

const PLUGIN_INTERNAL: &str = "Internal";
pub(crate) const PLUGIN_SHELL: &str = paloma_extension_shell::EXTENSION_ID;
pub(crate) const EXEC_CAPABILITY: &str = paloma_extension_shell::CAPABILITY_ID;

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
    pub capabilities: Vec<CapabilityInfo>,
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
        PLUGIN_INTERNAL => paloma_extension_internal::run(),
        PLUGIN_SHELL => paloma_extension_shell::run(),
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
