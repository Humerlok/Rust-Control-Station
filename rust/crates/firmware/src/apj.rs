//! `.apj` firmware files: JSON with a base64, zlib-compressed flash image.
use std::io::Read;

use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::ZlibDecoder;
use serde::Deserialize;

use crate::{bootloader::crc32_pad, FirmwareError};

/// Largest image accepted; real boards have at most a few MB of flash.
pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Deserialize)]
struct RawApj {
    #[serde(default)]
    magic: String,
    board_id: Option<u32>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    git_identity: String,
    #[serde(default)]
    ardupilot_git_hash: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    image_size: usize,
    #[serde(default)]
    extf_image_size: usize,
}

/// A decoded firmware image ready to be programmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firmware {
    /// The board this image was built for (the bootloader reports the same number).
    pub board_id: u32,
    pub description: String,
    pub version: String,
    pub git_hash: String,
    /// The flash image, padded with 0xFF to a multiple of 4 bytes.
    pub image: Vec<u8>,
    /// Size declared by the file, before padding.
    pub image_size: usize,
    /// Size of an external-flash image; images that carry one are not supported yet.
    pub extf_image_size: usize,
}

impl Firmware {
    /// The CRC the bootloader reports for flash that holds this image followed by erased
    /// (0xFF) flash up to `flash_size` bytes.
    pub fn crc(&self, flash_size: u32) -> u32 {
        crc32_pad(&self.image, flash_size)
    }
}

pub fn parse_apj(bytes: &[u8]) -> Result<Firmware, FirmwareError> {
    let raw: RawApj = serde_json::from_slice(bytes)
        .map_err(|e| FirmwareError::BadFile(format!("not valid JSON ({e})")))?;
    if !raw.magic.starts_with("APJFW") {
        return Err(FirmwareError::BadFile(
            "missing the APJFW marker; this is not an .apj firmware file".into(),
        ));
    }
    let board_id = raw
        .board_id
        .ok_or_else(|| FirmwareError::BadFile("no board_id".into()))?;
    if raw.image_size == 0 {
        return Err(FirmwareError::BadFile(
            "the file has no flash image (image_size is 0)".into(),
        ));
    }
    if raw.image_size > MAX_IMAGE_BYTES {
        return Err(FirmwareError::BadFile(format!(
            "image_size {} is implausibly large",
            raw.image_size
        )));
    }
    let compressed = STANDARD
        .decode(raw.image.trim())
        .map_err(|e| FirmwareError::Corrupt(format!("image is not valid base64 ({e})")))?;
    let mut image = Vec::with_capacity(raw.image_size + 3);
    ZlibDecoder::new(compressed.as_slice())
        .take(raw.image_size as u64 + 1)
        .read_to_end(&mut image)
        .map_err(|e| FirmwareError::Corrupt(format!("image does not decompress ({e})")))?;
    if image.len() != raw.image_size {
        return Err(FirmwareError::Corrupt(format!(
            "image is {} bytes after decompression but the file says {}",
            image.len(),
            raw.image_size
        )));
    }
    while !image.len().is_multiple_of(4) {
        image.push(0xFF);
    }
    Ok(Firmware {
        board_id,
        description: if raw.description.is_empty() {
            raw.summary
        } else {
            raw.description
        },
        version: raw.version,
        git_hash: if raw.git_identity.is_empty() {
            raw.ardupilot_git_hash
        } else {
            raw.git_identity
        },
        image,
        image_size: raw.image_size,
        extf_image_size: raw.extf_image_size,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;

    /// Builds an `.apj` file around `image` for `board_id`.
    pub(crate) fn make_apj(board_id: u32, image: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(image).unwrap();
        let compressed = encoder.finish().unwrap();
        serde_json::json!({
            "magic": "APJFWv1",
            "board_id": board_id,
            "description": "Firmware for a CubeOrange",
            "image": STANDARD.encode(compressed),
            "image_size": image.len(),
            "git_identity": "abc123",
            "extf_image_size": 0,
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn decodes_and_pads_to_four_bytes() {
        let fw = parse_apj(&make_apj(140, &[1, 2, 3, 4, 5])).unwrap();
        assert_eq!(fw.board_id, 140);
        assert_eq!(fw.image_size, 5);
        assert_eq!(fw.image, vec![1, 2, 3, 4, 5, 0xFF, 0xFF, 0xFF]);
        assert_eq!(fw.git_hash, "abc123");
        assert_eq!(fw.description, "Firmware for a CubeOrange");
    }

    #[test]
    fn rejects_files_that_are_not_firmware() {
        assert!(matches!(
            parse_apj(b"not json"),
            Err(FirmwareError::BadFile(_))
        ));
        assert!(matches!(
            parse_apj(br#"{"board_id": 1, "image_size": 4}"#),
            Err(FirmwareError::BadFile(m)) if m.contains("APJFW")
        ));
        assert!(matches!(
            parse_apj(br#"{"magic": "APJFWv1", "image_size": 4}"#),
            Err(FirmwareError::BadFile(m)) if m.contains("board_id")
        ));
        assert!(matches!(
            parse_apj(br#"{"magic": "APJFWv1", "board_id": 1}"#),
            Err(FirmwareError::BadFile(_))
        ));
    }

    #[test]
    fn a_size_mismatch_or_bad_data_is_corruption() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&make_apj(9, &[0u8; 64])).unwrap();
        value["image_size"] = 128.into();
        assert!(matches!(
            parse_apj(value.to_string().as_bytes()),
            Err(FirmwareError::Corrupt(m)) if m.contains("64")
        ));
        value["image_size"] = 64.into();
        value["image"] = "!!!!".into();
        assert!(matches!(
            parse_apj(value.to_string().as_bytes()),
            Err(FirmwareError::Corrupt(_))
        ));
        value["image"] = STANDARD.encode(b"plain, not zlib").into();
        assert!(matches!(
            parse_apj(value.to_string().as_bytes()),
            Err(FirmwareError::Corrupt(_))
        ));
    }

    #[test]
    fn crc_covers_the_erased_flash_after_the_image() {
        let fw = parse_apj(&make_apj(9, &[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        let mut flash = fw.image.clone();
        flash.resize(32, 0xFF);
        assert_eq!(fw.crc(32), crate::bootloader::crc32(&flash, 0));
    }
}
