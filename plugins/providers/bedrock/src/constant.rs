use std::sync::LazyLock;

use paloma_provider_protocol::v1::{Backend, ProviderAuthMethod};

pub(crate) const PROVIDER_ID: &str = "Amazon Bedrock";

pub(crate) mod backend_id {
    pub(crate) const BEDROCK_API: &str = "Bedrock API";
}

const BEDROCK_ICON: &[u8] = include_bytes!("../assets/bedrock.svg");

pub(crate) static BACKENDS: LazyLock<Vec<Backend>> = LazyLock::new(|| {
    vec![Backend {
        backend_id: backend_id::BEDROCK_API.into(),
        description: "Foundation models on Amazon Bedrock through API key or IAM credentials over Converse API."
            .into(),
        icon: Some(BEDROCK_ICON.to_vec()),
        auth_kind: ProviderAuthMethod::ApiKey as i32,
    }]
});
