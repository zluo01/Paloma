use std::sync::LazyLock;

use paloma_provider_protocol::v1::{Backend, ProviderAuthMethod};

pub(crate) const PROVIDER_ID: &str = "Anthropic";

pub(crate) mod backend_id {
    pub(crate) const ANTHROPIC_API: &str = "Anthropic API";
    pub(crate) const CLAUDE_CODE: &str = "Claude Code";
}

const CLAUDE_ICON: &[u8] = include_bytes!("../assets/claude.svg");

pub(crate) static BACKENDS: LazyLock<Vec<Backend>> = LazyLock::new(|| {
    vec![
        Backend {
            backend_id: backend_id::ANTHROPIC_API.into(),
            description: "Anthropic models through API key.".into(),
            icon: Some(CLAUDE_ICON.to_vec()),
            auth_kind: ProviderAuthMethod::ApiKey as i32,
        },
        Backend {
            backend_id: backend_id::CLAUDE_CODE.into(),
            description: "Anthropic models through Claude subscription.".into(),
            icon: Some(CLAUDE_ICON.to_vec()),
            auth_kind: ProviderAuthMethod::BrowserOauth as i32,
        },
    ]
});
