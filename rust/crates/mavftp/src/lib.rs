//! MAVFTP wire format (the payload of FILE_TRANSFER_PROTOCOL): frame encode/decode, NAK codes,
//! directory-listing parsing and the CRC32 the autopilot uses. Pure: no I/O, no MAVLink types.
//! The transfer logic lives in `link::FtpService`.
use serde::Serialize;

/// Size of the FILE_TRANSFER_PROTOCOL payload.
pub const PAYLOAD_LEN: usize = 251;
/// Bytes of header before the data.
pub const HEADER_LEN: usize = 12;
/// Most data bytes a frame can carry.
pub const MAX_DATA: usize = PAYLOAD_LEN - HEADER_LEN;
/// Read/write chunk used by default: small, to keep slow radios happy (same as Mission Planner).
pub const DEFAULT_CHUNK: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    None,
    TerminateSession,
    ResetSessions,
    ListDirectory,
    OpenFileRO,
    ReadFile,
    CreateFile,
    WriteFile,
    RemoveFile,
    CreateDirectory,
    RemoveDirectory,
    OpenFileWO,
    TruncateFile,
    Rename,
    CalcFileCRC32,
    BurstReadFile,
    ListDirectoryWithTime,
    Ack,
    Nak,
}

impl Opcode {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::TerminateSession => 1,
            Self::ResetSessions => 2,
            Self::ListDirectory => 3,
            Self::OpenFileRO => 4,
            Self::ReadFile => 5,
            Self::CreateFile => 6,
            Self::WriteFile => 7,
            Self::RemoveFile => 8,
            Self::CreateDirectory => 9,
            Self::RemoveDirectory => 10,
            Self::OpenFileWO => 11,
            Self::TruncateFile => 12,
            Self::Rename => 13,
            Self::CalcFileCRC32 => 14,
            Self::BurstReadFile => 15,
            Self::ListDirectoryWithTime => 16,
            Self::Ack => 128,
            Self::Nak => 129,
        }
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::None,
            1 => Self::TerminateSession,
            2 => Self::ResetSessions,
            3 => Self::ListDirectory,
            4 => Self::OpenFileRO,
            5 => Self::ReadFile,
            6 => Self::CreateFile,
            7 => Self::WriteFile,
            8 => Self::RemoveFile,
            9 => Self::CreateDirectory,
            10 => Self::RemoveDirectory,
            11 => Self::OpenFileWO,
            12 => Self::TruncateFile,
            13 => Self::Rename,
            14 => Self::CalcFileCRC32,
            15 => Self::BurstReadFile,
            16 => Self::ListDirectoryWithTime,
            128 => Self::Ack,
            129 => Self::Nak,
            _ => return None,
        })
    }
}

/// Why the vehicle refused a request (first data byte of a NAK).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FtpError {
    #[error("the vehicle reported an unspecified failure")]
    Fail,
    #[error("the vehicle failed the operation (errno {0})")]
    FailErrno(u8),
    #[error("invalid data size")]
    InvalidDataSize,
    #[error("the session is not open")]
    InvalidSession,
    #[error("the vehicle has no free file sessions")]
    NoSessionsAvailable,
    #[error("end of file")]
    Eof,
    #[error("the vehicle does not know that command")]
    UnknownCommand,
    #[error("the file already exists")]
    FileExists,
    #[error("the file is protected")]
    FileProtected,
    #[error("file or directory not found")]
    FileNotFound,
    #[error("unknown MAVFTP error {0}")]
    Other(u8),
}

impl FtpError {
    /// Decodes the data of a NAK frame (`data[0]` is the code, `data[1]` the errno for code 2).
    pub fn from_nak(data: &[u8]) -> Self {
        match data.first().copied().unwrap_or(1) {
            1 => Self::Fail,
            2 => Self::FailErrno(data.get(1).copied().unwrap_or(0)),
            3 => Self::InvalidDataSize,
            4 => Self::InvalidSession,
            5 => Self::NoSessionsAvailable,
            6 => Self::Eof,
            7 => Self::UnknownCommand,
            8 => Self::FileExists,
            9 => Self::FileProtected,
            10 => Self::FileNotFound,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("payload of {0} bytes is shorter than the 12 byte header")]
    TooShort(usize),
    #[error("unknown MAVFTP opcode {0}")]
    UnknownOpcode(u8),
    #[error("{0} data bytes do not fit a frame (max {MAX_DATA})")]
    TooMuchData(usize),
}

/// One MAVFTP request or response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub seq: u16,
    pub session: u8,
    pub opcode: Opcode,
    /// For `Read`/`BurstRead` requests this is the number of bytes wanted, not the data length.
    pub size: u8,
    pub req_opcode: Opcode,
    pub burst_complete: bool,
    pub offset: u32,
    pub data: Vec<u8>,
}

impl Frame {
    pub fn request(seq: u16, opcode: Opcode) -> Self {
        Self {
            seq,
            session: 0,
            opcode,
            size: 0,
            req_opcode: Opcode::None,
            burst_complete: false,
            offset: 0,
            data: Vec::new(),
        }
    }

    pub fn with_data(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.data = data.into();
        self.size = self.data.len().min(255) as u8;
        self
    }

    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_session(mut self, session: u8) -> Self {
        self.session = session;
        self
    }

    /// Overrides `size` (the byte count a read request asks for).
    pub fn with_size(mut self, size: u8) -> Self {
        self.size = size;
        self
    }

    pub fn encode(&self) -> Result<[u8; PAYLOAD_LEN], FrameError> {
        if self.data.len() > MAX_DATA {
            return Err(FrameError::TooMuchData(self.data.len()));
        }
        let mut out = [0u8; PAYLOAD_LEN];
        out[0..2].copy_from_slice(&self.seq.to_le_bytes());
        out[2] = self.session;
        out[3] = self.opcode.to_u8();
        out[4] = self.size;
        out[5] = self.req_opcode.to_u8();
        out[6] = u8::from(self.burst_complete);
        out[8..12].copy_from_slice(&self.offset.to_le_bytes());
        out[HEADER_LEN..HEADER_LEN + self.data.len()].copy_from_slice(&self.data);
        Ok(out)
    }

    /// `payload` is the 251 byte MAVLink field; the data length is taken from `size` (clamped).
    pub fn decode(payload: &[u8]) -> Result<Self, FrameError> {
        if payload.len() < HEADER_LEN {
            return Err(FrameError::TooShort(payload.len()));
        }
        let opcode = Opcode::from_u8(payload[3]).ok_or(FrameError::UnknownOpcode(payload[3]))?;
        let req_opcode = Opcode::from_u8(payload[5]).unwrap_or(Opcode::None);
        let size = payload[4];
        let available = payload.len() - HEADER_LEN;
        let data_len = usize::from(size).min(available).min(MAX_DATA);
        Ok(Self {
            seq: u16::from_le_bytes([payload[0], payload[1]]),
            session: payload[2],
            opcode,
            size,
            req_opcode,
            burst_complete: payload[6] != 0,
            offset: u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]),
            data: payload[HEADER_LEN..HEADER_LEN + data_len].to_vec(),
        })
    }

    /// The NAK error carried by this frame, if it is one.
    pub fn nak(&self) -> Option<FtpError> {
        (self.opcode == Opcode::Nak).then(|| FtpError::from_nak(&self.data))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since the Unix epoch, when the vehicle reported a plausible one.
    pub modified: Option<u32>,
}

/// Parses the data of one ListDirectory / ListDirectoryWithTime response. Returns the entries
/// and how many *records* the frame held (skip records count: the next request's offset is the
/// running record count).
pub fn parse_listing(data: &[u8], with_time: bool) -> (Vec<DirEntry>, usize) {
    let mut entries = Vec::new();
    let mut records = 0;
    let mut pos = 0;
    while pos < data.len() {
        let kind = data[pos];
        pos += 1;
        // Some ArduPilot versions (seen on 4.7.2) add a second NUL after every entry, and the
        // vehicle zeroes the unused tail. A stray NUL is not a record: counting it would also
        // shift the offset of the next page.
        if kind == 0 {
            continue;
        }
        let end = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .map_or(data.len(), |i| pos + i);
        let text = String::from_utf8_lossy(&data[pos..end]).into_owned();
        pos = (end + 1).min(data.len().max(end + 1));
        records += 1;
        let mut fields = text.split('\t');
        let name = fields.next().unwrap_or("").to_string();
        let rest: Vec<&str> = fields.collect();
        let modified = |s: Option<&&str>| {
            s.and_then(|v| v.parse::<u64>().ok())
                .filter(|&t| t != 0 && t <= u64::from(u32::MAX))
                .map(|t| t as u32)
        };
        match kind {
            b'F' => {
                let size = rest.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                entries.push(DirEntry {
                    name,
                    is_dir: false,
                    size,
                    modified: if with_time {
                        modified(rest.get(1))
                    } else {
                        None
                    },
                });
            }
            b'D' => entries.push(DirEntry {
                // A plain listing is just the name; a timed one has "<name>\t<size>\t<mtime>".
                name: if with_time { name } else { text.clone() },
                is_dir: true,
                size: 0,
                modified: if with_time {
                    modified(rest.get(1))
                } else {
                    None
                },
            }),
            b'S' => {}
            _ => {
                if !text.is_empty() {
                    entries.push(DirEntry {
                        name: text,
                        is_dir: false,
                        size: 0,
                        modified: None,
                    });
                }
            }
        }
    }
    (entries, records)
}

/// CRC32 as ArduPilot computes it for `CalcFileCRC32`: reflected, polynomial 0xEDB88320,
/// initial value 0 and no final xor (so not the zlib CRC; `crc32(0, ..)` of "123456789" is
/// 0x2DFD2D88).
pub fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Mission Planner's friendly errno names for `FailErrno`.
pub fn errno_name(errno: u8) -> &'static str {
    match errno {
        1 => "EPERM",
        2 => "ENOENT",
        5 => "EIO",
        13 => "EACCES",
        17 => "EEXIST",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        28 => "ENOSPC",
        _ => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_autopilot_variant() {
        // reflected CRC-32 with no init/final xor of "123456789"
        assert_eq!(crc32(0, b"123456789"), 0x2DFD_2D88);
        assert_eq!(crc32(0, b""), 0);
        // chunked computation continues the running value
        assert_eq!(crc32(crc32(0, b"1234"), b"56789"), crc32(0, b"123456789"));
    }

    #[test]
    fn frame_round_trips() {
        let frame = Frame::request(513, Opcode::WriteFile)
            .with_session(3)
            .with_offset(0x0102_0304)
            .with_data(b"hello".to_vec());
        let bytes = frame.encode().unwrap();
        assert_eq!(&bytes[..12], &[1, 2, 3, 7, 5, 0, 0, 0, 4, 3, 2, 1]);
        assert_eq!(&bytes[12..17], b"hello");
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
    }

    #[test]
    fn decode_clamps_size_and_rejects_bad_input() {
        let mut bytes = [0u8; PAYLOAD_LEN];
        bytes[3] = 128;
        bytes[4] = 255;
        assert_eq!(Frame::decode(&bytes).unwrap().data.len(), MAX_DATA);
        assert_eq!(Frame::decode(&[0; 5]), Err(FrameError::TooShort(5)));
        bytes[3] = 99;
        assert_eq!(Frame::decode(&bytes), Err(FrameError::UnknownOpcode(99)));
        let too_big = Frame::request(0, Opcode::WriteFile).with_data(vec![0; MAX_DATA + 1]);
        assert_eq!(too_big.encode(), Err(FrameError::TooMuchData(MAX_DATA + 1)));
    }

    #[test]
    fn nak_codes() {
        let nak = |data: &[u8]| {
            let mut f = Frame::request(1, Opcode::Nak).with_data(data.to_vec());
            f.opcode = Opcode::Nak;
            f.nak()
        };
        assert_eq!(nak(&[10]), Some(FtpError::FileNotFound));
        assert_eq!(nak(&[6]), Some(FtpError::Eof));
        assert_eq!(nak(&[2, 13]), Some(FtpError::FailErrno(13)));
        assert_eq!(nak(&[77]), Some(FtpError::Other(77)));
        assert_eq!(Frame::request(1, Opcode::Ack).nak(), None);
    }

    #[test]
    fn plain_listing() {
        let data = b"Dscripts\0Fparam.pck\t1234\0Smissing\0";
        let (entries, records) = parse_listing(data, false);
        assert_eq!(records, 3);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "scripts");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "param.pck");
        assert_eq!(entries[1].size, 1234);
        assert_eq!(entries[1].modified, None);
    }

    #[test]
    fn timed_listing() {
        let data = b"Dlogs\t0\t1700000000\0Fa.lua\t10\t0\0Fb.lua\t20\t99999999999\0";
        let (entries, records) = parse_listing(data, true);
        assert_eq!(records, 3);
        assert_eq!(entries[0].name, "logs");
        assert_eq!(entries[0].modified, Some(1_700_000_000));
        // zero and out-of-range times are "unknown"
        assert_eq!(entries[1].modified, None);
        assert_eq!(entries[2].modified, None);
        assert_eq!(entries[2].size, 20);
    }

    #[test]
    fn listing_tolerates_a_missing_terminator() {
        let (entries, records) = parse_listing(b"Fa\t5", false);
        assert_eq!((entries.len(), records), (1, 1));
        assert_eq!(entries[0].size, 5);
        assert_eq!(parse_listing(b"", false).1, 0);
    }

    #[test]
    fn doubled_terminators_are_not_records() {
        // ArduCopter 4.7.2 on a Cube Orange, /APM: every entry is followed by two NULs.
        let data = b"DSTRG_BAK\0\0DLOGS\0\0Dscripts\0\0Ffence.stg\t65535\0\0";
        let (entries, records) = parse_listing(data, false);
        assert_eq!(records, 4);
        let names: Vec<_> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.is_dir, e.size))
            .collect();
        assert_eq!(
            names,
            [
                ("STRG_BAK", true, 0),
                ("LOGS", true, 0),
                ("scripts", true, 0),
                ("fence.stg", false, 65535)
            ]
        );
    }
}
