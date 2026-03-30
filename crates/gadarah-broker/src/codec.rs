//! cTrader Protocol Codec
//! 
//! Handles encoding/decoding of cTrader OpenAPI messages.
//! The protocol uses a simple length-prefixed binary format.

use bytes::{Buf, BufMut, BytesMut};
use std::io::{Error, ErrorKind};
use tracing::{debug, trace};

/// Maximum message size (8MB as per cTrader docs)
pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;

/// cTrader message header size: 4 bytes for length + 2 bytes for payload type
pub const HEADER_SIZE: usize = 6;

pub struct CtraderCodec;

impl CtraderCodec {
    /// Encode a message with cTrader header
    /// 
    /// Format: [2 bytes payload type][4 bytes message length][message bytes]
    pub fn encode(&self, data: &[u8], payload_type: u16) -> Result<BytesMut, Error> {
        let mut buf = BytesMut::new();
        
        // Reserve space for header
        buf.reserve(HEADER_SIZE + data.len());
        
        // Write payload type (2 bytes, little-endian for cTrader)
        buf.put_u16_le(payload_type);
        
        // Write message length (4 bytes, little-endian)
        buf.put_u32_le(data.len() as u32);
        
        // Write message data
        buf.extend_from_slice(data);
        
        trace!("Encoded message: {} bytes", buf.len());
        Ok(buf)
    }
    
    /// Decode a cTrader message from buffer
    /// Returns (payload_type, message_bytes) if complete message available
    pub fn decode(&self, src: &mut BytesMut) -> Result<Option<(u16, Vec<u8>)>, Error> {
        // Need at least header to determine message length
        if src.remaining() < HEADER_SIZE {
            return Ok(None);
        }
        
        // Peek at header
        let payload_type = src.get_u16_le();
        let msg_length = src.get_u32_le() as usize;
        
        // Validate message length
        if msg_length > MAX_MESSAGE_SIZE {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("Message too large: {} bytes", msg_length)
            ));
        }
        
        // Check if we have the full message
        if src.remaining() < msg_length {
            // Put back the header bytes we peeked - need to reconstruct
            let mut header = BytesMut::new();
            header.put_u16_le(payload_type);
            header.put_u32_le(msg_length as u32);
            header.extend_from_slice(&src[..]);
            *src = header;
            return Ok(None);
        }
        
        // Extract message bytes
        let msg_bytes = src.copy_to_bytes(msg_length);
        
        debug!("Decoded message: payload_type={}, length={}", payload_type, msg_length);
        
        Ok(Some((payload_type, msg_bytes.to_vec())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_encode_decode() {
        let codec = CtraderCodec;
        let data = b"test message";
        
        // Encode
        let encoded = codec.encode(data, 1001).unwrap();
        assert!(encoded.len() > HEADER_SIZE);
        
        // Decode
        let mut buf = BytesMut::from(encoded.as_ref());
        let result = codec.decode(&mut buf).unwrap();
        
        assert!(result.is_some());
        let (pt, payload) = result.unwrap();
        assert_eq!(pt, 1001);
        assert_eq!(payload, data);
    }
}