//! A deliberately small MAVLink v1 framing layer. Dialect-specific decoding belongs above it.
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const V1_MAGIC: u8 = 0xFE;
pub const HEARTBEAT_ID: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub sequence: u8,
    pub system_id: u8,
    pub component_id: u8,
    pub message_id: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("not a MAVLink v1 frame")]
    InvalidMagic,
    #[error("truncated frame: expected {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },
}

impl Frame {
    /// Parses framing only. CRC/dialect validation is intentionally delegated to the protocol layer.
    pub fn parse_v1(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.first() != Some(&V1_MAGIC) {
            return Err(ParseError::InvalidMagic);
        }
        if bytes.len() < 8 {
            return Err(ParseError::Truncated {
                expected: 8,
                actual: bytes.len(),
            });
        }
        let payload_len = bytes[1] as usize;
        let expected = payload_len + 8;
        if bytes.len() < expected {
            return Err(ParseError::Truncated {
                expected,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            sequence: bytes[2],
            system_id: bytes[3],
            component_id: bytes[4],
            message_id: bytes[5],
            payload: bytes[6..6 + payload_len].to_vec(),
        })
    }

    pub fn is_heartbeat(&self) -> bool {
        self.message_id == HEARTBEAT_ID
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_heartbeat_frame() {
        let frame = Frame::parse_v1(&[0xFE, 0, 4, 1, 1, 0, 0, 0]).unwrap();
        assert!(frame.is_heartbeat());
        assert_eq!(frame.system_id, 1);
    }
    #[test]
    fn rejects_truncated_packet() {
        assert!(matches!(
            Frame::parse_v1(&[0xFE, 9]),
            Err(ParseError::Truncated { .. })
        ));
    }
}
