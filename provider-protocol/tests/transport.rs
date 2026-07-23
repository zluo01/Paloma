mod test_support {
    use std::collections::HashMap;

    use prost::bytes::BytesMut;
    use scry_provider_protocol::v1::{self, conversation_item::Item, request_event::Payload};
    use scry_utils::transport::VarintDelimitedCodec;
    use tokio_util::codec::Decoder;

    pub const JAVA_FIXTURE: &[u8] =
        include_bytes!("../../assets/fixtures/testdata/fixture_java.bin");

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
    use prost::{
        Message,
        bytes::{Bytes, BytesMut},
    };
    use scry_provider_protocol::v1::RequestEvent;
    use scry_utils::transport::VarintDelimitedCodec;
    use tokio_util::codec::Encoder;

    use super::test_support::{JAVA_FIXTURE, decode_all, expected_events};

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

    #[test]
    fn empty_payload_encodes_to_a_single_zero() {
        let mut dst = BytesMut::new();
        VarintDelimitedCodec.encode(Bytes::new(), &mut dst).unwrap();
        assert_eq!(&dst[..], &[0x00]);
    }

    #[test]
    fn encode_then_decode_should_produce_same_results() {
        let events = expected_events();
        let mut wire = BytesMut::new();
        for event in &events {
            VarintDelimitedCodec
                .encode(Bytes::from(event.encode_to_vec()), &mut wire)
                .unwrap();
        }
        let decoded: Vec<RequestEvent> = decode_all(&wire)
            .iter()
            .map(|f| RequestEvent::decode(&f[..]).expect("valid payload"))
            .collect();
        assert_eq!(decoded, events);
    }
}

mod decoder_tests {
    use prost::Message;
    use scry_provider_protocol::v1::RequestEvent;
    use scry_utils::transport::VarintDelimitedCodec;
    use tokio_util::codec::Decoder;

    use super::test_support::{JAVA_FIXTURE, buf, decode_all, expected_events};

    #[test]
    fn decodes_encoded_payload_from_other_language() {
        let decoded: Vec<RequestEvent> = decode_all(JAVA_FIXTURE)
            .iter()
            .map(|f| RequestEvent::decode(&f[..]).expect("valid protobuf payload"))
            .collect();
        assert_eq!(decoded, expected_events());
    }

    #[test]
    fn do_not_decode_on_not_enough_bytes() {
        let first_frame_len = 1 + JAVA_FIXTURE[0] as usize; // 1-byte prefix + payload
        let mut codec = VarintDelimitedCodec;
        for cut in 0..first_frame_len {
            let mut src = buf(&JAVA_FIXTURE[..cut]);
            assert_eq!(codec.decode(&mut src).unwrap(), None);
            assert_eq!(src.len(), cut);
        }
    }

    #[test]
    fn should_decode_frame_split_across_reads() {
        let expected = expected_events()[0].encode_to_vec();
        let mut codec = VarintDelimitedCodec;
        let mut src = buf(&[]);
        let mut frame = None;
        for &byte in JAVA_FIXTURE {
            src.extend_from_slice(&[byte]);
            frame = codec.decode(&mut src).unwrap();
            if frame.is_some() {
                break;
            }
        }
        assert_eq!(&frame.expect("a frame should decode")[..], &expected[..]);
        assert!(src.is_empty());
    }

    #[test]
    fn when_buffer_with_more_than_one_message_should_only_handle_first() {
        let frame_len = 1 + JAVA_FIXTURE[0] as usize;
        let end = frame_len + 3; // whole first frame + 3 bytes of the next
        let expected = expected_events()[0].encode_to_vec();
        let mut codec = VarintDelimitedCodec;
        let mut src = buf(&JAVA_FIXTURE[..end]);
        let frame = codec
            .decode(&mut src)
            .unwrap()
            .expect("first frame complete");
        assert_eq!(&frame[..], &expected[..]);
        assert_eq!(codec.decode(&mut src).unwrap(), None);
        assert_eq!(&src[..], &JAVA_FIXTURE[frame_len..end]);
    }

    #[test]
    fn should_wait_on_incomplete_varint_length() {
        assert_eq!(
            VarintDelimitedCodec.decode(&mut buf(&[0x80; 9])).unwrap(),
            None
        );
    }

    #[test]
    fn should_fail_on_corrupted_varint_length() {
        assert!(VarintDelimitedCodec.decode(&mut buf(&[0x80; 10])).is_err());
    }

    #[test]
    fn varint_overflowing_u64_is_rejected() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F];
        assert!(VarintDelimitedCodec.decode(&mut buf(&bytes)).is_err());
    }

    #[test]
    fn oversize_length_is_rejected_before_buffering() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01];
        assert!(VarintDelimitedCodec.decode(&mut buf(&bytes)).is_err());
    }
}
