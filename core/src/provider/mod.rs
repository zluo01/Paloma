mod connection;

use std::{collections::HashMap, process::exit, sync::LazyLock};

pub use connection::{ChatStream, ProviderConnectionError, ProviderPlugin};

use crate::{
    HealthStatus,
    entity::{Plugin, PluginArgs, Transport},
};

pub(crate) const PLUGIN_ANTHROPIC: &str = "Anthropic";
pub(crate) const PLUGIN_OPENAI: &str = "OpenAI";

const PROVIDER_PLUGIN_FLAG: &str = "--provider-plugin";

pub struct ProviderInfo {
    pub name: String,
    pub description: String,
    pub status: HealthStatus,
}

pub(crate) fn serve_plugin_and_exit_if_requested() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(PROVIDER_PLUGIN_FLAG) {
        return;
    }
    let Some(name) = args.next() else {
        eprintln!("{PROVIDER_PLUGIN_FLAG} requires a provider plugin name");
        exit(1);
    };
    let result = match name.as_str() {
        PLUGIN_ANTHROPIC => scry_provider_anthropic::run(),
        PLUGIN_OPENAI => scry_provider_openai::run(),
        _ => {
            eprintln!("unknown provider plugin {name}");
            exit(1);
        },
    };
    match result {
        Ok(()) => exit(0),
        Err(e) => {
            eprintln!("provider plugin {name} failed: {e}");
            exit(1);
        },
    }
}

pub(crate) static ANTHROPIC_PLUGIN: LazyLock<Plugin> =
    LazyLock::new(|| builtin_plugin(PLUGIN_ANTHROPIC));

pub(crate) static OPENAI_PLUGIN: LazyLock<Plugin> = LazyLock::new(|| builtin_plugin(PLUGIN_OPENAI));

fn builtin_plugin(name: &str) -> Plugin {
    let command = std::env::current_exe()
        .unwrap_or_else(|_| {
            panic!("current executable path should be resolvable for plugin {name}.")
        })
        .to_string_lossy()
        .into_owned();
    Plugin {
        name: name.to_string(),
        transport: Transport::Local,
        // no global timeout in provider plugin
        timeout: 0,
        disabled: false,
        env: HashMap::new(),
        args: PluginArgs::Local {
            command,
            args: vec![PROVIDER_PLUGIN_FLAG.to_string(), name.to_string()],
        },
    }
}
