pub use prost::{DecodeError, Message, bytes::Bytes};

pub const PROTOCOL_VERSION: u64 = 1;

pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/scry.provider.runtime.v1.rs"));

    impl ConversationItem {
        pub fn payload_type(&self) -> &'static str {
            use conversation_item::Item;
            match &self.item {
                Some(Item::UserPrompt(_)) => "user_prompt",
                Some(Item::Message(_)) => "message",
                Some(Item::Reasoning(_)) => "reasoning",
                Some(Item::ToolCall(_)) => "tool_call",
                Some(Item::ToolResult(_)) => "tool_result",
                Some(Item::HostedTool(_)) => "hosted_tool",
                Some(Item::Unknown(_)) | None => "unknown",
            }
        }
    }

    impl serde::Serialize for ConversationItem {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            match &self.item {
                Some(item) => item.serialize(serializer),
                None => Err(serde::ser::Error::custom("empty conversation item")),
            }
        }
    }

    impl<'de> serde::Deserialize<'de> for ConversationItem {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            conversation_item::Item::deserialize(deserializer).map(|item| Self { item: Some(item) })
        }
    }

    impl response_event::Payload {
        pub fn kind(&self) -> &'static str {
            use response_event::Payload;
            match self {
                Payload::HandshakeResponse(_) => "HandshakeResponse",
                Payload::InitializeBackendsResponse(_) => "InitializeBackendsResponse",
                Payload::InitBackendResponse(_) => "InitBackendResponse",
                Payload::RemoveBackendResponse(_) => "RemoveBackendResponse",
                Payload::InitConnectionResponse(_) => "InitConnectionResponse",
                Payload::FinalizeConnectionResponse(_) => "FinalizeConnectionResponse",
                Payload::CancelConnectionResponse(_) => "CancelConnectionResponse",
                Payload::ChatResponse(_) => "ChatResponse",
                Payload::ListModelsResponse(_) => "ListModelsResponse",
                Payload::HealthStatusResponse(_) => "HealthStatusResponse",
                Payload::BackendInitErrorResponse(_) => "BackendInitErrorResponse",
                Payload::CancelChatResponse(_) => "CancelChatResponse",
                Payload::BackendHealthStatusResponse(_) => "BackendHealthStatusResponse",
                Payload::ProviderError(_) => "ProviderError",
                Payload::AuthUpdateRequest(_) => "AuthUpdateRequest",
            }
        }
    }
}

#[cfg(test)]
mod stored_shape_tests {
    use serde_json::json;

    use crate::v1::{self, conversation_item::Item};

    fn item(inner: Item) -> v1::ConversationItem {
        v1::ConversationItem { item: Some(inner) }
    }

    #[test]
    fn user_prompt_serializes_in_stored_shape() {
        let value = serde_json::to_value(item(Item::UserPrompt(v1::UserPrompt {
            prompt: "hello".into(),
        })))
        .unwrap();
        assert_eq!(value, json!({"kind": "user_prompt", "prompt": "hello"}));
    }

    #[test]
    fn empty_provider_meta_is_omitted() {
        let value =
            serde_json::to_value(item(Item::Message(v1::ConversationMessage::default()))).unwrap();
        assert_eq!(value, json!({"kind": "message", "message": []}));
    }

    #[test]
    fn stored_rows_round_trip() {
        let rows = [
            json!({"kind": "user_prompt", "prompt": "hello"}),
            json!({
                "kind": "message",
                "message": [
                    {"content": "assistant text", "provider_meta": {"type": "output_text"}},
                    {"content": "no meta"},
                ],
                "provider_meta": {"id": "msg_123", "status": "completed"}
            }),
            json!({
                "kind": "reasoning",
                "reasoning": [{"content": "summary text", "provider_meta": {"type": "summary_text"}}],
                "provider_meta": {"id": "reasoning_123"}
            }),
            json!({
                "kind": "tool_call",
                "call_id": "call_123",
                "name": "shell",
                "arguments": "{\"cmd\":\"pwd\"}",
                "provider_meta": {"id": "fc_123"}
            }),
            json!({"kind": "tool_result", "call_id": "call_123", "name": "shell", "output": "ok"}),
            json!({
                "kind": "hosted_tool",
                "function_type": "web_search_call",
                "content": "searched docs",
                "provider_meta": {"id": "ws_123"}
            }),
            json!({"kind": "hosted_tool", "function_type": "web_search_call"}),
            json!({"kind": "unknown", "provider_meta": {"raw": "x"}}),
        ];
        for row in rows {
            let decoded: v1::ConversationItem = serde_json::from_value(row.clone()).unwrap();
            assert_eq!(serde_json::to_value(&decoded).unwrap(), row);
        }
    }

    #[test]
    fn malformed_rows_are_rejected() {
        for row in [
            // Variant fields without defaults stay required.
            json!({"kind": "unknown"}),
            json!({"kind": "user_prompt"}),
            // The `kind` tag itself is required.
            json!({"prompt": "hello"}),
            json!({}),
        ] {
            let result = serde_json::from_value::<v1::ConversationItem>(row.clone());
            assert!(result.is_err(), "expected rejection of {row}");
        }
    }

    #[test]
    fn empty_item_does_not_serialize() {
        assert!(serde_json::to_value(v1::ConversationItem { item: None }).is_err());
    }

    #[test]
    fn payload_type_matches_stored_strings() {
        let cases = [
            (Item::UserPrompt(v1::UserPrompt::default()), "user_prompt"),
            (Item::Message(v1::ConversationMessage::default()), "message"),
            (Item::Reasoning(v1::Reasoning::default()), "reasoning"),
            (Item::ToolCall(v1::ToolCall::default()), "tool_call"),
            (Item::ToolResult(v1::ToolResult::default()), "tool_result"),
            (Item::HostedTool(v1::HostedTool::default()), "hosted_tool"),
            (Item::Unknown(v1::Unknown::default()), "unknown"),
        ];
        for (inner, expected) in cases {
            assert_eq!(item(inner).payload_type(), expected);
        }
        assert_eq!(
            v1::ConversationItem { item: None }.payload_type(),
            "unknown"
        );
    }
}
