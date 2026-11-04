//! MAVLink v1/v2 framing over a byte stream, CRC validation and message encoding for the
//! ArduPilot dialect. I/O lives in `transport`; message meaning lives in `vehicle-state`.
use mavlink::{calculate_crc, MAVLinkV1MessageRaw, MAVLinkV2MessageRaw};

pub use mavlink::dialects::ardupilotmega as dialect;
pub use mavlink::dialects::ardupilotmega::MavMessage;
pub use mavlink::{MavHeader, MavlinkVersion, Message};

const MAGIC_V1: u8 = 0xFE;
const MAGIC_V2: u8 = 0xFD;
const IFLAG_SIGNED: u8 = 0x01;
const SIGNATURE_LEN: usize = 13;
const CHECKSUM_LEN: usize = 2;

/// Identity this ground station uses on the wire (matches Mission Planner).
pub const GCS_SYSTEM_ID: u8 = 255;
pub const GCS_COMPONENT_ID: u8 = 190;

#[derive(Debug, Clone)]
pub struct Packet {
    pub header: MavHeader,
    pub message: MavMessage,
    pub version: MavlinkVersion,
    pub wire_len: usize,
}

impl Packet {
    pub fn system_id(&self) -> u8 {
        self.header.system_id as u8
    }
    pub fn component_id(&self) -> u8 {
        self.header.component_id
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParserStats {
    pub packets: u64,
    pub bad_crc: u64,
    pub unknown: u64,
    pub dropped_bytes: u64,
}

/// Incremental parser: feed it bytes as they arrive, get back every complete valid packet.
#[derive(Debug, Default)]
pub struct Parser {
    buf: Vec<u8>,
    stats: ParserStats,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> ParserStats {
        self.stats
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Packet> {
        self.buf.extend_from_slice(bytes);
        let mut packets = Vec::new();
        let mut pos = 0;
        loop {
            let Some(start) = self.buf[pos..]
                .iter()
                .position(|b| *b == MAGIC_V1 || *b == MAGIC_V2)
            else {
                self.stats.dropped_bytes += (self.buf.len() - pos) as u64;
                pos = self.buf.len();
                break;
            };
            self.stats.dropped_bytes += start as u64;
            pos += start;
            match self.try_frame(pos) {
                Frame::Incomplete => break,
                Frame::Skip => {
                    self.stats.dropped_bytes += 1;
                    pos += 1;
                }
                Frame::Packet(packet) => {
                    pos += packet.wire_len;
                    packets.push(*packet);
                }
            }
        }
        self.buf.drain(..pos);
        packets
    }

    fn try_frame(&mut self, at: usize) -> Frame {
        let buf = &self.buf[at..];
        if buf.len() < 3 {
            return Frame::Incomplete;
        }
        let payload_len = buf[1] as usize;
        let (version, header_len, signed) = if buf[0] == MAGIC_V1 {
            (MavlinkVersion::V1, 5, false)
        } else {
            let incompat = buf[2];
            // SYSID32 and unknown incompatibility flags are not supported: resync past the magic.
            if incompat & !IFLAG_SIGNED != 0 {
                return Frame::Skip;
            }
            (MavlinkVersion::V2, 9, incompat & IFLAG_SIGNED != 0)
        };
        let body_end = 1 + header_len + payload_len;
        let total = body_end + CHECKSUM_LEN + if signed { SIGNATURE_LEN } else { 0 };
        if buf.len() < total {
            return Frame::Incomplete;
        }
        let (seq, sys, comp, msgid) = match version {
            MavlinkVersion::V1 => (buf[2], buf[3], buf[4], buf[5] as u32),
            MavlinkVersion::V2 => (
                buf[4],
                buf[5],
                buf[6],
                u32::from_le_bytes([buf[7], buf[8], buf[9], 0]),
            ),
        };
        if MavMessage::default_message_from_id(msgid).is_none() {
            self.stats.unknown += 1;
            return Frame::Skip;
        }
        let wire_crc = u16::from_le_bytes([buf[body_end], buf[body_end + 1]]);
        if calculate_crc(&buf[1..body_end], MavMessage::extra_crc(msgid)) != wire_crc {
            self.stats.bad_crc += 1;
            return Frame::Skip;
        }
        let payload = &buf[1 + header_len..body_end];
        match MavMessage::parse(version, msgid, payload) {
            Ok(message) => {
                self.stats.packets += 1;
                Frame::Packet(Box::new(Packet {
                    header: MavHeader {
                        system_id: sys as u32,
                        component_id: comp,
                        sequence: seq,
                    },
                    message,
                    version,
                    wire_len: total,
                }))
            }
            Err(_) => {
                self.stats.unknown += 1;
                Frame::Skip
            }
        }
    }
}

enum Frame {
    Incomplete,
    Skip,
    Packet(Box<Packet>),
}

/// Serializes a message into a complete wire frame.
pub fn encode(header: MavHeader, message: &MavMessage, version: MavlinkVersion) -> Vec<u8> {
    match version {
        MavlinkVersion::V1 => {
            let mut raw = MAVLinkV1MessageRaw::new();
            raw.serialize_message(header, message);
            raw.raw_bytes().to_vec()
        }
        MavlinkVersion::V2 => {
            let mut raw = MAVLinkV2MessageRaw::new();
            raw.serialize_message(header, message);
            raw.raw_bytes().to_vec()
        }
    }
}

/// The heartbeat this ground station broadcasts so vehicles know a GCS is present.
pub fn gcs_heartbeat() -> MavMessage {
    use dialect::{MavAutopilot, MavModeFlag, MavState, MavType, HEARTBEAT_DATA};
    MavMessage::HEARTBEAT(HEARTBEAT_DATA {
        custom_mode: 0,
        mavtype: MavType::MAV_TYPE_GCS,
        autopilot: MavAutopilot::MAV_AUTOPILOT_INVALID,
        base_mode: MavModeFlag::empty(),
        system_status: MavState::MAV_STATE_ACTIVE,
        mavlink_version: 3,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dialect::{ATTITUDE_DATA, HEARTBEAT_DATA};

    fn header(seq: u8) -> MavHeader {
        MavHeader {
            system_id: 1,
            component_id: 1,
            sequence: seq,
        }
    }

    fn attitude() -> MavMessage {
        MavMessage::ATTITUDE(ATTITUDE_DATA {
            time_boot_ms: 1234,
            roll: 0.5,
            pitch: -0.25,
            yaw: 1.0,
            rollspeed: 0.0,
            pitchspeed: 0.0,
            yawspeed: 0.0,
        })
    }

    #[test]
    fn roundtrips_v2_and_v1() {
        for version in [MavlinkVersion::V2, MavlinkVersion::V1] {
            let bytes = encode(header(7), &attitude(), version);
            let packets = Parser::new().push(&bytes);
            assert_eq!(packets.len(), 1, "{version:?}");
            assert_eq!(packets[0].version, version);
            assert_eq!(packets[0].header.sequence, 7);
            assert_eq!(packets[0].system_id(), 1);
            assert_eq!(packets[0].wire_len, bytes.len());
            match &packets[0].message {
                MavMessage::ATTITUDE(a) => {
                    assert_eq!(a.time_boot_ms, 1234);
                    assert!((a.roll - 0.5).abs() < 1e-6);
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn v2_truncated_zero_payload_still_decodes() {
        let heartbeat = MavMessage::HEARTBEAT(HEARTBEAT_DATA::default());
        let bytes = encode(header(0), &heartbeat, MavlinkVersion::V2);
        let packets = Parser::new().push(&bytes);
        assert!(matches!(packets[0].message, MavMessage::HEARTBEAT(_)));
    }

    #[test]
    fn handles_split_chunks_and_garbage() {
        let mut stream = vec![0x00, 0x13, 0xFD, 0xFF];
        stream.extend(encode(header(1), &attitude(), MavlinkVersion::V2));
        stream.extend([0xAA; 5]);
        stream.extend(encode(header(2), &attitude(), MavlinkVersion::V1));
        let mut parser = Parser::new();
        let mut got = Vec::new();
        for chunk in stream.chunks(3) {
            got.extend(parser.push(chunk));
        }
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].header.sequence, 1);
        assert_eq!(got[1].header.sequence, 2);
        assert!(parser.stats().dropped_bytes > 0);
    }

    #[test]
    fn rejects_bad_crc() {
        let mut bytes = encode(header(1), &attitude(), MavlinkVersion::V2);
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let mut parser = Parser::new();
        assert!(parser.push(&bytes).is_empty());
        assert_eq!(parser.stats().bad_crc, 1);
    }

    #[test]
    fn gcs_heartbeat_is_a_gcs() {
        let bytes = encode(
            MavHeader {
                system_id: GCS_SYSTEM_ID as u32,
                component_id: GCS_COMPONENT_ID,
                sequence: 0,
            },
            &gcs_heartbeat(),
            MavlinkVersion::V2,
        );
        let packets = Parser::new().push(&bytes);
        match &packets[0].message {
            MavMessage::HEARTBEAT(h) => assert_eq!(h.mavtype, dialect::MavType::MAV_TYPE_GCS),
            other => panic!("unexpected {other:?}"),
        }
    }
}
