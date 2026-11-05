//! The PX4/ArduPilot serial bootloader protocol: request builders, status checking, the CRC
//! the bootloader reports, and board compatibility rules. Every request ends with `EOC`; every
//! reply ends with `INSYNC` followed by `OK` (or an error status).
use serde::Serialize;

pub const EOC: u8 = 0x20;
pub const BL_REV_MIN: u32 = 3;
pub const BL_REV_MAX: u32 = 20;
/// Bytes per PROG_MULTI request: the protocol allows 255 but the size must be a multiple of 4.
pub const PROG_MULTI_MAX: usize = 252;

/// Reply bytes.
pub mod reply {
    pub const OK: u8 = 0x10;
    pub const FAILED: u8 = 0x11;
    pub const INSYNC: u8 = 0x12;
    pub const INVALID: u8 = 0x13;
    pub const BAD_SILICON_REV: u8 = 0x14;
}

mod code {
    pub const GET_SYNC: u8 = 0x21;
    pub const GET_DEVICE: u8 = 0x22;
    pub const CHIP_ERASE: u8 = 0x23;
    pub const PROG_MULTI: u8 = 0x27;
    pub const GET_CRC: u8 = 0x29;
    pub const GET_SN: u8 = 0x2b;
    pub const GET_CHIP: u8 = 0x2c;
    pub const GET_CHIP_DES: u8 = 0x2e;
    pub const REBOOT: u8 = 0x30;
}

/// `GET_DEVICE` selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Info {
    BlRev = 1,
    BoardId = 2,
    BoardRev = 3,
    FlashSize = 4,
    ExtfSize = 6,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BootError {
    #[error("the board is not answering like a bootloader (got {0:#04x} instead of INSYNC)")]
    NotInSync(u8),
    #[error("the bootloader rejected the request as invalid")]
    Invalid,
    #[error("the bootloader reports that the operation failed")]
    Failed,
    #[error("the bootloader reports a bad silicon revision")]
    BadSilicon,
    #[error("unexpected status {0:#04x} from the bootloader")]
    Unexpected(u8),
}

/// Checks the two status bytes that close every reply.
pub fn check_status(insync: u8, status: u8) -> Result<(), BootError> {
    if insync != reply::INSYNC {
        return Err(BootError::NotInSync(insync));
    }
    match status {
        reply::OK => Ok(()),
        reply::INVALID => Err(BootError::Invalid),
        reply::FAILED => Err(BootError::Failed),
        reply::BAD_SILICON_REV => Err(BootError::BadSilicon),
        other => Err(BootError::Unexpected(other)),
    }
}

/// Requests, as the exact bytes to send.
pub mod request {
    use super::{code, Info, EOC, PROG_MULTI_MAX};

    pub fn get_sync() -> Vec<u8> {
        vec![code::GET_SYNC, EOC]
    }

    pub fn get_device(info: Info) -> Vec<u8> {
        vec![code::GET_DEVICE, info as u8, EOC]
    }

    pub fn chip_erase() -> Vec<u8> {
        vec![code::CHIP_ERASE, EOC]
    }

    /// `data` must be at most [`PROG_MULTI_MAX`] bytes and a multiple of 4.
    pub fn prog_multi(data: &[u8]) -> Vec<u8> {
        assert!(
            data.len() <= PROG_MULTI_MAX && data.len().is_multiple_of(4),
            "PROG_MULTI chunks are at most {PROG_MULTI_MAX} bytes and a multiple of 4"
        );
        let mut frame = Vec::with_capacity(data.len() + 3);
        frame.push(code::PROG_MULTI);
        frame.push(data.len() as u8);
        frame.extend_from_slice(data);
        frame.push(EOC);
        frame
    }

    pub fn get_crc() -> Vec<u8> {
        vec![code::GET_CRC, EOC]
    }

    pub fn get_chip() -> Vec<u8> {
        vec![code::GET_CHIP, EOC]
    }

    pub fn get_chip_des() -> Vec<u8> {
        vec![code::GET_CHIP_DES, EOC]
    }

    /// Reads one 32-bit word of the unique id at byte offset `address`.
    pub fn get_sn(address: u32) -> Vec<u8> {
        let mut frame = vec![code::GET_SN];
        frame.extend_from_slice(&address.to_le_bytes());
        frame.push(EOC);
        frame
    }

    pub fn reboot() -> Vec<u8> {
        vec![code::REBOOT, EOC]
    }
}

/// What the bootloader says about the board it runs on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardInfo {
    pub bootloader_rev: u32,
    pub board_id: u32,
    pub board_rev: u32,
    /// Usable flash for firmware, in bytes (0 when the bootloader does not say).
    pub flash_size: u32,
    /// External flash in bytes (0 when none).
    pub extf_size: u32,
    pub chip: Option<u32>,
    pub chip_description: Option<String>,
    /// Unique id as uppercase hex, when readable.
    pub serial: Option<String>,
}

/// An image for `firmware_board` may go on a board that identifies as `board`. A board id 33
/// is also accepted for fmuv2 (9) images, like the legacy Mission Planner.
pub fn boards_compatible(board: u32, firmware_board: u32) -> bool {
    board == firmware_board || (board == 33 && firmware_board == 9)
}

/// Splits `data` into `size`-byte pieces; the last one may be shorter.
pub fn chunks(data: &[u8], size: usize) -> impl Iterator<Item = &[u8]> {
    data.chunks(size)
}

const fn make_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static TABLE: [u32; 256] = make_table();

/// The bootloader's CRC32: reflected 0xEDB88320, no final xor (not zlib's), continued from
/// `state` (start with 0).
pub fn crc32(bytes: &[u8], mut state: u32) -> u32 {
    for &byte in bytes {
        state = TABLE[((state ^ byte as u32) & 0xFF) as usize] ^ (state >> 8);
    }
    state
}

/// The CRC of `image` followed by erased flash (0xFF words) up to `flash_size`, which is what
/// `GET_CRC` returns after programming.
pub fn crc32_pad(image: &[u8], flash_size: u32) -> u32 {
    let mut state = crc32(image, 0);
    let mut offset = image.len() as u64;
    while offset + 1 < flash_size as u64 {
        state = crc32(&[0xFF; 4], state);
        offset += 4;
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent bit-by-bit implementation for cross-checking the table.
    fn crc_bitwise(bytes: &[u8]) -> u32 {
        let mut crc = 0u32;
        for &b in bytes {
            crc ^= b as u32;
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

    #[test]
    fn crc_matches_the_bitwise_definition_and_is_not_zlib() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        assert_eq!(crc32(&data, 0), crc_bitwise(&data));
        // zlib's crc32("123456789") is 0xCBF43926; without init/final xor it differs.
        assert_ne!(crc32(b"123456789", 0), 0xCBF4_3926);
        assert_eq!(crc32(b"123456789", 0), crc_bitwise(b"123456789"));
        // Continuing from a state equals one pass.
        assert_eq!(crc32(&data[500..], crc32(&data[..500], 0)), crc32(&data, 0));
    }

    #[test]
    fn padding_matches_the_reference_loop() {
        // Reference: for i in range(len(image), flash_size - 1, 4): crc over 4 * 0xFF.
        let image = [9u8; 8];
        let mut flash = image.to_vec();
        flash.resize(40, 0xFF);
        assert_eq!(crc32_pad(&image, 40), crc32(&flash, 0));
        // A flash size that is not a multiple of 4 rounds the pad up, like the reference.
        let mut flash = image.to_vec();
        flash.resize(12, 0xFF);
        assert_eq!(crc32_pad(&image, 10), crc32(&flash, 0));
        // An image as large as the flash is not padded.
        assert_eq!(crc32_pad(&image, 8), crc32(&image, 0));
        assert_eq!(crc32_pad(&image, 0), crc32(&image, 0));
    }

    #[test]
    fn requests_are_byte_exact() {
        assert_eq!(request::get_sync(), [0x21, 0x20]);
        assert_eq!(request::get_device(Info::BoardId), [0x22, 2, 0x20]);
        assert_eq!(request::get_device(Info::FlashSize), [0x22, 4, 0x20]);
        assert_eq!(request::chip_erase(), [0x23, 0x20]);
        assert_eq!(request::get_crc(), [0x29, 0x20]);
        assert_eq!(request::reboot(), [0x30, 0x20]);
        assert_eq!(request::get_sn(8), [0x2b, 8, 0, 0, 0, 0x20]);
        assert_eq!(
            request::prog_multi(&[1, 2, 3, 4]),
            [0x27, 4, 1, 2, 3, 4, 0x20]
        );
        let big = [0u8; PROG_MULTI_MAX];
        assert_eq!(request::prog_multi(&big).len(), PROG_MULTI_MAX + 3);
    }

    #[test]
    #[should_panic]
    fn oversized_chunks_are_a_programming_error() {
        request::prog_multi(&[0u8; PROG_MULTI_MAX + 4]);
    }

    #[test]
    fn status_bytes_map_to_errors() {
        assert_eq!(check_status(0x12, 0x10), Ok(()));
        assert_eq!(check_status(0x12, 0x11), Err(BootError::Failed));
        assert_eq!(check_status(0x12, 0x13), Err(BootError::Invalid));
        assert_eq!(check_status(0x12, 0x14), Err(BootError::BadSilicon));
        assert_eq!(check_status(0x12, 0x55), Err(BootError::Unexpected(0x55)));
        assert_eq!(check_status(0x00, 0x10), Err(BootError::NotInSync(0)));
    }

    #[test]
    fn board_compatibility_is_exact_except_the_fmuv2_alias() {
        assert!(boards_compatible(140, 140));
        assert!(!boards_compatible(140, 9));
        assert!(!boards_compatible(9, 33));
        assert!(boards_compatible(33, 9));
    }

    #[test]
    fn chunking_covers_every_byte() {
        let data: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        let parts: Vec<&[u8]> = chunks(&data, PROG_MULTI_MAX).collect();
        assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), 1000);
        assert!(parts[..parts.len() - 1]
            .iter()
            .all(|p| p.len() == PROG_MULTI_MAX));
    }
}
