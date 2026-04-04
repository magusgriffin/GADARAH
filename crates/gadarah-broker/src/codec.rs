//! cTrader Protocol Framing
//!
//! Wire format: `[4 bytes big-endian length][ProtoMessage protobuf bytes]`
//!
//! Every message is wrapped in a `ProtoMessage` envelope:
//!   - `payload_type`: identifies the inner message type
//!   - `payload`:      protobuf-encoded inner message
//!   - `client_msg_id`: echoed in responses for request correlation

use bytes::{Buf, BufMut, BytesMut};
use prost::Message as ProstMessage;
use std::io::{Error, ErrorKind};
use tracing::trace;

use crate::proto::ProtoMessage;

/// Maximum frame: 8 MB per cTrader docs.
pub const MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

// ── Encoding ────────────────────────────────────────────────────────────────

/// Wrap `msg` in a `ProtoMessage` envelope and encode as a length-prefixed frame.
pub fn encode_message<T: ProstMessage>(
    payload_type: u32,
    msg: &T,
    client_msg_id: Option<String>,
) -> Result<BytesMut, Error> {
    let mut payload_bytes = Vec::new();
    msg.encode(&mut payload_bytes)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

    let wrapper = ProtoMessage {
        payload_type,
        payload: Some(prost::bytes::Bytes::from(payload_bytes)),
        client_msg_id,
    };
    encode_proto_message(&wrapper)
}

/// Encode a `ProtoMessage` into a length-prefixed frame.
pub fn encode_proto_message(msg: &ProtoMessage) -> Result<BytesMut, Error> {
    let msg_len = msg.encoded_len();
    if msg_len > MAX_FRAME_SIZE {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("Message too large: {msg_len}"),
        ));
    }
    let mut msg_bytes = Vec::with_capacity(msg_len);
    msg.encode(&mut msg_bytes)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

    let mut frame = BytesMut::with_capacity(4 + msg_bytes.len());
    frame.put_u32(msg_bytes.len() as u32); // 4-byte big-endian length
    frame.extend_from_slice(&msg_bytes);
    trace!("Encoded frame: {} payload bytes", msg_bytes.len());
    Ok(frame)
}

// ── Decoding ─────────────────────────────────────────────────────────────────

/// Attempt to decode one complete `ProtoMessage` frame from the buffer.
/// Returns `None` if there is not enough data yet (caller should buffer and retry).
pub fn decode_frame(src: &mut BytesMut) -> Result<Option<ProtoMessage>, Error> {
    if src.len() < 4 {
        return Ok(None);
    }
    let frame_len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;
    if frame_len > MAX_FRAME_SIZE {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("Frame too large: {frame_len} bytes"),
        ));
    }
    if src.len() < 4 + frame_len {
        return Ok(None);
    }
    src.advance(4);
    let msg_bytes = src.split_to(frame_len);
    trace!("Decoding frame: {} bytes", frame_len);
    let msg = ProtoMessage::decode(msg_bytes.as_ref())
        .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Proto decode: {e}")))?;
    Ok(Some(msg))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::ProtoHeartbeatEvent;

    #[test]
    fn encode_decode_roundtrip() {
        // Encode a heartbeat wrapped in ProtoMessage
        let hb = ProtoHeartbeatEvent { payload_type: None };
        let frame = encode_message(51, &hb, Some("msg-1".to_string())).unwrap();

        let mut buf = BytesMut::from(frame.as_ref());
        let decoded = decode_frame(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.payload_type, 51);
        assert_eq!(decoded.client_msg_id.as_deref(), Some("msg-1"));
        assert!(buf.is_empty(), "buffer should be fully consumed");
    }

    #[test]
    fn partial_frame_returns_none() {
        let hb = ProtoHeartbeatEvent { payload_type: None };
        let frame = encode_message(51, &hb, None).unwrap();

        // Give only the first 3 bytes (header incomplete)
        let mut buf = BytesMut::from(&frame[..3]);
        assert!(decode_frame(&mut buf).unwrap().is_none());
    }

    #[test]
    fn two_frames_decoded_sequentially() {
        let hb = ProtoHeartbeatEvent { payload_type: None };
        let frame1 = encode_message(51, &hb, Some("1".to_string())).unwrap();
        let frame2 = encode_message(51, &hb, Some("2".to_string())).unwrap();

        let mut buf = BytesMut::new();
        buf.extend_from_slice(&frame1);
        buf.extend_from_slice(&frame2);

        let msg1 = decode_frame(&mut buf).unwrap().unwrap();
        let msg2 = decode_frame(&mut buf).unwrap().unwrap();
        assert_eq!(msg1.client_msg_id.as_deref(), Some("1"));
        assert_eq!(msg2.client_msg_id.as_deref(), Some("2"));
        assert!(buf.is_empty());
    }
}
