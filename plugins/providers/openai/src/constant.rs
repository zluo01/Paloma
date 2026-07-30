use std::sync::LazyLock;

use paloma_provider_protocol::v1::{Backend, ProviderAuthMethod};

pub(crate) const PROVIDER_ID: &str = "OpenAI";

pub(crate) mod backend_id {
    pub(crate) const OPENAI_API: &str = "OpenAI API";
    pub(crate) const CODEX: &str = "Codex";
}

/// Codex is an OpenAI product; both backends share the OpenAI mark.
const OPENAI_ICON: &[u8] = include_bytes!("../assets/openai.svg");

/// Backends advertised in the handshake; cloned into each response.
pub(crate) static BACKENDS: LazyLock<Vec<Backend>> = LazyLock::new(|| {
    vec![
        Backend {
            backend_id: backend_id::OPENAI_API.into(),
            description: "OpenAI models through API key.".into(),
            icon: Some(OPENAI_ICON.to_vec()),
            auth_kind: ProviderAuthMethod::ApiKey as i32,
        },
        Backend {
            backend_id: backend_id::CODEX.into(),
            description: "OpenAI models with ChatGPT subscription.".into(),
            icon: Some(OPENAI_ICON.to_vec()),
            auth_kind: ProviderAuthMethod::DeviceCode as i32,
        },
    ]
});
