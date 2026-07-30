mod test_support {
    use std::collections::HashMap;

    use paloma_provider_protocol::v1::{self, conversation_item::Item, request_event::Payload};
    use paloma_utils::transport::VarintDelimitedCodec;
    use prost::bytes::BytesMut;
    use tokio_util::codec::Decoder;

    pub const JAVA_FIXTURE: &[u8] = include_bytes!("testdata/fixture_java.bin");

    pub fn buf(bytes: &[u8]) -> BytesMut {
        let mut b = BytesMut::new();
        b.extend_from_slice(bytes);
        b
    }

    pub fn expected_events() -> Vec<v1::RequestEvent> {
        vec![
            v1::RequestEvent {
                event_id: 7,
                payload: Some(Payload::HandshakeRequest(v1::HandshakeRequest {})),
                ..Default::default()
            },
            v1::RequestEvent {
                event_id: 8,
                backend_id: Some("deepseek".to_owned()),
                payload: Some(Payload::ChatRequest(v1::ChatRequest {
                    session_id: "sess-abc".to_owned(),
                    instruction: "be concise".to_owned(),
                    model: "deepseek-chat".to_owned(),
                    effort: "medium".to_owned(),
                    messages: vec![v1::ChatRequestMessage {
                        provider_id: "deepseek".to_owned(),
                        backend_id: "api".to_owned(),
                        item: Some(v1::ConversationItem {
                            item: Some(Item::Message(v1::ConversationMessage {
                                message: vec![v1::MessageContentItem {
                                    content: "hello".to_owned(),
                                    provider_meta: HashMap::from([(
                                        "type".to_owned(),
                                        "output_text".to_owned(),
                                    )]),
                                }],
                                provider_meta: HashMap::from([
                                    ("id".to_owned(), "msg_123".to_owned()),
                                    ("status".to_owned(), "completed".to_owned()),
                                ]),
                            })),
                        }),
                    }],
                    tools: vec![v1::ToolDefinition {
                        name: "shell".to_owned(),
                        description: "run a shell command".to_owned(),
                        parameters: "{\"type\":\"object\"}".to_owned(),
                    }],
                })),
            },
            v1::RequestEvent {
                event_id: 9,
                backend_id: Some("deepseek".to_owned()),
                payload: Some(Payload::HealthStatusRequest(v1::HealthStatusRequest {})),
            },
        ]
    }

    pub fn decode_all(bytes: &[u8]) -> Vec<BytesMut> {
        let mut codec = VarintDelimitedCodec;
        let mut src = buf(bytes);
        let mut frames = Vec::new();
        while let Some(frame) = codec.decode(&mut src).expect("well-formed frames") {
            frames.push(frame);
        }
        assert!(src.is_empty(), "decoder left {} trailing bytes", src.len());
        frames
    }
}

mod encoder_tests {
    use paloma_utils::transport::VarintDelimitedCodec;
    use prost::{
        Message,
        bytes::{Bytes, BytesMut},
    };
    use tokio_util::codec::Encoder;

    use super::test_support::{JAVA_FIXTURE, expected_events};

    fn encode(message: &impl Message) -> BytesMut {
        let mut dst = BytesMut::new();
        VarintDelimitedCodec
            .encode(Bytes::from(message.encode_to_vec()), &mut dst)
            .expect("encode is infallible");
        dst
    }

    #[test]
    fn encoded_result_should_match_protobuf_generated_encode() {
        for event in expected_events() {
            assert_eq!(&encode(&event)[..], event.encode_length_delimited_to_vec(),);
        }
    }

    #[test]
    fn for_non_map_encoded_result_match_protobuf_encode_from_other_language() {
        let events = expected_events();

        let handshake = encode(&events[0]);
        assert_eq!(&JAVA_FIXTURE[..handshake.len()], &handshake[..],);

        let health = encode(&events[2]);
        let tail = JAVA_FIXTURE.len() - health.len();
        assert_eq!(&JAVA_FIXTURE[tail..], &health[..],);
    }
}

mod decoder_tests {
    use paloma_provider_protocol::v1::RequestEvent;
    use prost::Message;

    use super::test_support::{JAVA_FIXTURE, decode_all, expected_events};

    #[test]
    fn decodes_encoded_payload_from_other_language() {
        let decoded: Vec<RequestEvent> = decode_all(JAVA_FIXTURE)
            .iter()
            .map(|f| RequestEvent::decode(&f[..]).expect("valid protobuf payload"))
            .collect();
        assert_eq!(decoded, expected_events());
    }
}
