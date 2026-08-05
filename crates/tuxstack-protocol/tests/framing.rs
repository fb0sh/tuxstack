use std::io::Cursor;

use tuxstack_protocol::{
    ClientHello, FrameError, PROTOCOL_VERSION, ProtocolBody, ProtocolEnvelope, Request,
    decode_frame, encode_frame, encode_frame_with_limit, read_frame,
};

fn ping() -> ProtocolEnvelope {
    ProtocolEnvelope::new(7, ProtocolBody::Ping)
}

#[test]
fn cbor_frame_round_trips() {
    let envelope = ProtocolEnvelope::new(
        42,
        ProtocolBody::Request(Request::GetProviderDescriptor(
            tuxstack_protocol::ResourcePath::root(
                tuxstack_protocol::DockerResourceRef::Container {
                    container_id: "abc".into(),
                },
            ),
        )),
    );
    let encoded = encode_frame(&envelope).unwrap();
    assert_eq!(decode_frame(&encoded).unwrap(), envelope);
    assert_eq!(read_frame(&mut Cursor::new(encoded)).unwrap(), envelope);
    assert!(matches!(
        encode_frame_with_limit(&envelope, 1),
        Err(FrameError::Oversized { maximum: 1, .. })
    ));
}

#[test]
fn rejects_zero_oversized_and_truncated_frames() {
    assert_eq!(
        decode_frame(&0_u32.to_be_bytes()),
        Err(FrameError::ZeroLength)
    );

    let oversized = (tuxstack_protocol::MAX_FRAME_SIZE + 1).to_be_bytes();
    assert!(matches!(
        decode_frame(&oversized),
        Err(FrameError::Oversized { .. })
    ));
    assert_eq!(
        decode_frame(&[0, 0, 0]),
        Err(FrameError::TruncatedHeader { actual: 3 })
    );
    assert_eq!(
        decode_frame(&[0, 0, 0, 4, 1, 2]),
        Err(FrameError::TruncatedBody {
            expected: 4,
            actual: 2
        })
    );
}

#[test]
fn rejects_unknown_protocol_and_trailing_bytes() {
    let mut unknown = ping();
    unknown.protocol_version = PROTOCOL_VERSION + 1;
    let mut payload = Vec::new();
    ciborium::ser::into_writer(&unknown, &mut payload).unwrap();
    let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
    frame.extend(payload);
    assert!(matches!(
        decode_frame(&frame),
        Err(FrameError::UnknownProtocol { .. })
    ));

    let mut valid = encode_frame(&ping()).unwrap();
    valid.push(0);
    assert_eq!(decode_frame(&valid), Err(FrameError::TrailingBytes(1)));

    let mut payload_with_junk = Vec::new();
    ciborium::ser::into_writer(&ping(), &mut payload_with_junk).unwrap();
    payload_with_junk.push(0xff);
    let mut inner_trailing = (payload_with_junk.len() as u32).to_be_bytes().to_vec();
    inner_trailing.extend(payload_with_junk);
    assert!(matches!(
        decode_frame(&inner_trailing),
        Err(FrameError::TrailingBytes(1))
    ));
}

#[test]
fn malformed_data_never_panics_or_bypasses_bounds() {
    // Deterministic fuzz-like coverage without a heavyweight fuzz dependency.
    let mut state = 0x5eed_f00d_dead_beef_u64;
    for length in 0..4096_usize {
        let mut bytes = vec![0_u8; length];
        for byte in &mut bytes {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        let outcome = std::panic::catch_unwind(|| decode_frame(&bytes));
        assert!(
            outcome.is_ok(),
            "decoder panicked for {length} random bytes"
        );
        if length >= 4 {
            let declared = u32::from_be_bytes(bytes[..4].try_into().unwrap());
            if declared > tuxstack_protocol::MAX_FRAME_SIZE {
                assert!(matches!(
                    outcome.unwrap(),
                    Err(FrameError::Oversized { .. })
                ));
            }
        }
    }
}

#[test]
fn hello_carries_negotiation_inputs() {
    let hello = ClientHello::current("test-client");
    assert_eq!(hello.supported_protocol_versions, vec![PROTOCOL_VERSION]);
    assert_eq!(hello.max_frame_size, tuxstack_protocol::MAX_FRAME_SIZE);
}
