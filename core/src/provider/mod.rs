mod connection;

use std::{process::exit, sync::LazyLock};

pub use connection::{ChatStream, ProviderConnectionError, ProviderPlugin};

use crate::{HealthStatus, entity::Plugin};

const PLUGIN_ANTHROPIC: &str = "Anthropic";
const PLUGIN_OPENAI: &str = "OpenAI";

const PROVIDER_PLUGIN_FLAG: &str = "--provider-plugin";

pub(crate) static BUILTIN_PROVIDERS: LazyLock<Vec<Plugin>> = LazyLock::new(|| {
    vec![
        Plugin::builtin(PLUGIN_ANTHROPIC, PROVIDER_PLUGIN_FLAG),
        Plugin::builtin(PLUGIN_OPENAI, PROVIDER_PLUGIN_FLAG),
    ]
});

#[derive(Clone)]
pub struct ProviderInfo {
    pub name: String,
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
    /// `None` for built-in providers, which have no user-editable config.
    pub config: Option<Plugin>,
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
