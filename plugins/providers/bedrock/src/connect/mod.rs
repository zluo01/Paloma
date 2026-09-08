use paloma_provider_base::{ProviderAuthenticator, ProviderError, Result};
use paloma_provider_protocol::v1::{
    ConnectionPayload, Instruction, InstructionLink, ManualInput, ProviderAuth, connection_payload,
    finalize_connection_request, instruction, provider_auth,
};

use crate::constant::backend_id;

pub(crate) const INSTRUCTION_URL: &str =
    "https://docs.aws.amazon.com/bedrock/latest/userguide/api-keys.html#api-keys-gen-long";

/// Supports two ways, Long Term API key or IAM role
/// Long Term API will be passed through connection, empty string on IAM role.
/// IAM role will go through default AWS SDK credential provider
pub(crate) struct BedrockConnector;

#[async_trait::async_trait]
impl ProviderAuthenticator for BedrockConnector {
    fn id(&self) -> String {
        backend_id::BEDROCK_API.into()
    }

    async fn init_connection(&self) -> Result<ConnectionPayload> {
        Ok(ConnectionPayload {
            payload: Some(connection_payload::Payload::ManualInput(ManualInput {
                instructions: vec![
                    text("Enter a "),
                    link("long-term API key", INSTRUCTION_URL),
                    text(
                        " in <region>:<api-key> format. Leave blank to use the AWS credentials configured on this device.",
                    ),
                ],
            })),
        })
    }

    async fn finalize_connection(
        &self,
        input: finalize_connection_request::Input,
    ) -> Result<ProviderAuth> {
        let finalize_connection_request::Input::ApiKey(credential) = input else {
            return Err(ProviderError::InvalidConnection { expected: "ApiKey" });
        };

        let credential = credential.trim();
        // empty means using IAM role
        if !credential.is_empty() {
            BedrockCredential::parse(credential)?;
        }

        Ok(ProviderAuth {
            payload: Some(provider_auth::Payload::ApiKey(credential.to_string())),
        })
    }
}

fn text(text: &str) -> Instruction {
    Instruction {
        content: Some(instruction::Content::Text(text.to_string())),
    }
}

fn link(label: &str, link: &str) -> Instruction {
    Instruction {
        content: Some(instruction::Content::Link(InstructionLink {
            label: label.to_string(),
            link: link.to_string(),
        })),
    }
}

/// `<aws_region>:<api key>`
pub(crate) struct BedrockCredential {
    pub region: String,
    pub api_key: String,
}

impl BedrockCredential {
    pub(crate) fn parse(credential: &str) -> Result<Self> {
        let invalid = || {
            ProviderError::Other(
                "Amazon Bedrock api key must be entered as `<region>:<api key>`, e.g. `us-east-1:ABSK...`"
                    .into(),
            )
        };
        let (region, api_key) = credential.split_once(':').ok_or_else(invalid)?;
        if region.is_empty() || api_key.is_empty() || region.contains(char::is_whitespace) {
            return Err(invalid());
        }
        Ok(Self {
            region: region.to_string(),
            api_key: api_key.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse {
        use super::*;

        #[test]
        fn region_colon_key_input_gives_region_and_key() {
            let credential = BedrockCredential::parse("us-east-1:ABSKexample").unwrap();

            assert_eq!(credential.region, "us-east-1");
            assert_eq!(credential.api_key, "ABSKexample");
        }

        #[test]
        fn key_containing_colons_keeps_everything_after_the_first_colon_as_key() {
            let credential = BedrockCredential::parse("eu-west-1:ab:cd==").unwrap();

            assert_eq!(credential.region, "eu-west-1");
            assert_eq!(credential.api_key, "ab:cd==");
        }

        #[test]
        fn input_without_colon_is_rejected() {
            assert!(BedrockCredential::parse("ABSKexample").is_err());
        }

        #[test]
        fn input_with_empty_region_or_empty_key_is_rejected() {
            for credential in [":ABSKexample", "us-east-1:", ":"] {
                assert!(
                    BedrockCredential::parse(credential).is_err(),
                    "{credential}"
                );
            }
        }

        #[test]
        fn region_containing_whitespace_is_rejected() {
            assert!(BedrockCredential::parse("us east 1:ABSKexample").is_err());
        }
    }

    mod finalize_connection {
        use super::*;

        async fn finalize(input: &str) -> Result<ProviderAuth> {
            BedrockConnector
                .finalize_connection(finalize_connection_request::Input::ApiKey(input.into()))
                .await
        }

        fn api_key(auth: ProviderAuth) -> String {
            match auth.payload {
                Some(provider_auth::Payload::ApiKey(key)) => key,
                other => panic!("expected api key payload, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn blank_input_is_stored_as_empty_string_for_iam() {
            assert_eq!(api_key(finalize("   ").await.unwrap()), "");
        }

        #[tokio::test]
        async fn region_colon_key_input_is_stored_trimmed() {
            assert_eq!(
                api_key(finalize("  us-east-1:ABSKexample \n").await.unwrap()),
                "us-east-1:ABSKexample"
            );
        }

        #[tokio::test]
        async fn input_without_region_is_rejected() {
            assert!(finalize("ABSKexample").await.is_err());
        }

        #[tokio::test]
        async fn non_api_key_input_is_rejected_as_invalid_connection() {
            let result = BedrockConnector
                .finalize_connection(finalize_connection_request::Input::AuthorizationResponse(
                    "code".into(),
                ))
                .await;

            assert!(matches!(
                result,
                Err(ProviderError::InvalidConnection { expected: "ApiKey" })
            ));
        }
    }
}
