//! The CAN transfer layer: the 29-bit identifier, the tail byte, multi-frame transfers with a
//! CRC-16 seeded by the data type signature, and reassembly with a timeout.
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// A transfer that stays incomplete this long is dropped.
pub const TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);
/// Largest payload of a classic CAN frame (7 data bytes and the tail byte).
pub const CLASSIC_FRAME: usize = 8;
/// Largest payload of a CAN FD frame.
pub const FD_FRAME: usize = 64;
/// Priority the ground station uses (the lowest the legacy tool sent with).
pub const GCS_PRIORITY: u8 = 30;

const FD_LENGTHS: [usize; 8] = [8, 12, 16, 20, 24, 32, 48, 64];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanFrame {
    /// The 29-bit extended identifier.
    pub id: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferKind {
    /// A broadcast message. Anonymous ones (source node 0) carry only the low two bits of the id.
    Message {
        type_id: u16,
    },
    Request {
        service_id: u8,
        destination: u8,
    },
    Response {
        service_id: u8,
        destination: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub priority: u8,
    /// 0 is anonymous.
    pub source: u8,
    pub kind: TransferKind,
    /// 5 bits.
    pub transfer_id: u8,
    pub payload: Vec<u8>,
}

/// CRC-16/CCITT-FALSE.
pub fn crc16(mut crc: u16, bytes: &[u8]) -> u16 {
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn transfer_crc(signature: u64, payload: &[u8]) -> u16 {
    crc16(crc16(0xFFFF, &signature.to_le_bytes()), payload)
}

/// The identifier of a transfer. `discriminator` only matters for anonymous messages.
pub fn encode_id(transfer: &Transfer, discriminator: u16) -> u32 {
    let priority = u32::from(transfer.priority & 0x1F) << 24;
    let source = u32::from(transfer.source & 0x7F);
    match transfer.kind {
        TransferKind::Message { type_id } if transfer.source == 0 => {
            priority | (u32::from(discriminator & 0x3FFF) << 10) | (u32::from(type_id & 3) << 8)
        }
        TransferKind::Message { type_id } => priority | (u32::from(type_id) << 8) | source,
        TransferKind::Request {
            service_id,
            destination,
        } => {
            priority
                | (u32::from(service_id) << 16)
                | (1 << 15)
                | (u32::from(destination & 0x7F) << 8)
                | (1 << 7)
                | source
        }
        TransferKind::Response {
            service_id,
            destination,
        } => {
            priority
                | (u32::from(service_id) << 16)
                | (u32::from(destination & 0x7F) << 8)
                | (1 << 7)
                | source
        }
    }
}

/// Splits an identifier. Returns the priority, source node and kind.
pub fn decode_id(id: u32) -> (u8, u8, TransferKind) {
    let priority = ((id >> 24) & 0x1F) as u8;
    let source = (id & 0x7F) as u8;
    let kind = if id & (1 << 7) != 0 {
        let service_id = ((id >> 16) & 0xFF) as u8;
        let destination = ((id >> 8) & 0x7F) as u8;
        if id & (1 << 15) != 0 {
            TransferKind::Request {
                service_id,
                destination,
            }
        } else {
            TransferKind::Response {
                service_id,
                destination,
            }
        }
    } else if source == 0 {
        TransferKind::Message {
            type_id: ((id >> 8) & 3) as u16,
        }
    } else {
        TransferKind::Message {
            type_id: ((id >> 8) & 0xFFFF) as u16,
        }
    };
    (priority, source, kind)
}

fn tail(start: bool, end: bool, toggle: bool, transfer_id: u8) -> u8 {
    (u8::from(start) << 7) | (u8::from(end) << 6) | (u8::from(toggle) << 5) | (transfer_id & 0x1F)
}

/// Pads a CAN FD frame up to the next valid length, with zeros before the tail byte.
fn finish_frame(mut data: Vec<u8>, tail_byte: u8, fd: bool) -> Vec<u8> {
    if fd {
        let wanted = FD_LENGTHS
            .iter()
            .copied()
            .find(|&n| n > data.len())
            .unwrap_or(FD_FRAME);
        data.resize(wanted - 1, 0);
    }
    data.push(tail_byte);
    data
}

/// Turns a transfer into CAN frames. `signature` seeds the CRC of multi-frame transfers.
pub fn encode_transfer(
    transfer: &Transfer,
    signature: u64,
    fd: bool,
    discriminator: u16,
) -> Vec<CanFrame> {
    let id = encode_id(transfer, discriminator);
    let capacity = if fd { FD_FRAME } else { CLASSIC_FRAME } - 1;
    let payload = &transfer.payload;
    if payload.len() <= capacity {
        let data = finish_frame(
            payload.clone(),
            tail(true, true, false, transfer.transfer_id),
            fd,
        );
        return vec![CanFrame { id, data }];
    }
    let crc = transfer_crc(signature, payload);
    let mut stream = Vec::with_capacity(payload.len() + 2);
    stream.extend_from_slice(&crc.to_le_bytes());
    stream.extend_from_slice(payload);
    let chunks: Vec<&[u8]> = stream.chunks(capacity).collect();
    let last = chunks.len() - 1;
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| CanFrame {
            id,
            data: finish_frame(
                chunk.to_vec(),
                tail(
                    index == 0,
                    index == last,
                    index % 2 == 1,
                    transfer.transfer_id,
                ),
                fd && index == last,
            ),
        })
        .collect()
}

struct Session {
    data: Vec<u8>,
    next_toggle: bool,
    transfer_id: u8,
    started: Instant,
}

/// Collects frames into transfers.
#[derive(Default)]
pub struct Reassembler {
    sessions: HashMap<u32, Session>,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one frame. `signature_of` gives the CRC seed for a kind of transfer; a transfer of
    /// a type it does not know is dropped. Returns the finished transfer, if this frame ended one.
    pub fn push(
        &mut self,
        frame: &CanFrame,
        now: Instant,
        signature_of: impl Fn(&TransferKind) -> Option<u64>,
    ) -> Option<Transfer> {
        self.sessions
            .retain(|_, s| now.duration_since(s.started) < TRANSFER_TIMEOUT);
        let (&tail_byte, body) = frame.data.split_last()?;
        let start = tail_byte & 0x80 != 0;
        let end = tail_byte & 0x40 != 0;
        let toggle = tail_byte & 0x20 != 0;
        let transfer_id = tail_byte & 0x1F;
        let (priority, source, kind) = decode_id(frame.id);
        let key = frame.id & 0x00FF_FFFF;

        if start && end {
            // A single-frame transfer. Anonymous transfers can only be this.
            self.sessions.remove(&key);
            return Some(Transfer {
                priority,
                source,
                kind,
                transfer_id,
                payload: body.to_vec(),
            });
        }
        if source == 0 {
            return None; // anonymous transfers cannot be multi-frame
        }
        if start {
            self.sessions.insert(
                key,
                Session {
                    data: body.to_vec(),
                    next_toggle: true,
                    transfer_id,
                    started: now,
                },
            );
            return None;
        }
        let session = self.sessions.get_mut(&key)?;
        if session.transfer_id != transfer_id || session.next_toggle != toggle {
            self.sessions.remove(&key);
            return None;
        }
        session.data.extend_from_slice(body);
        session.next_toggle = !session.next_toggle;
        if !end {
            return None;
        }
        let session = self.sessions.remove(&key)?;
        let signature = signature_of(&kind)?;
        let (crc_bytes, payload) = session.data.split_first_chunk::<2>()?;
        let expected = u16::from_le_bytes(*crc_bytes);
        // CAN FD pads the last frame with zeros before the tail byte; accept the payload with or
        // without that padding.
        let max_strip = payload
            .iter()
            .rev()
            .take(FD_FRAME)
            .take_while(|&&b| b == 0)
            .count();
        (0..=max_strip).find_map(|strip| {
            let candidate = &payload[..payload.len() - strip];
            (transfer_crc(signature, candidate) == expected).then(|| Transfer {
                priority,
                source,
                kind,
                transfer_id,
                payload: candidate.to_vec(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNATURE: u64 = 0xEE46_8A81_21C4_6A9E;

    fn transfer(kind: TransferKind, payload: Vec<u8>) -> Transfer {
        Transfer {
            priority: 20,
            source: 10,
            kind,
            transfer_id: 7,
            payload,
        }
    }

    fn reassemble(frames: &[CanFrame]) -> Option<Transfer> {
        let mut r = Reassembler::new();
        let now = Instant::now();
        frames
            .iter()
            .filter_map(|f| r.push(f, now, |_| Some(SIGNATURE)))
            .last()
    }

    #[test]
    fn crc16_check_value() {
        assert_eq!(crc16(0xFFFF, b"123456789"), 0x29B1);
    }

    #[test]
    fn broadcast_identifier_layout() {
        let t = transfer(TransferKind::Message { type_id: 341 }, vec![1, 2, 3]);
        // priority 20, type 341, node 10.
        assert_eq!(encode_id(&t, 0), 0x1401_550A);
        assert_eq!(
            decode_id(0x1401_550A),
            (20, 10, TransferKind::Message { type_id: 341 })
        );
        let frames = encode_transfer(&t, SIGNATURE, false, 0);
        assert_eq!(frames.len(), 1);
        // start + end, toggle 0, transfer id 7.
        assert_eq!(frames[0].data, vec![1, 2, 3, 0b1100_0111]);
    }

    #[test]
    fn service_identifier_layout() {
        let request = transfer(
            TransferKind::Request {
                service_id: 1,
                destination: 42,
            },
            vec![],
        );
        let id = encode_id(&request, 0);
        assert_eq!(
            id,
            (20 << 24) | (1 << 16) | (1 << 15) | (42 << 8) | (1 << 7) | 10
        );
        assert_eq!(
            decode_id(id),
            (
                20,
                10,
                TransferKind::Request {
                    service_id: 1,
                    destination: 42
                }
            )
        );
        let response = transfer(
            TransferKind::Response {
                service_id: 11,
                destination: 127,
            },
            vec![],
        );
        assert_eq!(
            decode_id(encode_id(&response, 0)).2,
            TransferKind::Response {
                service_id: 11,
                destination: 127
            }
        );
    }

    #[test]
    fn anonymous_messages_carry_two_type_bits() {
        let mut t = transfer(TransferKind::Message { type_id: 1 }, vec![0x01]);
        t.source = 0;
        let id = encode_id(&t, 0x2ABC);
        assert_eq!(id & 0x7F, 0);
        assert_eq!(decode_id(id), (20, 0, TransferKind::Message { type_id: 1 }));
        assert_eq!((id >> 10) & 0x3FFF, 0x2ABC);
    }

    #[test]
    fn multi_frame_transfers_round_trip() {
        let payload: Vec<u8> = (0..50).collect();
        let t = transfer(
            TransferKind::Response {
                service_id: 1,
                destination: 127,
            },
            payload.clone(),
        );
        let frames = encode_transfer(&t, SIGNATURE, false, 0);
        assert_eq!(
            frames.len(),
            8,
            "2 CRC bytes + 50 payload bytes over 7-byte frames"
        );
        assert_eq!(
            frames[0].data.last().unwrap() & 0xE0,
            0b1000_0000,
            "start, toggle 0"
        );
        assert_eq!(
            frames[1].data.last().unwrap() & 0xE0,
            0b0010_0000,
            "toggle 1"
        );
        assert_eq!(
            frames[7].data.last().unwrap() & 0xE0,
            0b0110_0000,
            "end, toggle 1"
        );
        assert!(frames.iter().all(|f| f.data.len() <= 8));
        let done = reassemble(&frames).expect("reassembled");
        assert_eq!(done.payload, payload);
        assert_eq!(done.kind, t.kind);
        assert_eq!((done.source, done.transfer_id, done.priority), (10, 7, 20));
    }

    #[test]
    fn a_corrupted_or_wrongly_signed_transfer_is_dropped() {
        let t = transfer(TransferKind::Message { type_id: 341 }, (0..20).collect());
        let mut frames = encode_transfer(&t, SIGNATURE, false, 0);
        frames[1].data[0] ^= 0xFF;
        assert!(reassemble(&frames).is_none());
        let good = encode_transfer(&t, SIGNATURE, false, 0);
        let mut r = Reassembler::new();
        let now = Instant::now();
        let out = good
            .iter()
            .filter_map(|f| r.push(f, now, |_| Some(SIGNATURE + 1)))
            .last();
        assert!(out.is_none(), "the CRC is seeded with the signature");
    }

    #[test]
    fn lost_frames_and_stale_sessions_are_dropped() {
        let t = transfer(TransferKind::Message { type_id: 341 }, (0..30).collect());
        let frames = encode_transfer(&t, SIGNATURE, false, 0);
        // A frame in the middle is missing: the toggle bit catches it.
        let missing: Vec<CanFrame> = frames
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 2)
            .map(|(_, f)| f.clone())
            .collect();
        assert!(reassemble(&missing).is_none());
        // The second half arrives after the timeout.
        let mut r = Reassembler::new();
        let start = Instant::now();
        let mut done = None;
        for (i, f) in frames.iter().enumerate() {
            let at = if i < 3 {
                start
            } else {
                start + TRANSFER_TIMEOUT + Duration::from_millis(1)
            };
            done = r.push(f, at, |_| Some(SIGNATURE));
        }
        assert!(done.is_none());
        // Frames without a start are ignored.
        assert!(reassemble(&frames[1..]).is_none());
    }

    #[test]
    fn interleaved_transfers_from_two_nodes_do_not_mix() {
        let a = transfer(TransferKind::Message { type_id: 341 }, (0..20).collect());
        let mut b = transfer(TransferKind::Message { type_id: 341 }, (100..120).collect());
        b.source = 11;
        let fa = encode_transfer(&a, SIGNATURE, false, 0);
        let fb = encode_transfer(&b, SIGNATURE, false, 0);
        let mut r = Reassembler::new();
        let now = Instant::now();
        let mut done = Vec::new();
        for i in 0..fa.len().max(fb.len()) {
            for f in [fa.get(i), fb.get(i)].into_iter().flatten() {
                done.extend(r.push(f, now, |_| Some(SIGNATURE)));
            }
        }
        assert_eq!(done.len(), 2);
        assert!(done
            .iter()
            .any(|t| t.source == 10 && t.payload == a.payload));
        assert!(done
            .iter()
            .any(|t| t.source == 11 && t.payload == b.payload));
    }

    #[test]
    fn can_fd_frames_are_padded_before_the_tail_byte() {
        let single = transfer(TransferKind::Message { type_id: 341 }, vec![9; 9]);
        let frames = encode_transfer(&single, SIGNATURE, true, 0);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].data.len(),
            12,
            "9 + tail rounded up to a valid length"
        );
        assert_eq!(
            *frames[0].data.last().unwrap(),
            0b1100_0111,
            "the tail byte stays last"
        );
        assert_eq!(
            &frames[0].data[9..11],
            &[0, 0],
            "zero padding sits before the tail"
        );

        let payload: Vec<u8> = (1..=100).collect();
        let long = transfer(
            TransferKind::Response {
                service_id: 48,
                destination: 127,
            },
            payload.clone(),
        );
        let frames = encode_transfer(&long, SIGNATURE, true, 0);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].data.len(), 64, "61 payload bytes after the CRC");
        assert!([8, 12, 16, 20, 24, 32, 48, 64].contains(&frames[1].data.len()));
        let done = reassemble(&frames).expect("reassembled");
        assert_eq!(done.payload, payload);
    }

    #[test]
    fn empty_frames_are_ignored() {
        let mut r = Reassembler::new();
        assert!(r
            .push(
                &CanFrame {
                    id: 0x1401_550A,
                    data: vec![]
                },
                Instant::now(),
                |_| Some(SIGNATURE)
            )
            .is_none());
    }
}
