//! Pure firmware logic for ArduPilot boards: the firmware manifest, `.apj` images, and the
//! PX4/ArduPilot serial bootloader wire protocol. No I/O lives here; `control-station-link`
//! drives a serial port with these pieces.
mod apj;
mod bootloader;
mod manifest;

pub use apj::{parse_apj, Firmware, MAX_IMAGE_BYTES};
pub use bootloader::{
    boards_compatible, check_status, chunks, crc32, crc32_pad, reply, request, BoardInfo,
    BootError, Info, BL_REV_MAX, BL_REV_MIN, EOC, PROG_MULTI_MAX,
};
pub use manifest::{
    boards, is_trusted_url, parse_manifest, parse_manifest_gz, select, BoardSummary, ManifestEntry,
    ManifestError, Query, MANIFEST_URL,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FirmwareError {
    #[error("not a firmware file: {0}")]
    BadFile(String),
    #[error("firmware file is damaged: {0}")]
    Corrupt(String),
}
